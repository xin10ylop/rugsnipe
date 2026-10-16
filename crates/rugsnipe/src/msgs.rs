//! Messages between the threads. Everything the hot thread sends is `Copy`
//! (or a fixed-size `Template`) so the `rtrb` rings never allocate.

use pump_codec::Pubkey;
use rugsnipe_core::rslog::ShadowOutcome;
use rugsnipe_exit::{ExitReason, Position, Trade, Venue};
use rugsnipe_feed::{FeedSrc, FundingEvent};
use rugsnipe_tx::template::Template;

/// Hot (or sender) thread -> cold thread.
#[derive(Debug, Clone, Copy)]
pub enum ColdMsg {
    /// A decision produced an intent (step 14).
    Intent {
        intent: u64,
        mint: Pubkey,
        op_id: u32,
        armset_ver: u64,
        slot: u64,
        bank_id: u64,
        via: u8,
        src: FeedSrc,
        spend: u64,
        v_max: u64,
        min_out: u64,
        nonce_idx: u16,
        nonce_value: [u8; 32],
        decided_ns: u64,
        real: bool,
    },
    /// One variant handed to one sender.
    Sent {
        intent: u64,
        sig: [u8; 64],
        variant: u8,
        sender: u8,
        /// The sender's ledger name (`jito_frankfurt`, `helius_swqos`, ...).
        label: &'static str,
        prio: u64,
        tip: u64,
        /// The base bid B the plan was split from.
        bid: u64,
        sent_ns: u64,
        sent_unix_ms: u64,
        result: u8,
        /// Key-free failure name (`""` when accepted / recorded).
        err: &'static str,
        /// HTTP status of an `http` failure.
        http: u16,
        /// `VariantKind` byte (buy / sell / burn).
        kind: u8,
    },
    /// A real intent (probe): what the real fill lane needs to find our
    /// transaction, read our TradeEvent and rank it in its slot (sent right
    /// after its `Intent`).
    RealPending {
        intent: u64,
        mint: Pubkey,
        bc: Pubkey,
        create_sig: [u8; 64],
        slot: u64,
        creator: Pubkey,
        user: Pubkey,
        nonce_idx: u16,
        nonce_value: [u8; 32],
        spend: u64,
        bid: u64,
    },
    /// A real position is open (or still held): poll its token balance.
    RealHeld {
        intent: u64,
        mint: Pubkey,
    },
    /// Paper buy resolved (filled at the E1/E2 state, or failed: E2 above
    /// `v_max`, or an E1 failure with its reason and cost).
    Fill {
        intent: u64,
        mint: Pubkey,
        sig: [u8; 64],
        slot: u64,
        tokens: u64,
        sol_delta: i64,
        filled: bool,
        late: bool,
        buyers_ahead: u32,
        rival_sol_ahead: u64,
        /// Why the buy would have failed (`None` for a fill or an E2 miss).
        fail: Option<ShadowOutcome>,
        /// What the failed attempt cost (lamports).
        fail_cost: u64,
    },
    /// The shadow-mode entry of a resolved paper intent (docs/04 section 7).
    Shadow {
        intent: u64,
        mint: Pubkey,
        e1: bool,
        outcome: ShadowOutcome,
        info: ShadowInfo,
        spend: u64,
        cost: u64,
    },
    /// Position snapshot (open / after a fill / closed).
    Position(Position),
    /// A sell fill.
    Exit {
        mint: Pubkey,
        sig: [u8; 64],
        reason: ExitReason,
        venue: Venue,
        tokens: u64,
        sol_delta: i64,
        slot: u64,
        owner: Pubkey,
        try_no: u32,
    },
    /// Watch the mint's trades (LogsWs per-address subscription).
    WatchMint(Pubkey),
    UnwatchMint(Pubkey),
    /// Resolve the paper fill of an intent (E1 or E2 lane).
    PaperPending {
        intent: u64,
        mint: Pubkey,
        bc: Pubkey,
        create_sig: [u8; 64],
        slot: u64,
        /// Curve after the create's in-tx buys (E2 when nothing else traded).
        v0: u64,
        t0: u64,
        decided_unix_ms: u64,
        /// Creator argument and create signer (the creator set's known part).
        creator: Pubkey,
        user: Pubkey,
        intx_buys: u8,
        /// Creates the hot loop saw in the 60 / 600 s before this one
        /// (entry filter; `u32::MAX` = window not covered yet).
        rate60: u32,
        rate600: u32,
    },
    /// The entry filter's verdict on a resolved paper intent (sent before
    /// its `Shadow`): `scored = false` when the intent had no features.
    FilterScore {
        intent: u64,
        mint: Pubkey,
        scored: bool,
        score: f64,
        tertile: u8,
        skipped: bool,
    },
    /// Funding transfer from the feed: update the watchlist (copy-on-write, cold side).
    Funding(FundingEvent),
    /// First dump signal: burn the intent's nonce now (only while `intent`
    /// still holds it; a later claim by another mint is left alone).
    BurnNonce {
        idx: u16,
        intent: u64,
    },
    /// A position closed with realized P&L (lamports).
    Closed {
        mint: Pubkey,
        op_slot: usize,
        pnl: i64,
    },
}

