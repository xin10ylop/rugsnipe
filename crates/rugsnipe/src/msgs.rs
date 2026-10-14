//! Messages between the threads. Everything the hot thread sends is `Copy`
//! (or a fixed-size `Template`) so the `rtrb` rings never allocate.

use pump_codec::Pubkey;
use rugsnipe_exit::{ExitReason, Position, Venue};
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
        prio: u64,
        tip: u64,
        sent_ns: u64,
        result: u8,
    },
    /// Paper buy resolved (filled at the E2 state or unfilled above `v_max`).
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
    /// Resolve the paper fill of an intent (E2 lane).
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
    },
    /// Funding transfer from the feed: update the watchlist (copy-on-write, cold side).
    Funding(FundingEvent),
    /// First dump signal: burn the intent's nonce now.
    BurnNonce {
        idx: u16,
    },
    /// A position closed with realized P&L (lamports).
    Closed {
        mint: Pubkey,
        op_slot: usize,
        pnl: i64,
    },
}

/// Cold thread -> hot thread.
#[derive(Debug, Clone, Copy)]
pub enum HotIn {
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