/// The creation slot as the E1 lane reconstructed it (`shadow` row).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShadowInfo {
    pub create_slot: u64,
    pub create_index: Option<u32>,
    pub block_txs: Option<u32>,
    pub bundle_trades: u32,
    pub bundle_wallets: u32,
    pub our_index: Option<u32>,
    pub tp_ahead: u32,
    pub tp_behind: u32,
    pub s0_failed: u32,
    /// Premium the fill pays over the post-bundle price (bps).
    pub premium_bps: u32,
    pub post_bundle_v: u64,
    pub post_bundle_t: u64,
    /// Post-bundle market cap, lamports.
    pub post_bundle_mcap: u64,
}

/// Cold thread -> hot thread.
#[derive(Debug, Clone, Copy)]
pub enum HotIn {
    /// E1 verdict on a paper intent. `Filled`: the position opens at the
    /// post-bundle state `(ref_v, ref_t)` with `info.premium_bps`, then the
    /// lane's [`HotIn::E1Insider`], [`HotIn::E1Trade`] and [`HotIn::E1End`]
    /// follow in order. Any other outcome closes the intent.
    E1Open {
        intent: u64,
        mint: Pubkey,
        outcome: ShadowOutcome,
        info: ShadowInfo,
    },
    /// A bundle wallet (`creator = false`) or creator-set key (`true`) for
    /// the insider-sell trigger.
    E1Insider {
        intent: u64,
        key: Pubkey,
        creator: bool,
    },
    /// One historical trade after our landing (reserves without us).
    E1Trade { intent: u64, trade: Trade },
    /// The entry filter's feature vector of a paper intent (sent right
    /// before its [`HotIn::E1Open`], which scores it).
    E1Feat {
        intent: u64,
        feat: crate::filter::Feat,
        /// LEAD outcomes known for the signer / the best-known bundle wallet.
        op_lead_n: u32,
        bw_lead_n: u32,
    },
    /// The replay tape is complete up to `horizon`.
    E1End { intent: u64, horizon: u64 },
    /// Real (probe): our buy landed and succeeded; `trade` is our own
    /// TradeEvent from the landed transaction (the position opens on it).
    RealBuy {
        intent: u64,
        sig: [u8; 64],
        trade: Trade,
    },
    /// Real: our buy landed and failed (`cost` = its `meta.fee`), or
    /// nothing landed and the nonce moved on (`dropped`, cost 0).
    RealFailed {
        intent: u64,
        cost: u64,
        dropped: bool,
    },
    /// Real: one of our own trades on a held mint (a sell, or a buy variant
    /// that beat the burn), from the landed transaction's TradeEvent.
    RealTrade { sig: [u8; 64], trade: Trade },
    /// Real: the chain token balance of a held mint.
    RealBalance {
        mint: Pubkey,
        tokens: u64,
        slot: u64,
    },
    /// The E2 state for a paper intent (`ok = false`: resolution failed,
    /// the hot loop falls back to its buffered trades at the deadline).
    PaperFill {
        intent: u64,
        mint: Pubkey,
        v: u64,
        t: u64,
        slot: u64,
        buyers_ahead: u32,
        rival_sol_ahead: u64,
        ok: bool,
    },
}

/// Hot thread -> sender thread: a patched template to re-price, re-sign and
/// send as variant `idx`.
#[derive(Debug, Clone)]
pub struct FireJob {
    pub tmpl: Template,
    pub intent: u64,
    pub idx: u8,
    pub prio: u64,
    pub tip: u64,
    pub tip_account: Pubkey,
    /// The base bid B (before the class split; ledger `sends.bid`).
    pub bid: u64,
    /// `true` only for a `Decision::Fire` intent (or a sell of its position)
    /// in probe/live. Anything else (paper mode, or a decision forced to paper
    /// in a real mode: regime freeze, ArmSet gate, op not armed) goes to the
    /// paper recorder only, never to a real sender.
    pub real: bool,
}

/// rslog / metrics code of an exit reason.
pub fn reason_code(r: ExitReason) -> u8 {
    match r {
        ExitReason::BundleSell => 1,
        ExitReason::Consolidation => 2,
        ExitReason::BigBuy => 3,
        ExitReason::Trailing => 5,
        ExitReason::PhantomSl => 6,
        ExitReason::NoActivity => 7,
        ExitReason::MaxHold => 8,
        ExitReason::TakeProfit(i) => 0x10 + i.min(15),
    }
}

/// Index of an exit reason in [`crate::stats::EXIT_REASONS`].
pub fn reason_index(r: ExitReason) -> usize {
    match r {
        ExitReason::BundleSell => 0,
        ExitReason::Consolidation => 1,
        ExitReason::BigBuy => 2,
        ExitReason::TakeProfit(_) => 3,
        ExitReason::Trailing => 4,
        ExitReason::PhantomSl => 5,
        ExitReason::NoActivity => 6,
        ExitReason::MaxHold => 7,
    }
}
