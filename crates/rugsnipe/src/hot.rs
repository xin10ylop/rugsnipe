//! The hot loop (docs/06 section 7.1), run to completion on one thread:
//!
//! ```text
//! 1  ev = feed.poll()                                  (FeedMux: cross-feed dedup by signature)
//! 2  FiredSet.first(mint, class)                       (free feeds use their own class: never claim a live slot)
//! 3  hit = ev.hit (logs) | create_hit(TxView)          (wire)
//! 4  class filters: SOL quote, not mayhem, Token-2022, not cashback
//! 5  watchlist: signer / fee payer = live; creator_arg only = shadow
//! 6  age <= max_age_slots
//! 7  RiskGate.decide (paper: would_skip)
//! 8  V0 (curve after every in-tx buy) < op.v_max; a real decision after the
//!    create slot (slot_now > create slot) is refused (`late_slot`)
//! 9  A/B skip (probe/live only)
//! 10 nonce claim xxh3(mint) % N, value older than the create slot (paper: simulated pool)
//! 11 size, min_tokens_out at V_cap = min(v_max, V_pb * sqrt(1 + entry.max_premium)), V_pb the bundle
//!    run's observed post-trade reserve (entry.require_bundle) else V0 + entry.bundle_lamports
//! 12 patch the template of that nonce
//! 13 sign; primary variant to the paper sender (paper) / sender thread (real); secondary variant to the sender thread
//! 14 record Decision + Sent (rslog), Intent + Sent (ledger via the cold ring)
//! ```
//!
//! Every create gets a `CreateHit` and a `Decision` record (hit or miss).
//! Paper positions open on the configured fill model (`paper.fill_model`):
//!
//! * **E1** (default; docs/04 sections 2 and 7): the cold lane
//!   ([`crate::e1`]) reconstructs the creation slot and returns a verdict.
//!   A fill opens at the post-bundle state with the premium (unless the
//!   real buy's on-chain bound, `min_tokens_out` at `V_cap`, fails it:
//!   `slippage`; paper and live share one bound), then the lane's tape of
//!   the next slots is replayed through the exit engine on a slot clock
//!   (`ms = (slot - landing) * slot_ms`, a tick per slot), on the
//!   counterfactual curve that keeps our tokens outstanding (others' trades
//!   arrive without us: `t' = t - ours`, `v' = v t / t'`). A failed attempt
//!   (`premium`, `slippage`, `not_in_slot0`) is recorded with its priority
//!   fee; a `class` miss (not multi-bundle) is a skip at no cost.
//! * **E2**: the end of slot+1 (the cold thread's RPC lane).
//!
//! Without a fill lane (tests, `--no-e2`) every model falls back at the
//! deadline to E2 from the mint's buffered trades. The exit engine
//! (`rugsnipe_exit::decide_into`) drives paper sells whose fills are
//! simulated with the conformance-tape rule: one chance at the reserves at
//! the end of the trigger slot + `paper.exit_latency_slots`,
//! `net >= min_sol_output`.
//!
//! No heap allocation after warm-up: positions live in a fixed table, the
//! exit action buffer is reused, every message is `Copy` into an `rtrb`
//! ring, metrics are pre-created atomics (checked by `rugsnipe-replay`).

use crate::convert::{hit_from_lite, trade_from_lite, v0_of};
use crate::filter::{CreateRate, Feat, Model};
use crate::leader::{ClientClass, LeaderSnapshot, UNKNOWN as LEADER_UNKNOWN};
use crate::msgs::{reason_code, reason_index, ColdMsg, FireJob, HotIn, ShadowInfo};
use crate::setup::{min_tokens_out, op_armed, ArmState, Templates};
use crate::stats::{code, HotStats};
use arc_swap::ArcSwap;
use hdrhistogram::Histogram;
use pump_codec::constants::{TOKEN_2022_PROGRAM_ID, WSOL_MINT};
use pump_codec::create_hit::CreateHit;
use pump_codec::curve::{CurveState, FeeBps};
use pump_codec::{pda, Pubkey};
use rugsnipe_core::bid::{BidPlan, BidPolicy, LeaderClass};
use rugsnipe_core::config::{Config, FillModel, FilterMode, LeaderMix, ProbeStage};
use rugsnipe_core::risk::{DecideInput, Decision, RiskGate};
use rugsnipe_core::rslog::{CreateHitRec, Rec, RslogProducer, ShadowOutcome};
use rugsnipe_core::watchlist::{Match, Via, Watchlist};
use rugsnipe_core::{IntentId, Mode};
use rugsnipe_exit::{
    decide_into, BundleSet, ExitAction, ExitEvent, ExitPolicy, Now, Position, PositionState,
    SellOrder, Trade, Venue,
};
use rugsnipe_feed::bundle::RUN_MAX_AFTER;
use rugsnipe_feed::clock::mono_ns;
use rugsnipe_feed::event::{RunDoneLite, TradeLite};
use rugsnipe_feed::health::MissKind;
use rugsnipe_feed::wiremeta::{WireMeta, EMPTY_INNER, MAX_INNER, MAX_LOADED};
use rugsnipe_feed::{Feed, FeedBuf, FeedEvent, FeedSrc, FeedStatus, Fired, FiredSet, Payload};
use rugsnipe_tx::nonce::NoncePool;
use rugsnipe_tx::senders::{Sender as _, SenderId, SenderSet, Variant, VariantKind};
use rugsnipe_tx::signer::HotSigner;
use rugsnipe_tx::template::{MintKeys, Patch};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::*};
use std::sync::Arc;

/// Positions tracked at once (open or waiting for their paper fill).
pub const MAX_POS: usize = 64;
/// Trades buffered per position while its paper fill is pending.
pub const TRADE_BUF: usize = 96;
/// Sell attempts waiting for their simulated fill per position.
pub const MAX_ATTEMPTS: usize = 4;
/// FiredSet class of post-execution / wire feeds (may fire).
pub const LIVE_CLASS: u8 = 0;
/// FiredSet class of free feeds (paper only; never claim a live slot).
pub const FREE_CLASS: u8 = 1;
/// Lifetime of paper sells (final sells use a blockhash; paper never sends).
const PAPER_BLOCKHASH: [u8; 32] = [0x11; 32];
/// Slot-clock origin of replayed E1 positions (ms; only differences matter).
const E1_MS_BASE: u64 = 1_000_000_000_000;

/// State shared with the cold and sender threads.
#[derive(Debug)]
pub struct Shared {
    /// Highest slot seen on any feed.
    pub slot_now: AtomicU64,
    /// Incremented every loop iteration (lease / watchdog liveness).
    pub heartbeat: AtomicU64,
    /// This process holds the exits lease (sells allowed).
    pub exits_owned: AtomicBool,
    pub stop: AtomicBool,
    /// Recent blockhash and the slot it was read at (real sells; the cold
    /// lane refreshes it every few seconds). `None` = no real sell possible.
    pub blockhash: std::sync::Mutex<Option<([u8; 32], u64)>>,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            slot_now: AtomicU64::new(0),
            heartbeat: AtomicU64::new(0),
            exits_owned: AtomicBool::new(true),
            stop: AtomicBool::new(false),
            blockhash: std::sync::Mutex::new(None),
        }
    }
}

impl Shared {
    pub fn set_blockhash(&self, hash: [u8; 32], slot: u64) {
        *self.blockhash.lock().unwrap_or_else(|e| e.into_inner()) = Some((hash, slot));
    }
    /// The blockhash if it was read within the last 60 slots of `slot_now`
    /// (a blockhash lives ~150 slots).
    pub fn fresh_blockhash(&self) -> Option<[u8; 32]> {
        let now = self.slot_now.load(Relaxed);
        let g = self.blockhash.lock().unwrap_or_else(|e| e.into_inner());
        g.filter(|(_, at)| now == 0 || now.saturating_sub(*at) <= 60)
            .map(|(h, _)| h)
    }
}

/// Hot-loop settings derived from the config.
#[derive(Debug, Clone)]
pub struct HotCfg {
    pub mode: Mode,
    pub max_age_slots: u64,
    pub freeze: Vec<[u64; 2]>,
    /// Slots after the create before a pending paper fill uses the buffered trades.
    pub fill_timeout_slots: u64,
    /// Base bid B (lamports) before the per-class split.
    pub bid_lamports: u64,
    pub max_bid: u64,
    /// Per-leader-class split, scheduling floor and failed-attempt cap
    /// (`[bid]`, docs/03 section 6 "Pay on success").
    pub bid_policy: BidPolicy,
    /// Class of the create slot's leader when known (keeper / leader
    /// schedule). `None`: the primary variant bids for a tip-ordered leader
    /// (pay on success) and the secondary for a priority-ordered one
    /// (clamped); both ride one durable nonce, so at most one lands.
    pub leader_class: Option<LeaderClass>,
    pub secondary_variant: bool,
    pub policy: ExitPolicy,
    pub fees: FeeBps,
    pub tick_ms: u64,
    pub ab_salt: u64,
    pub ab_enabled: bool,
    /// Paper fill model (`paper.fill_model`).
    pub fill_model: FillModel,
    /// A cold fill lane answers paper intents (E1 verdicts / E2 states).
    /// Without one every model falls back to E2 from buffered trades at the
    /// deadline.
    pub fill_lane: bool,
    /// A paper sell fills at the end of trigger slot + this (`paper.exit_latency_slots`).
    pub sell_lat_slots: u64,
    /// Slot clock of replayed E1 positions (`paper.slot_ms`).
    pub slot_ms: u64,
    /// Fixed cost of a failed E1 attempt (`paper.failed_attempt_lamports`);
    /// 0 = modelled per leader class from `bid_policy` and `leader_mix`.
    pub failed_attempt_lamports: u64,
    /// Leader-class shares of the modelled failed-attempt cost.
    pub leader_mix: LeaderMix,
    /// The entry filter (`entry.filter_model`, loaded at startup) and its
    /// mode: every E1 verdict with creation-slot features is scored here,
    /// on the hot thread, before the position opens.
    pub filter: Option<Arc<Model>>,
    pub filter_mode: FilterMode,
    /// `probe.stage`: a probe buy needs the ArmSet to name no stage or this one.
    pub probe_stage: ProbeStage,
    /// A real mode with the post-execution feed running: free-feed creates
    /// are recorded only (`free_feed_defers`), the live-class copy decides.
    pub post_exec_feed: bool,
    /// `entry.max_premium` in basis points: the fill cap over the expected
    /// post-bundle price that the on-chain bound enforces.
    pub max_premium_bps: u32,
    /// `entry.require_bundle`: real buys fire only on another wallet's buy
    /// of the mint in the create slot (see [`Parked`]).
    pub require_bundle: bool,
    /// `entry.bundle_lamports`: the fallback expectation of the bundle's
    /// SOL outside the create transaction (`V_pb = V0 + this`) when the run
    /// was not observed.
    pub bundle_lamports: u64,
    /// `entry.bundle_wait_ms` in ns: how long a parked create waits after
    /// its first bundle buy for the rest of the run before firing anyway.
    pub bundle_wait_ns: u64,
}

impl HotCfg {
    /// From the TOML. `bid_lamports` is `bid.lamports`, defaulting to the
    /// measured median first-winner bid (docs/03 section 6: 0.0034 SOL),
    /// capped by `max_bid`.
    pub fn from_config(cfg: &Config, policy: ExitPolicy) -> Self {
        let max_bid = if cfg.risk.max_bid_lamports > 0 {
            cfg.risk.max_bid_lamports
        } else {
            25_000_000
        };
        Self {
            mode: cfg.mode.mode,
            max_age_slots: cfg.mode.max_age_slots,
            freeze: cfg.mode.regime_freeze_slots.clone(),
            fill_timeout_slots: 300,
            bid_lamports: match cfg.bid.lamports {
                0 => 3_400_000u64,
                v => v,
            }
            .min(max_bid),
            max_bid,
            bid_policy: BidPolicy::from_cfg(&cfg.bid),
            leader_class: None,
            secondary_variant: true,
            policy,
            fees: FeeBps::DEFAULT,
            tick_ms: 100,
            ab_salt: 0x5eed,
            ab_enabled: cfg.mode.mode.is_real() && cfg.ab.skip_p > 0.0,
            fill_model: cfg.paper.fill_model,
            fill_lane: false,
            sell_lat_slots: cfg.paper.exit_latency_slots,
            slot_ms: cfg.paper.slot_ms.max(1),
            failed_attempt_lamports: cfg.paper.failed_attempt_lamports,
            leader_mix: cfg.paper.leader_mix.clone(),
            filter: None,
            filter_mode: cfg.entry.filter_mode,
            probe_stage: cfg.probe.stage,
            post_exec_feed: false,
            max_premium_bps: (cfg.entry.max_premium * 10_000.0).round().clamp(0.0, 1e6) as u32,
            bundle_lamports: cfg.entry.bundle_lamports,
            bundle_wait_ns: cfg.entry.bundle_wait_ms.saturating_mul(1_000_000),
            require_bundle: cfg.entry.require_bundle,
        }
    }

    /// The virtual SOL reserve the real buy's `min_tokens_out` is computed
    /// at when the bundle run was not observed: `V_cap = min(v_max, V_pb *
    /// sqrt(1 + max_premium))` with `V_pb = v0 + entry.bundle_lamports`
    /// ([`entry_v_cap`]).
    pub fn v_cap(&self, v0: u64, v_max: u64) -> u64 {
        self.v_cap_at(v0.saturating_add(self.bundle_lamports), v_max)
    }

    /// [`HotCfg::v_cap`] at an observed post-bundle reserve `v_pb` (the
    /// bundle run's last trade in the create slot, [`Parked::pb`]).
    pub fn v_cap_at(&self, v_pb: u64, v_max: u64) -> u64 {
        entry_v_cap(v_pb, self.max_premium_bps).min(v_max)
    }

    /// The variant of a sell: the exit engine's sell bid split by the
    /// leader's class (`None`: planned as priority-ordered), so a
    /// tip-ordered class pays the priority floor plus the bid as a tip and a
    /// priority-ordered class the bid as the priority fee, never both
    /// (docs/03 section 6.1).
    pub fn sell_plan(&self, bid: u64, n_sigs: u8) -> BidPlan {
        self.sell_plan_at(None, bid, n_sigs)
    }

    /// [`HotCfg::sell_plan`] for a leader class read from the schedule
    /// (`leader_class`, when set, overrides it).
    pub fn sell_plan_at(&self, class: Option<LeaderClass>, bid: u64, n_sigs: u8) -> BidPlan {
        self.bid_policy
            .plan_sell(self.leader_class.or(class), bid, n_sigs)
    }

    /// The variants of one snipe: the primary for the leader's class (or a
    /// tip-ordered leader when unknown) and, when unknown, a priority-ordered
    /// secondary. Every plan's failed cost is within the cap.
    pub fn bid_plans(&self, bid: u64, n_sigs: u8) -> (BidPlan, BidPlan) {
        self.bid_plans_at(None, None, bid, n_sigs)
    }

    /// The variants for the leaders of the create slot (`cur`) and the next
    /// slot (`next`) from the leader schedule (docs/03 section 6: slot+1
    /// has the same leader 75% of the time). Both known and equal: both
    /// variants bid for that class; different: the primary bids for `cur`,
    /// the secondary for `next`; `cur` unknown: the pay-on-success primary
    /// and the clamped priority secondary. `leader_class`, when set,
    /// overrides the schedule.
    pub fn bid_plans_at(
        &self,
        cur: Option<LeaderClass>,
        next: Option<LeaderClass>,
        bid: u64,
        n_sigs: u8,
    ) -> (BidPlan, BidPlan) {
        let p = &self.bid_policy;
        match (self.leader_class, cur, next) {
            (Some(c), _, _) => (p.plan(c, bid, n_sigs), p.plan(c, bid, n_sigs)),
            (None, Some(c), n) => (p.plan(c, bid, n_sigs), p.plan(n.unwrap_or(c), bid, n_sigs)),
            (None, None, _) => (
                p.plan(LeaderClass::JitoAgave, bid, n_sigs),
                p.plan(LeaderClass::Harmonic, bid, n_sigs),
            ),
        }
    }

    /// What a failed attempt costs (paper): the fixed override, else the
    /// base fee plus the priority fee of the variant matching the leader
    /// (tips revert), weighted by `leader_mix` when the leader is unknown.
    pub fn failed_attempt_cost(&self, bid: u64, n_sigs: u8) -> u64 {
        self.failed_attempt_cost_at(None, bid, n_sigs)
    }

    /// [`HotCfg::failed_attempt_cost`] with the create slot leader's class
    /// from the schedule (`leader_class`, when set, overrides it).
    pub fn failed_attempt_cost_at(&self, class: Option<LeaderClass>, bid: u64, n_sigs: u8) -> u64 {
        if self.failed_attempt_lamports > 0 {
            return self.failed_attempt_lamports;
        }
        match self.leader_class.or(class) {
            Some(c) => self.bid_policy.plan(c, bid, n_sigs).failed_cost(n_sigs),
            None => self
                .bid_policy
                .expected_failed_cost(&self.leader_mix, bid, n_sigs),
        }
    }

    fn frozen(&self, slot: u64) -> bool {
        self.freeze.iter().any(|[a, b]| (*a..=*b).contains(&slot))
    }
}

/// `V_pb * sqrt(1 + bps / 10,000)` in integers (floor): the virtual SOL
/// reserve at which the price (`V^2 / k` on the constant-product curve) is
/// `bps` above the price at `V_pb`. The real buy's `min_tokens_out` is the
/// quote at this reserve, so a fill that pays more than the premium over
/// the expected post-bundle price fails on chain.
pub fn entry_v_cap(v_pb: u64, bps: u32) -> u64 {
    let Some(sq) = (v_pb as u128 * v_pb as u128).checked_mul(10_000 + bps as u128) else {
        return u64::MAX;
    };
    u64::try_from((sq / 10_000).isqrt()).unwrap_or(u64::MAX)
}

#[derive(Debug, Clone, Copy)]
struct Attempt {
    slot: u64,
    order: SellOrder,
    /// Signature of the paper transaction (zero for PumpSwap paper sells).
    sig: [u8; 64],
}

/// A paper (or real) position plus what the hot loop needs around it.
#[derive(Debug, Clone, Copy)]
struct Slot {
    pos: Position,
    intent: u64,
    op_slot: usize,
    create_slot: u64,
    spend: u64,
    v0: u64,
    t0: u64,
    /// The real buy's on-chain bound (`min_tokens_out` at `V_cap`): sent in
    /// probe/live, applied to E1 paper fills.
    min_out: u64,
    /// The operator's wide bound (`min_tokens_out` at `v_max`): the E2
    /// paper model (end of slot+1, not a creation-slot entry) keeps it.
    min_out_e2: u64,
    nonce_idx: u16,
    keys: MintKeys,
    sig0: [u8; 64],
    filled: bool,
    /// `Decision::Fire` in probe/live. A decision forced to paper in a real
    /// mode (freeze, ArmSet gate, op not armed) is a paper position: its sells
    /// go to the paper recorder and its P&L never touches the RiskGate.
    real: bool,
    deadline: u64,
    buf: [Option<Trade>; TRADE_BUF],
    buf_len: usize,
    attempts: [Option<Attempt>; MAX_ATTEMPTS],
    /// Filled at E1: slot clock, counterfactual reserves.
    e1: bool,
    /// E1 tape replay in progress (stream trades are buffered meanwhile).
    replaying: bool,
    /// Last slot the replay reached.
    replay_slot: u64,
    /// The entry filter's features (with the signer's and the best bundle
    /// wallet's LEAD-outcome counts), from the cold lane.
    feat: Option<(Feat, u32, u32)>,
    /// Real positions: signatures of our own trades already applied (the
    /// feed and the real fill lane can both report one).
    own_sigs: [[u8; 64]; OWN_SIGS],
    own_n: usize,
}

/// Own-trade signatures remembered per position.
const OWN_SIGS: usize = 8;

/// `entry.require_bundle`: parked creates at most (one create slot's worth
/// at the highest launch rate seen; a full table drops the oldest).
pub const PARK_SLOTS: usize = 64;
/// A parked create older than this is skipped even without a slot event.
pub const PARK_MAX_NS: u64 = 1_000_000_000;

/// A create held by the bundle gate: everything `decide_from_watch` needs,
/// `Copy` (no heap on the create path).
#[derive(Debug, Clone, Copy)]
pub struct Parked {
    pub hit: CreateHit,
    pub src: FeedSrc,
    pub recv_ns: u64,
    pub slot: u64,
    pub bank_id: u64,
    pub t0: u64,
    pub intent: u64,
    pub rates: (Option<u32>, Option<u32>),
    pub parked_ns: u64,
    /// The create's transaction index in its block (`u32::MAX` unknown).
    pub create_index: u32,
    /// The bundle run so far: post-trade reserves `(v, t)` of the
    /// highest-index trade of the mint in the create slot (the state we
    /// buy on), that index (0: none yet), when the first bundle buy was
    /// seen and how many qualified ([`bundle_buy_triggers`]).
    pub pb: Option<(u64, u64)>,
    pub pb_index: u32,
    pub bundle_ns: u64,
    pub bundle_buys: u8,
    /// The qualifying bundle buyers (distinct, at most one per run index):
    /// they join the position's bundle set so the LEAD exit's insider-sell
    /// rule sees their sells (the sim's bundle wallets).
    pub wallets: [Pubkey; RUN_MAX_AFTER as usize],
    pub n_wallets: u8,
    /// A trade of the mint beyond the run was delivered before any bundle
    /// buy (out-of-order stream): a bundle buy that arrives later cannot
    /// make us first any more.
    pub behind: bool,
}

/// The number of trades the hot loop keeps for mints that have no park
/// yet: the create's wire record and its slot's trades travel on two rings,
/// so a bundle buy can be polled one iteration before its create
/// ([`HotLoop::park`] replays them).
pub const PREPARK: usize = 16;

/// What a trade of a parked mint did to its park ([`park_trade`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParkStep {
    /// Another slot, no block order, or at / before the create: nothing.
    Ignore,
    /// Inside the run: recorded (the post-run state; a bundle buy counted),
    /// still waiting for the run to complete. Also a trade beyond the run
    /// on a park with no bundle buy yet (the park only remembers it).
    Hold,
    /// The run holds a bundle buy and cannot grow (a trade at its last
    /// possible index) or no wait is configured: fire now.
    Fire,
    /// A trade of the mint beyond the run executed before we fired: the
    /// first buy behind the bundle is someone else's.
    NotFirst,
    /// Unused by [`park_trade`] since the out-of-order fix: an unarmed park
    /// waits for the slot to end (kept for the hot loop's slot-end skip).
    NoBundle,
}

/// A trade of the parked mint. The post-run state is the highest-index
/// trade's reserves (buys and sells alike: it is the curve we buy on); a
/// qualifying bundle buy arms the park. The park fires here when the run
/// cannot grow (a trade at `create + RUN_MAX_AFTER`) or, with `wait_ns ==
/// 0`, on the first bundle buy; otherwise the hot loop fires it on the
/// stream's run-complete signal ([`RunDoneLite`]) or when [`park_due`].
pub fn park_trade(
    p: &mut Parked,
    t: &TradeLite,
    owner: &Pubkey,
    wait_ns: u64,
    now_ns: u64,
) -> ParkStep {
    if t.mint != p.hit.mint
        || t.slot != p.slot
        || p.create_index == u32::MAX
        || t.index == u32::MAX
        || t.index <= p.create_index
    {
        return ParkStep::Ignore;
    }
    let last = p.create_index.saturating_add(RUN_MAX_AFTER);
    if t.index > last {
        if p.bundle_buys > 0 {
            return ParkStep::NotFirst;
        }
        // no bundle buy yet: the stream may be out of order (the bundle's
        // own trades still to come), so the park stays until the slot ends
        p.behind = true;
        return ParkStep::Hold;
    }
    if t.index >= p.pb_index {
        p.pb_index = t.index;
        p.pb = Some((t.virtual_sol_reserves, t.virtual_token_reserves));
    }
    if bundle_buy_triggers(&p.hit, p.slot, p.create_index, t, owner) {
        if p.behind {
            return ParkStep::NotFirst;
        }
        if p.bundle_buys == 0 {
            p.bundle_ns = now_ns;
        }
        p.bundle_buys = p.bundle_buys.saturating_add(1);
        let n = p.n_wallets as usize;
        if !p.wallets[..n].contains(&t.user) && n < p.wallets.len() {
            p.wallets[n] = t.user;
            p.n_wallets += 1;
        }
    }
    if p.bundle_buys > 0 && (t.index == last || wait_ns == 0) {
        ParkStep::Fire
    } else {
        ParkStep::Hold
    }
}

/// The wait after the first bundle buy is over.
pub fn park_due(p: &Parked, wait_ns: u64, now_ns: u64) -> bool {
    p.bundle_buys > 0 && now_ns.saturating_sub(p.bundle_ns) >= wait_ns
}

/// Whether `t` is the bundle buy a parked create waits for: a buy of the
/// mint in the create slot, within [`RUN_MAX_AFTER`] transactions after the
/// create in block order (the dev's bundle, the in-class launch:
/// `feed_bundle.bundle_wallets`), by a wallet that is neither ours nor the
/// creator's (the create's signer or `creator` argument). A buy further
/// down the block is another sniper, not the bundle: buying behind one is
/// the entry the LEAD policy was not scored on. Unknown indices never
/// trigger.
pub fn bundle_buy_triggers(
    hit: &CreateHit,
    create_slot: u64,
    create_index: u32,
    t: &TradeLite,
    owner: &Pubkey,
) -> bool {
    t.mint == hit.mint
        && t.is_buy
        && t.slot == create_slot
        && create_index != u32::MAX
        && t.index != u32::MAX
        && t.index > create_index
        && t.index <= create_index.saturating_add(RUN_MAX_AFTER)
        && t.user != *owner
        && t.user != hit.user
        && t.user != hit.creator_arg
}

/// The first 8 base58 characters of a key, allocation-free (journal lines
/// on the hot thread).
fn key8(k: &Pubkey) -> [u8; 8] {
    let mut buf = [0u8; 48];
    let n = bs58::encode(k.as_ref()).onto(&mut buf[..]).unwrap_or(0);
    let mut out = [b'?'; 8];
    let n = n.min(8);
    out[..n].copy_from_slice(&buf[..n]);
    out
}

/// A journal line from the hot thread: formatted on the stack and written to
/// stderr in one call (`eprintln!` goes through the test harness's output
/// capture, which allocates). Truncated at 255 bytes.
struct LineBuf {
    buf: [u8; 256],
    len: usize,
}

impl core::fmt::Write for LineBuf {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let n = s.len().min(self.buf.len() - 1 - self.len);
        self.buf[self.len..self.len + n].copy_from_slice(&s.as_bytes()[..n]);
        self.len += n;
        Ok(())
    }
}

fn journal(args: core::fmt::Arguments<'_>) {
    use std::io::Write as _;
    let mut b = LineBuf {
        buf: [0u8; 256],
        len: 0,
    };
    let _ = core::fmt::write(&mut b, args);
    b.buf[b.len] = b'\n';
    let _ = std::io::stderr().write_all(&b.buf[..b.len + 1]);
}

/// Plain counters for reports (the metrics hold the same numbers).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct LoopCounters {
    pub events: u64,
    pub creates: u64,
    pub decisions: u64,
    pub intents: u64,
    pub fills: u64,
    pub unfilled: u64,
    pub sells: u64,
    pub sell_fills: u64,
    pub closed: u64,
    pub trades: u64,
    pub ring_full: u64,
    /// `entry.require_bundle`: creates parked for a bundle buy, and the
    /// parked creates that fired on one.
    pub parked: u64,
    pub bundle_fired: u64,
    /// Bundle gate: run-complete signals from the stream, creates skipped
    /// because a trade beyond the run executed before we fired, and the
    /// wait from the first bundle buy to the fire summed in microseconds
    /// (mean: `bundle_wait_us / bundle_fired`).
    pub run_done: u64,
    pub not_first: u64,
    pub bundle_wait_us: u64,
    /// Real: zero-balance reports not trusted (node view not past the fill,
    /// or a missing account inside the grace window), `balance_is_trustworthy`.
    pub balance_ignored: u64,
    /// E1 verdicts other than a fill (premium, slippage, not in slot 0, class, unresolved).
    pub e1_failed: u64,
    pub e1_replayed_trades: u64,
    /// Intents the entry filter dropped (`entry.filter_mode = "skip"`).
    pub filtered: u64,
    /// Real (probe): buys that landed and failed / were dropped, own trades
    /// applied, real sells skipped for want of a fresh blockhash.
    pub real_failed: u64,
    pub real_dropped: u64,
    pub own_trades: u64,
    pub sells_no_blockhash: u64,
}

/// Everything [`HotLoop::new`] needs.
pub struct HotParts {
    pub cfg: HotCfg,
    pub watch: Arc<Watchlist>,
    pub arm: Arc<ArcSwap<ArmState>>,
    pub risk: Arc<RiskGate>,
    pub fired: Arc<FiredSet>,
    pub nonces: Arc<NoncePool>,
    pub signer: Arc<HotSigner>,
    pub templates: Templates,
    pub senders: Arc<SenderSet>,
    pub rslog: RslogProducer,
    pub cold: rtrb::Producer<ColdMsg>,
    pub fills_in: rtrb::Consumer<HotIn>,
    pub fire: rtrb::Producer<FireJob>,
    pub stats: Arc<HotStats>,
    pub shared: Arc<Shared>,
    /// The leader schedule (cold lane publishes; empty = unknown).
    pub leaders: LeaderSnapshot,
}

/// The hot loop.
pub struct HotLoop {
    cfg: HotCfg,
    watch: Arc<Watchlist>,
    arm: Arc<ArcSwap<ArmState>>,
    risk: Arc<RiskGate>,
    fired: Arc<FiredSet>,
    nonces: Arc<NoncePool>,
    signer: Arc<HotSigner>,
    owner: Pubkey,
    tmpl: Templates,
    senders: Arc<SenderSet>,
    rslog: RslogProducer,
    cold: rtrb::Producer<ColdMsg>,
    fills_in: rtrb::Consumer<HotIn>,
    fire: rtrb::Producer<FireJob>,
    stats: Arc<HotStats>,
    shared: Arc<Shared>,
    slots: Box<[Option<Slot>]>,
    acts: Vec<ExitAction>,
    slot_now: u64,
    last_tick_ns: u64,
    last_gauge_ns: u64,
    /// on_event start -> primary handed to the sender (decisions that sent).
    hist_decision: Histogram<u64>,
    /// recv_ns (feed thread) -> primary handed to the sender.
    hist_wire: Histogram<u64>,
    /// Per-event processing time (all events).
    hist_event: Histogram<u64>,
    pub counters: LoopCounters,
    /// Creates per 60 / 600 s (entry filter feature).
    rates: CreateRate,
    /// `entry.require_bundle`: creates waiting for a bundle buy (fixed
    /// table: no allocation on the create path).
    parked: [Option<Parked>; PARK_SLOTS],
    /// Recent trades of mints with no park and no position ([`PREPARK`]).
    prepark: [Option<TradeLite>; PREPARK],
    prepark_at: usize,
    leaders: LeaderSnapshot,
    /// Scratch for the ALT-loaded addresses of a wire event (no allocation).
    loaded: Box<[Pubkey]>,
    last_mark_ns: u64,
}

impl core::fmt::Debug for HotLoop {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HotLoop")
            .field("mode", &self.cfg.mode)
            .field("owner", &self.owner.to_string())
            .field("counters", &self.counters)
            .finish_non_exhaustive()
    }
}

fn hist() -> Histogram<u64> {
    Histogram::new_with_bounds(1, 60_000_000_000, 3).expect("histogram bounds")
}

#[inline]
fn src_class(src: FeedSrc) -> u8 {
    match src {
        FeedSrc::Grpc | FeedSrc::GrpcB | FeedSrc::Shreds => LIVE_CLASS,
        _ => FREE_CLASS,
    }
}

impl HotLoop {
    pub fn new(p: HotParts) -> Self {
        let owner = p.signer.pubkey();
        Self {
            cfg: p.cfg,
            watch: p.watch,
            arm: p.arm,
            risk: p.risk,
            fired: p.fired,
            nonces: p.nonces,
            signer: p.signer,
            owner,
            tmpl: p.templates,
            senders: p.senders,
            rslog: p.rslog,
            cold: p.cold,
            fills_in: p.fills_in,
            fire: p.fire,
            stats: p.stats,
            shared: p.shared,
            slots: (0..MAX_POS).map(|_| None).collect(),
            acts: Vec::with_capacity(64),
            slot_now: 0,
            last_tick_ns: 0,
            last_gauge_ns: 0,
            hist_decision: hist(),
            hist_wire: hist(),
            hist_event: hist(),
            counters: LoopCounters::default(),
            rates: CreateRate::new(crate::now_ms()),
            parked: [None; PARK_SLOTS],
            prepark: [None; PREPARK],
            prepark_at: 0,
            leaders: p.leaders,
            loaded: vec![Pubkey::default(); MAX_LOADED].into_boxed_slice(),
            last_mark_ns: 0,
        }
    }

    /// Classes of the leaders of `slot` and `slot + 1` (schedule lookup:
    /// lock-free, no allocation).
    #[inline]
    fn leader_classes(&self, slot: u64) -> (Option<ClientClass>, Option<ClientClass>) {
        let m = self.leaders.load();
        (m.class_at(slot), m.class_at(slot + 1))
    }

    /// The paper wallet (template fee payer / position owner).
    pub fn owner(&self) -> Pubkey {
        self.owner
    }

    /// `(decision, wire, event)` latency histograms (ns).
    pub fn histograms(&self) -> (&Histogram<u64>, &Histogram<u64>, &Histogram<u64>) {
        (&self.hist_decision, &self.hist_wire, &self.hist_event)
    }

    pub fn reset_histograms(&mut self) {
        self.hist_decision.reset();
        self.hist_wire.reset();
        self.hist_event.reset();
    }

    /// Positions held or pending.
    pub fn positions(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// rslog records dropped on a full ring.
    pub fn rslog_dropped(&self) -> u64 {
        self.rslog.dropped()
    }

    /// One iteration: poll, process every event, drain the cold lane, tick.
    pub fn step<F: Feed + ?Sized>(&mut self, feed: &mut F, buf: &mut FeedBuf) -> FeedStatus {
        buf.clear();
        let st = feed.poll(buf);
        for i in 0..buf.len() {
            let ev = buf.events()[i];
            let t0 = mono_ns();
            self.on_event(&ev, buf, t0);
            self.hist_event
                .saturating_record(mono_ns().saturating_sub(t0).max(1));
        }
        while let Ok(m) = self.fills_in.pop() {
            self.on_hot_in(m);
        }
        // The feed's slot updates keep the clock moving when no event does:
        // the exit engine evaluates at slot close, and a held mint that goes
        // quiet used to wait for the next event anywhere (a create, or the
        // dump on the mint itself) before its hold timers were looked at
        // (2026-10-22: sells triggered 3-17 slots late at night).
        let before = self.slot_now;
        self.advance_slot(feed.slot());
        let now = mono_ns();
        if self.cfg.require_bundle {
            self.fire_due_parked(now);
        }
        // a slot close is looked at as soon as it is known, not at the next tick
        if self.slot_now > before
            || now.saturating_sub(self.last_tick_ns) >= self.cfg.tick_ms * 1_000_000
        {
            self.last_tick_ns = now;
            self.tick(now);
        }
        self.shared.heartbeat.fetch_add(1, Relaxed);
        st
    }

    /// The slot clock moves forward (an event's slot, or the feed's slot
    /// updates between events).
    fn advance_slot(&mut self, slot: u64) {
        if slot > self.slot_now {
            self.slot_now = slot;
            self.shared.slot_now.store(slot, Relaxed);
            if self.cfg.require_bundle {
                self.expire_parked();
            }
        }
    }

    /// Process one feed event.
    pub fn on_event(&mut self, ev: &FeedEvent, buf: &FeedBuf, t0: u64) {
        self.counters.events += 1;
        self.stats.feed_events[ev.src as usize].inc();
        self.advance_slot(ev.slot);
        match ev.payload {
            Payload::Hit(lite) => {
                let hit = hit_from_lite(&lite);
                self.on_create(
                    ev.src,
                    ev.recv_ns,
                    ev.slot.max(lite.slot),
                    ev.bank_id,
                    &hit,
                    t0,
                    u32::MAX,
                );
            }
            Payload::Wire(w) => {
                // raw transaction + (gRPC) the post-execution trailer: the
                // lookup tables' addresses and the inner instructions resolve
                // v0 creates and launcher CPIs; all on the stack / scratch
                let meta = WireMeta::parse(buf.wire_meta(w));
                if meta.is_some_and(|m| m.failed()) {
                    return; // a failed transaction created nothing
                }
                let res = match pump_codec::tx::TxView::parse(buf.wire_tx(w)) {
                    Ok(tx) => {
                        let mut inner = [EMPTY_INNER; MAX_INNER];
                        let (nl, ni) = match &meta {
                            Some(m) => (m.loaded_into(&mut self.loaded), m.inner_into(&mut inner)),
                            None => (0, 0),
                        };
                        let loaded = (nl > 0).then(|| &self.loaded[..nl]);
                        pump_codec::create_hit(&tx, loaded, &inner[..ni], self.cfg.fees)
                            .map_err(|e| MissKind::of_hit(&e))
                    }
                    Err(e) => Err(MissKind::of_decode(&e)),
                };
                match res {
                    Ok(Some(mut hit)) => {
                        hit.slot = Some(ev.slot);
                        let create_index = meta.and_then(|m| m.tx_index).unwrap_or(u32::MAX);
                        self.on_create(
                            ev.src,
                            ev.recv_ns,
                            ev.slot,
                            ev.bank_id,
                            &hit,
                            t0,
                            create_index,
                        );
                    }
                    Ok(None) => {}
                    Err(kind) => {
                        // counted by kind (a match and two atomic adds: the
                        // error is never formatted here); the adapter's line
                        // for the same signature says which shape it was
                        self.stats.wire_misses.inc();
                        self.stats.wire_miss_kind[kind as usize].inc();
                    }
                }
            }
            Payload::Trade(t) if t.user == self.owner && self.cfg.mode.is_real() => {
                // our own TradeEvent from the feed: opens / sizes a real position
                self.on_own_trade(t.sig.0, trade_from_lite(&t), false);
            }
            Payload::Trade(t) => {
                if !(self.cfg.require_bundle && self.parked_trade(&t)) {
                    if self.cfg.require_bundle
                        && t.index != u32::MAX
                        && self.find_mint(&t.mint).is_none()
                    {
                        // a bundle buy polled before its create's wire
                        // record: replayed when the create is parked
                        self.prepark[self.prepark_at % PREPARK] = Some(t);
                        self.prepark_at = self.prepark_at.wrapping_add(1);
                    }
                    self.on_trade(trade_from_lite(&t));
                }
            }
            Payload::RunDone(r) => {
                if self.cfg.require_bundle {
                    self.parked_run_done(&r);
                }
            }
            Payload::Funding(f) => {
                self.stats.funding_events.inc();
                self.send_cold(ColdMsg::Funding(f));
            }
            Payload::Dup(_) => {
                self.stats.dups.inc();
            }
        }
    }

    #[inline]
    fn send_cold(&mut self, m: ColdMsg) {
        if self.cold.push(m).is_err() {
            self.counters.ring_full += 1;
            self.stats.ring_full.inc();
        }
    }

    fn decision(
        &mut self,
        intent: u64,
        hit: &CreateHit,
        code: u8,
        would_skip: u8,
        size: u64,
        v_max: u64,
        min_tokens: u64,
        nonce_idx: u16,
    ) {
        self.counters.decisions += 1;
        self.stats
            .decision(if code & 0xf0 == 0x10 && would_skip != 0 {
                code::PAPER_WOULD_SKIP
            } else {
                code
            });
        self.rslog.push(Rec::Decision {
            intent,
            mint: hit.mint,
            mode: self.cfg.mode.as_u8(),
            code,
            would_skip,
            size,
            v_max,
            min_tokens,
            nonce_idx,
            decided_ns: mono_ns(),
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn on_create(
        &mut self,
        src: FeedSrc,
        recv_ns: u64,
        slot: u64,
        bank_id: u64,
        hit: &CreateHit,
        t0: u64,
        create_index: u32,
    ) {
        self.counters.creates += 1;
        self.stats.creates[src as usize].inc();
        let s = src as u8;
        self.rslog.push(Rec::FeedSeen {
            src: s,
            slot,
            recv_ns,
            sig: hit.sig,
        });
        self.rslog
            .push(Rec::CreateHit(CreateHitRec::from_hit(s, hit)));
        // the first-seen benchmark: every feed's copy, before first-wins
        self.send_cold(ColdMsg::FeedSeen {
            src,
            sig_hash: xxhash_rust::xxh3::xxh3_64(&hit.sig),
            recv_ns,
            slot,
        });
        let intent = IntentId::new(&hit.mint, &self.owner).0;
        // 2: first-wins per mint and feed class
        if let Fired::Already { .. } = self.fired.first(&hit.mint, src_class(src), src) {
            return self.decision(intent, hit, code::ALREADY_FIRED, 0, 0, 0, 0, 0);
        }
        if self.cfg.post_exec_feed && self.cfg.mode.is_real() && src_class(src) == FREE_CLASS {
            return self.decision(intent, hit, code::FREE_FEED_DEFERS, 0, 0, 0, 0, 0);
        }
        // across classes: a mint already held or pending from the other
        // class's feed (the gRPC copy of a create the logs path saw first, or
        // the reverse) is one launch, one intent
        if self.find_mint(&hit.mint).is_some() {
            return self.decision(intent, hit, code::ALREADY_FIRED, 0, 0, 0, 0, 0);
        }
        // creates of the last 60 / 600 s, before this one (entry filter)
        let rates = self.rates.on_create(crate::now_ms());
        // 4: class filters
        if !hit.is_sol_quote()
            || hit.mayhem
            || hit.token_prog != TOKEN_2022_PROGRAM_ID
            || hit.cashback
        {
            return self.decision(intent, hit, code::CLASS, 0, 0, 0, 0, 0);
        }
        // 4b: the bundle gate (real only): park until another wallet's buy
        // of the mint shows in the create slot and the bundle run is
        // complete, then fire at its post-trade state; the slot moving on
        // without a bundle buy skips the launch (docs/04 section 2: the
        // LEAD policy's population is the bundled launch, entered right
        // behind the whole bundle, bound at that price + max_premium)
        if self.cfg.require_bundle && self.cfg.mode.is_real() && src_class(src) != FREE_CLASS {
            self.park(Parked {
                hit: *hit,
                src,
                recv_ns,
                slot,
                bank_id,
                t0,
                intent,
                rates,
                parked_ns: mono_ns(),
                create_index,
                pb: None,
                pb_index: 0,
                bundle_ns: 0,
                bundle_buys: 0,
                wallets: [Pubkey::default(); RUN_MAX_AFTER as usize],
                n_wallets: 0,
                behind: false,
            });
            return;
        }
        self.decide_from_watch(src, recv_ns, slot, bank_id, hit, t0, intent, rates, None);
    }

    /// Park a create for the bundle gate; a full table drops its oldest
    /// entry as `skip_no_bundle`. Trades of the mint polled before the
    /// create's wire record ([`PREPARK`]) are replayed into the new park.
    fn park(&mut self, p: Parked) {
        self.counters.parked += 1;
        let i = match self.parked.iter().position(|x| x.is_none()) {
            Some(i) => i,
            None => {
                let oldest = (0..PARK_SLOTS)
                    .min_by_key(|&i| self.parked[i].map_or(u64::MAX, |x| x.parked_ns))
                    .unwrap_or(0);
                if let Some(old) = self.parked[oldest].take() {
                    self.decision(old.intent, &old.hit, code::NO_BUNDLE, 0, 0, 0, 0, 0);
                }
                oldest
            }
        };
        self.parked[i] = Some(p);
        let (mint, slot) = (p.hit.mint, p.slot);
        for k in 0..PREPARK {
            let Some(t) = self.prepark[k] else { continue };
            if t.mint != mint || t.slot != slot {
                continue;
            }
            self.prepark[k] = None;
            if self.parked[i].is_none() || !self.parked_trade(&t) {
                break;
            }
        }
    }

    /// The slot moved on (or a second passed): a parked create of an
    /// earlier slot with no bundle buy is an unbundled launch, skipped; one
    /// with a bundle buy had its run end with the slot: a real buy after
    /// the create slot is refused (step 8b), recorded as `late_slot` here
    /// without a decision (no risk token, no nonce).
    fn expire_parked(&mut self) {
        let now = mono_ns();
        for i in 0..PARK_SLOTS {
            let Some(p) = self.parked[i] else { continue };
            if p.slot < self.slot_now && p.bundle_buys > 0 {
                self.parked[i] = None;
                self.decision(p.intent, &p.hit, code::LATE_SLOT, 0, 0, 0, 0, 0);
            } else if p.slot < self.slot_now || now.saturating_sub(p.parked_ns) > PARK_MAX_NS {
                self.parked[i] = None;
                self.decision(p.intent, &p.hit, code::NO_BUNDLE, 0, 0, 0, 0, 0);
            }
        }
    }

    /// A trade on a parked mint ([`park_trade`]): the bundle run's state is
    /// kept, a complete run with a bundle buy fires the parked decision at
    /// it, a trade beyond the run before we fired skips the launch. `true`
    /// when the trade was for a parked mint (consumed either way: there is
    /// no position to size yet).
    fn parked_trade(&mut self, t: &TradeLite) -> bool {
        let Some(i) = self
            .parked
            .iter()
            .position(|x| x.is_some_and(|p| p.hit.mint == t.mint))
        else {
            return false;
        };
        let Some(mut p) = self.parked[i] else {
            return false;
        };
        let now = mono_ns();
        match park_trade(&mut p, t, &self.owner, self.cfg.bundle_wait_ns, now) {
            ParkStep::Ignore => {}
            ParkStep::Hold => self.parked[i] = Some(p),
            ParkStep::Fire => {
                self.parked[i] = None;
                self.fire_parked(&p, now);
            }
            ParkStep::NotFirst => {
                self.parked[i] = None;
                self.counters.not_first += 1;
                self.decision(p.intent, &p.hit, code::NOT_FIRST, 0, 0, 0, 0, 0);
            }
            ParkStep::NoBundle => {
                self.parked[i] = None;
                self.decision(p.intent, &p.hit, code::NO_BUNDLE, 0, 0, 0, 0, 0);
            }
        }
        true
    }

    /// The stream's run-complete signal (a pump transaction of the create
    /// slot beyond the run): a parked create with a bundle buy fires at the
    /// run's state. One without stays parked (the signal follows delivery
    /// order, the bundle's own trades may still come): the slot's end
    /// skips it.
    fn parked_run_done(&mut self, r: &RunDoneLite) {
        self.counters.run_done += 1;
        let Some(i) = self
            .parked
            .iter()
            .position(|x| x.is_some_and(|p| p.hit.mint == r.mint && p.slot == r.slot))
        else {
            return;
        };
        if self.parked[i].is_some_and(|p| p.bundle_buys == 0) {
            return;
        }
        if let Some(p) = self.parked[i].take() {
            self.fire_parked(&p, mono_ns());
        }
    }

    /// Parked creates whose wait after the first bundle buy is over
    /// (`entry.bundle_wait_ms`): fired at the run seen so far.
    fn fire_due_parked(&mut self, now: u64) {
        for i in 0..PARK_SLOTS {
            if self.parked[i]
                .as_ref()
                .is_some_and(|p| park_due(p, self.cfg.bundle_wait_ns, now))
            {
                if let Some(p) = self.parked[i].take() {
                    self.fire_parked(&p, now);
                }
            }
        }
    }

    /// Steps 5-11 for a parked create, bound at the bundle run's post-trade
    /// state.
    fn fire_parked(&mut self, p: &Parked, now: u64) {
        self.counters.bundle_fired += 1;
        self.counters.bundle_wait_us += now.saturating_sub(p.bundle_ns) / 1_000;
        self.decide_from_watch(
            p.src,
            p.recv_ns,
            p.slot,
            p.bank_id,
            &p.hit,
            p.t0,
            p.intent,
            p.rates,
            Some(p),
        );
    }

    /// Steps 5-11 of the decision: watchlist, risk, keys, nonce, size and
    /// the send (or the paper record). `parked`: the bundle gate's park,
    /// with the run's observed post-trade reserves (the state the buy is
    /// bound at; without it the bound falls back to `V0 +
    /// entry.bundle_lamports`) and the bundle wallets for the position.
    #[allow(clippy::too_many_arguments)]
    fn decide_from_watch(
        &mut self,
        src: FeedSrc,
        recv_ns: u64,
        slot: u64,
        bank_id: u64,
        hit: &CreateHit,
        t0: u64,
        intent: u64,
        rates: (Option<u32>, Option<u32>),
        parked: Option<&Parked>,
    ) {
        let pb = parked.and_then(|p| p.pb);
        // 5: watchlist
        let now_ms = crate::now_ms();
        let now_unix = now_ms / 1_000;
        let (entry, via) = match self.watch.match_hit(hit, now_unix) {
            Match::Live { entry, via } => (entry, via),
            Match::Shadow { entry } => {
                self.rslog.push(Rec::Matched {
                    intent,
                    op_id: entry.op_id,
                    armset_ver: self.watch.armset_ver(),
                    via: 0,
                    shadow: 1,
                });
                return self.decision(intent, hit, code::SHADOW, 0, 0, 0, 0, 0);
            }
            Match::None => return self.decision(intent, hit, code::NO_MATCH, 0, 0, 0, 0, 0),
        };
        let arm = self.arm.load();
        let Some(op) = arm.ops.get(&entry.op_id).copied() else {
            return self.decision(intent, hit, code::BAD_OP, 0, 0, 0, 0, 0);
        };
        let via_code = match (via, entry.dynamic) {
            (Via::Class, _) => 4,
            (_, true) => 3,
            (Via::Signer, false) => 1,
            (Via::FeePayer, false) => 2,
        };
        self.rslog.push(Rec::Matched {
            intent,
            op_id: op.op_id,
            armset_ver: arm.ver,
            via: via_code,
            shadow: 0,
        });
        // 6: age
        if self.slot_now.saturating_sub(slot) > self.cfg.max_age_slots {
            return self.decision(intent, hit, code::STALE, 0, op.size, op.v_max, 0, 0);
        }
        // 7: risk
        let (v0, t0_tok) = v0_of(hit);
        // the reserve we buy at: the bundle run's post-trade state when the
        // gate observed it (the create's state otherwise; the bound below
        // then adds the expected bundle)
        let v_entry = pb.map_or(v0, |(v, _)| v);
        let mut bid = self.cfg.bid_lamports.min(self.cfg.max_bid);
        if op.ev_cap > 0 {
            bid = bid.min(op.ev_cap);
        }
        // Free feeds (logs path, collector relay) know no fee payer and no
        // token owners and are slot+1 at best: paper only, in every mode
        // (docs/06 section 7.1 "Free mode"). Real buys need a live-class feed.
        let mode = if src_class(src) == FREE_CLASS {
            Mode::Paper
        } else {
            self.cfg.mode
        };
        let d = self.risk.decide(&DecideInput {
            mode,
            op_slot: op.slot,
            op_armed: op_armed(mode, op.state),
            // probe needs a signed probe_allowed ArmSet, live a signed gate_passed one
            armset_ok: arm.verified
                && match mode {
                    Mode::Live => arm.gate_passed,
                    Mode::Probe => {
                        arm.probe_allowed && self.cfg.probe_stage.admitted_by(arm.probe_stage)
                    }
                    Mode::Paper => true,
                },
            frozen: self.cfg.frozen(slot),
            spend: op.size,
            bid,
            v_entry,
            now_unix,
        });
        let dcode = d.code();
        let would = match d {
            Decision::Skip(_) => {
                return self.decision(intent, hit, dcode, 0, op.size, op.v_max, 0, 0);
            }
            Decision::Paper {
                would_skip: Some(r),
                ..
            } => {
                return self.decision(intent, hit, dcode, r as u8, op.size, op.v_max, 0, 0);
            }
            Decision::Paper { .. } | Decision::Fire => 0u8,
        };
        let real = d.is_fire();
        // 8: entry bound
        if v_entry >= op.v_max {
            if real {
                self.risk.release_unfilled();
            }
            return self.decision(intent, hit, code::TOO_HIGH, would, op.size, op.v_max, 0, 0);
        }
        // 8b: fire only in the create slot (docs/04 section 2). A real
        // decision that runs after the lane has seen a later slot cannot
        // land behind the bundle any more: refused, never sent. (Paper keeps
        // scoring: a free feed is slot+1 by construction and E1 models the
        // creation slot from history.)
        if real && self.slot_now > slot {
            self.risk.release_unfilled();
            return self.decision(intent, hit, code::LATE_SLOT, 0, op.size, op.v_max, 0, 0);
        }
        // 9: randomized skip (probe/live only; A/B is off in paper)
        if real && self.cfg.ab_enabled && op.ab_skip_p > 0.0 {
            let mut b = [0u8; 40];
            b[..8].copy_from_slice(&self.cfg.ab_salt.to_le_bytes());
            b[8..].copy_from_slice(hit.mint.as_ref());
            let u = xxhash_rust::xxh3::xxh3_64(&b) as f64 / u64::MAX as f64;
            if u < op.ab_skip_p {
                self.risk.release_unfilled();
                return self.decision(intent, hit, code::SKIP_AB, 0, op.size, op.v_max, 0, 0);
            }
        }
        // a position slot must exist before any nonce is claimed
        let Some(free) = self.slots.iter().position(|s| s.is_none()) else {
            if real {
                self.risk.release_unfilled();
            }
            return self.decision(
                intent,
                hit,
                code::NO_POSITION_SLOT,
                would,
                op.size,
                op.v_max,
                0,
                0,
            );
        };
        // keys and the per-mint compute-unit limit (gap-5: 65,000 + 1,500*S
        // over the four PDA bumps plus the wallet's UVA term, clamped to
        // [tx.cu_limit floor, 1.4M]), before any nonce is claimed
        let abc = if hit.abc != Pubkey::default() {
            hit.abc
        } else {
            pda::associated_bonding_curve(&hit.mint, &hit.token_prog)
        };
        let aqbc = hit
            .aqbc_hint
            .unwrap_or_else(|| pda::ata_spl(&hit.bc, &WSOL_MINT));
        let user_ata = pda::ata_token_2022(&self.owner, &hit.mint);
        let keys = MintKeys::curve(hit.mint, hit.bc, abc, aqbc, user_ata);
        let Some(cu_limit) = keys.snipe_cu_limit(
            &self.owner,
            &TOKEN_2022_PROGRAM_ID,
            self.tmpl.cu_wallet_missing,
            self.tmpl.cu_floor,
        ) else {
            // a key is not the canonical PDA of its seeds: the buy would fail
            if real {
                self.risk.release_unfilled();
            }
            return self.decision(intent, hit, code::BAD_KEYS, would, op.size, op.v_max, 0, 0);
        };
        // 10: nonce (no probing; value older than the create slot)
        let claim = match self
            .nonces
            .try_claim(&hit.mint, slot, intent, self.slot_now)
        {
            Ok(c) => c,
            Err(_) => {
                if real {
                    self.risk.release_unfilled();
                }
                return self.decision(intent, hit, code::NO_NONCE, would, op.size, op.v_max, 0, 0);
            }
        };
        // 11: size and the on-chain bound at V_cap = min(v_max, V_pb *
        // sqrt(1 + max_premium)), V_pb the bundle run's observed post-trade
        // reserve (else V0 + the expected bundle outside the create tx): a
        // fill above the post-bundle price + max_premium fails on chain
        // (cheaply), the same bound paper E1 applies
        let spend = op.size;
        let v_cap = match pb {
            Some((v, _)) => self.cfg.v_cap_at(v, op.v_max),
            None => self.cfg.v_cap(v0, op.v_max),
        };
        let min_out = min_tokens_out(spend, v_cap, hit.creator_arg, self.cfg.fees);
        let min_out_e2 = min_tokens_out(spend, op.v_max, hit.creator_arg, self.cfg.fees);
        // 12: patch (keys and the per-mint CU limit were resolved before the claim)
        // per-class bid: the tip (after the buy, reverts on failure) carries
        // the bid where the leader orders by it; the priority fee stays at the
        // floor there and is clamped by bid.failed_attempt_lamports_max elsewhere
        let n_sigs = self.tmpl.buys[claim.idx].table.n_sigs.clamp(1, 2) as u8;
        // the leaders of the create slot and the next one (schedule)
        let (lc0, lc1) = self.leader_classes(slot);
        let leader0 = lc0.map_or(LEADER_UNKNOWN, |c| c as u8);
        let leader1 = lc1.or(lc0).map_or(LEADER_UNKNOWN, |c| c as u8);
        let (plan0, plan1) = self.cfg.bid_plans_at(
            lc0.map(ClientClass::bid_class),
            lc1.map(ClientClass::bid_class),
            bid,
            n_sigs,
        );
        let (prio0, tip0) = (plan0.priority_lamports, plan0.tip_lamports);
        let tip_account = self.tmpl.tip_account;
        let t = &mut self.tmpl.buys[claim.idx];
        t.patch(&Patch {
            keys: &keys,
            lifetime: &claim.value,
            amount: spend,
            limit: min_out,
            priority_lamports: prio0,
            tip_lamports: tip0,
            cu_limit: Some(cu_limit),
        });
        // 13: sign and hand the primary variant over
        t.sign(&self.signer, None);
        let v = Variant::from_template(
            t,
            intent,
            0,
            VariantKind::Buy,
            prio0,
            tip0,
            Some(tip_account),
        );
        let (sender, result) = if real {
            let job = FireJob {
                tmpl: t.clone(),
                intent,
                idx: 0,
                prio: prio0,
                tip: tip0,
                tip_account,
                bid,
                real: true,
                leader: leader0,
            };
            if self.fire.push(job).is_err() {
                self.counters.ring_full += 1;
            }
            (SenderId::Paper as u8, 0u8)
        } else {
            let r = self.senders.paper.send(&v);
            self.stats.sends[SenderId::Paper as usize].inc();
            (SenderId::Paper as u8, r.code())
        };
        let sent_ns = mono_ns();
        self.hist_decision
            .saturating_record(sent_ns.saturating_sub(t0).max(1));
        self.hist_wire
            .saturating_record(sent_ns.saturating_sub(recv_ns).max(1));
        // the secondary variant bids for another class (unknown leader, or a
        // different leader next slot); the same plan twice would be the same
        // transaction (one signature), so it is not sent again
        if self.cfg.secondary_variant && plan1 != plan0 {
            let job = FireJob {
                tmpl: t.clone(),
                intent,
                idx: 1,
                prio: plan1.priority_lamports,
                tip: plan1.tip_lamports,
                tip_account,
                bid,
                // a paper decision's secondary variant is recorded, never sent
                real,
                leader: leader1,
            };
            if self.fire.push(job).is_err() {
                self.counters.ring_full += 1;
                self.stats.ring_full.inc();
            }
        }
        // 14: record
        let pcode = if real { 0 } else { dcode };
        self.decision(
            intent,
            hit,
            pcode,
            0,
            spend,
            op.v_max,
            min_out,
            claim.idx as u16,
        );
        self.counters.intents += 1;
        self.stats.intents.inc();
        if !real {
            self.rslog.push(Rec::Sent {
                intent,
                variant: 0,
                sender,
                prio: prio0,
                tip: tip0,
                sig: v.sig,
                result,
                latency_ns: sent_ns.saturating_sub(t0),
            });
            self.send_cold(ColdMsg::Sent {
                intent,
                sig: v.sig,
                variant: 0,
                sender,
                label: SenderId::Paper.name(),
                prio: prio0,
                tip: tip0,
                bid,
                sent_ns,
                sent_unix_ms: now_ms,
                result,
                err: "",
                http: 0,
                kind: VariantKind::Buy as u8,
                leader: leader0,
            });
        }
        self.send_cold(ColdMsg::Intent {
            intent,
            mint: hit.mint,
            op_id: op.op_id,
            armset_ver: arm.ver,
            slot,
            bank_id,
            via: via_code,
            src,
            spend,
            v_max: op.v_max,
            min_out,
            nonce_idx: claim.idx as u16,
            nonce_value: claim.value,
            decided_ns: sent_ns,
            real,
            leader: leader0,
        });
        self.send_cold(ColdMsg::WatchMint(hit.mint));
        if real {
            // the real fill lane: find our transaction, read our TradeEvent
            self.send_cold(ColdMsg::RealPending {
                intent,
                mint: hit.mint,
                bc: hit.bc,
                create_sig: hit.sig,
                slot,
                creator: hit.creator_arg,
                user: hit.user,
                nonce_idx: claim.idx as u16,
                nonce_value: claim.value,
                spend,
                bid,
            });
        }
        if !real {
            self.send_cold(ColdMsg::PaperPending {
                intent,
                mint: hit.mint,
                bc: hit.bc,
                create_sig: hit.sig,
                slot,
                v0,
                t0: t0_tok,
                decided_unix_ms: now_ms,
                creator: hit.creator_arg,
                user: hit.user,
                intx_buys: hit.intx_buys,
                rate60: crate::filter::rate_code(rates.0),
                rate600: crate::filter::rate_code(rates.1),
            });
            if self.cfg.mode.is_real() {
                // forced to paper in a real mode: nothing was sent with this
                // value, so the real nonce is released (no burn needed)
                self.nonces.release_unsent(&claim);
            }
        }
        // the position (opens on our own fill only)
        let mut bundle = BundleSet::new(hit.intx_buyers.as_slice());
        for k in hit.signers.as_slice() {
            if *k != hit.mint {
                bundle.insert(*k);
            }
        }
        bundle.insert(hit.creator_arg);
        if let Some(fp) = hit.fee_payer {
            bundle.insert(fp);
        }
        // the bundle's separate-transaction buyers (exit rule: their sells)
        if let Some(p) = parked {
            for w in &p.wallets[..p.n_wallets as usize] {
                bundle.insert(*w);
            }
        }
        let pos = Position::pending(
            hit.mint,
            self.owner,
            intent,
            self.cfg.policy,
            bundle,
            hit.creator_arg,
            self.cfg.fees,
            pump_codec::curve::TOKEN_TOTAL_SUPPLY,
        );
        self.slots[free] = Some(Slot {
            pos,
            intent,
            op_slot: op.slot,
            create_slot: slot,
            spend,
            v0,
            t0: t0_tok,
            min_out,
            min_out_e2,
            nonce_idx: claim.idx as u16,
            keys,
            sig0: v.sig,
            filled: false,
            real,
            deadline: slot + self.cfg.fill_timeout_slots,
            buf: [None; TRADE_BUF],
            buf_len: 0,
            attempts: [None; MAX_ATTEMPTS],
            e1: false,
            replaying: false,
            replay_slot: slot,
            feat: None,
            own_sigs: [[0u8; 64]; OWN_SIGS],
            own_n: 0,
        });
        self.stats.positions_open.set(self.positions() as i64);
    }

    fn find_mint(&self, mint: &Pubkey) -> Option<usize> {
        self.slots
            .iter()
            .position(|s| s.as_ref().is_some_and(|s| s.pos.mint == *mint))
    }

    fn find_intent(&self, intent: u64) -> Option<usize> {
        self.slots
            .iter()
            .position(|s| s.as_ref().is_some_and(|s| s.intent == intent))
    }

    fn on_trade(&mut self, t: Trade) {
        self.counters.trades += 1;
        self.stats.trades_seen.inc();
        let Some(i) = self.find_mint(&t.mint) else {
            self.stats.trades_unmatched.inc();
            return;
        };
        let s = self.slots[i].as_mut().expect("found");
        if !s.filled || s.replaying {
            if s.buf_len < TRADE_BUF {
                s.buf[s.buf_len] = Some(t);
                s.buf_len += 1;
            } else {
                self.stats.trade_buffer_drops.inc();
            }
            return;
        }
        if s.e1 {
            if t.slot <= s.replay_slot {
                return; // the replay already covered this slot
            }
            let tr = Self::counterfactual(s, t);
            let now = e1_clock(s.create_slot, self.cfg.slot_ms, t.slot);
            self.exit_event(i, ExitEvent::Trade(tr), now);
            return;
        }
        let now = Now {
            ms: crate::now_ms(),
            slot: t.slot,
        };
        self.exit_event(i, ExitEvent::Trade(t), now);
    }

    /// The slot clock of an E1 position.
    #[inline]
    fn e1_now(&self, s: &Slot, slot: u64) -> Now {
        e1_clock(s.create_slot, self.cfg.slot_ms, slot)
    }

    /// Someone else's trade on the curve with our tokens still outstanding:
    /// the history was made without us, our tokens stay out of the curve
    /// (`t' = t - ours`, `v' = v t / t'`; the tape simulator's model).
    #[inline]
    fn counterfactual(s: &Slot, t: Trade) -> Trade {
        let q = s.pos.tokens as u128;
        if !s.e1 || q == 0 || t.venue != Venue::Curve || t.user == s.pos.owner {
            return t;
        }
        let th = t.reserve_token as u128;
        if th <= q {
            return t;
        }
        let t2 = th - q;
        let v2 = (t.reserve_sol as u128) * th / t2;
        Trade {
            reserve_sol: u64::try_from(v2).unwrap_or(u64::MAX),
            reserve_token: t2 as u64,
            ..t
        }
    }

    fn on_hot_in(&mut self, m: HotIn) {
        match m {
            HotIn::PaperFill {
                intent,
                v,
                t,
                slot,
                buyers_ahead,
                rival_sol_ahead,
                ok,
                ..
            } => {
                let Some(i) = self.find_intent(intent) else {
                    return;
                };
                if !ok || self.slots[i].as_ref().is_some_and(|s| s.filled) {
                    return;
                }
                self.buy_fill(i, v, t, slot, false, buyers_ahead, rival_sol_ahead);
            }
            HotIn::E1Open {
                intent,
                outcome,
                info,
                ..
            } => {
                let Some(i) = self.find_intent(intent) else {
                    return;
                };
                if self.slots[i].as_ref().is_some_and(|s| s.filled) {
                    return;
                }
                self.e1_open(i, outcome, info);
            }
            HotIn::E1Insider {
                intent,
                key,
                creator,
            } => {
                let pol = self.cfg.policy.insider_sell;
                let Some(i) = self.find_intent(intent) else {
                    return;
                };
                if let Some(s) = self.slots[i].as_mut() {
                    if s.e1
                        && ((creator && pol.creator_set()) || (!creator && pol.bundle_wallets()))
                    {
                        s.pos.bundle.insert(key);
                    }
                }
            }
            HotIn::E1Feat {
                intent,
                feat,
                op_lead_n,
                bw_lead_n,
            } => {
                if let Some(i) = self.find_intent(intent) {
                    if let Some(s) = self.slots[i].as_mut() {
                        s.feat = Some((feat, op_lead_n, bw_lead_n));
                    }
                }
            }
            HotIn::E1Trade { intent, trade } => {
                if let Some(i) = self.find_intent(intent) {
                    self.replay_trade(i, trade);
                }
            }
            HotIn::E1End { intent, horizon } => {
                if let Some(i) = self.find_intent(intent) {
                    self.replay_end(i, horizon);
                }
            }
            HotIn::RealBuy { intent, sig, trade } => {
                if self.find_intent(intent).is_some() {
                    self.on_own_trade(sig, trade, true);
                }
            }
            HotIn::RealTrade { sig, trade } => self.on_own_trade(sig, trade, true),
            HotIn::RealFailed {
                intent,
                cost,
                dropped,
            } => self.real_failed(intent, cost, dropped),
            HotIn::RealBalance {
                mint,
                tokens,
                slot,
                exists,
            } => {
                let Some(i) = self.find_mint(&mint) else {
                    return;
                };
                // a pending intent opens on its own TradeEvent (the lane sends
                // it first); a balance only sizes or closes a held position
                if self.slots[i].as_ref().is_some_and(|s| s.real && s.filled) {
                    // a zero from a node view not past the fill (or a missing
                    // account in the first seconds) is "not visible yet", not
                    // "sold": closing on it leaves the tokens in the wallet
                    let (opened, exiting) = self.slots[i].as_ref().map_or((0, false), |s| {
                        (s.pos.opened_slot, s.pos.state == PositionState::Exiting)
                    });
                    if !crate::real::balance_is_trustworthy(exists, tokens, slot, opened, exiting) {
                        self.counters.balance_ignored += 1;
                        return;
                    }
                    // the hot loop's slot is the exit clock; the report's
                    // own slot (the RPC node's) only sized the trust check
                    let now = Now {
                        ms: crate::now_ms(),
                        slot: self.slot_now,
                    };
                    self.exit_event(i, ExitEvent::Balance { tokens, slot }, now);
                }
            }
        }
    }

    /// Our own trade on a held mint (real positions only): a buy opens the
    /// position (never a send: docs/06 section 8), a sell sizes or closes
    /// it. The feed and the real fill lane can both report one trade: each
    /// signature is applied once.
    fn on_own_trade(&mut self, sig: [u8; 64], t: Trade, from_lane: bool) {
        let Some(i) = self.find_mint(&t.mint) else {
            return;
        };
        let owner = self.owner;
        let Some(s) = self.slots[i].as_mut() else {
            return;
        };
        if !s.real || t.user != owner {
            return;
        }
        if s.own_sigs[..s.own_n.min(OWN_SIGS)].contains(&sig) {
            return;
        }
        s.own_sigs[s.own_n % OWN_SIGS] = sig;
        s.own_n += 1;
        self.counters.own_trades += 1;
        let now = Now {
            ms: crate::now_ms(),
            slot: t.slot.max(self.slot_now),
        };
        if t.is_buy && !s.filled {
            s.filled = true;
            let (intent, mint) = (s.intent, s.pos.mint);
            let (buf, n) = (s.buf, s.buf_len);
            s.buf_len = 0;
            self.counters.fills += 1;
            self.stats.fills.inc();
            // the fill's timing, for the journal (rare: one line per position)
            let k8 = key8(&mint);
            journal(format_args!(
                "fill {} slot {} slot_now {} via {} buffered {}",
                std::str::from_utf8(&k8).unwrap_or("?"),
                t.slot,
                self.slot_now,
                if from_lane { "lane" } else { "feed" },
                n
            ));
            self.exit_event(i, ExitEvent::Trade(t), now);
            self.send_cold(ColdMsg::RealHeld { intent, mint });
            for tr in buf[..n].iter().flatten() {
                if tr.slot > t.slot && self.slots[i].is_some() {
                    self.exit_event(i, ExitEvent::Trade(*tr), now);
                }
            }
            return;
        }
        if !t.is_buy {
            // the order this fill answers (same size first, else the oldest)
            let p = &s.pos;
            let order = p
                .pending
                .iter()
                .flatten()
                .find(|o| o.order.tokens == t.token_amount)
                .or_else(|| p.pending.iter().flatten().next())
                .map(|o| o.order);
            let (mint, owner) = (p.mint, p.owner);
            self.counters.sell_fills += 1;
            self.stats.sell_fills.inc();
            let reason = order.map_or(rugsnipe_exit::ExitReason::MaxHold, |o| o.reason);
            self.stats.exits[reason_index(reason)].inc();
            self.send_cold(ColdMsg::Exit {
                mint,
                sig,
                reason,
                venue: t.venue,
                tokens: t.token_amount,
                sol_delta: t.sol_amount.saturating_sub(t.fee_lamports) as i64,
                slot: t.slot,
                owner,
                try_no: order.map_or(0, |o| o.try_no),
            });
        }
        self.exit_event(i, ExitEvent::Trade(t), now);
        if let Some(s) = self.slots[i].as_ref() {
            let snap = s.pos;
            self.send_cold(ColdMsg::Position(snap));
        }
    }

    /// A real buy landed and failed (`cost` = `meta.fee`) or nothing landed
    /// (dropped): the intent ends without a position and its slot of the
    /// risk gate comes back, the cost counting toward the daily loss.
    fn real_failed(&mut self, intent: u64, cost: u64, dropped: bool) {
        let Some(i) = self.find_intent(intent) else {
            return;
        };
        let Some(s) = self.slots[i].as_ref() else {
            return;
        };
        if !s.real || s.filled {
            return; // our own TradeEvent already opened it: the trade wins
        }
        let mint = s.pos.mint;
        self.slots[i] = None;
        self.risk.on_attempt_failed(cost, crate::now_unix());
        if dropped {
            self.counters.real_dropped += 1;
        } else {
            self.counters.real_failed += 1;
        }
        self.counters.unfilled += 1;
        self.stats.unfilled.inc();
        self.stats.positions_open.set(self.positions() as i64);
        self.send_cold(ColdMsg::UnwatchMint(mint));
    }

    /// Recovery (real modes): a token balance found on chain at start gets an
    /// exit owner. The position opens on the balance (`orphan` origin) with
    /// the curve keys derived from the mint, and its balance is polled.
    /// `false` when no position slot is free.
    pub fn adopt(&mut self, mint: Pubkey, tokens: u64, intent: u64) -> bool {
        if tokens == 0 || self.find_mint(&mint).is_some() {
            return true;
        }
        let Some(free) = self.slots.iter().position(|s| s.is_none()) else {
            return false;
        };
        // an orphan balance (no intent of ours) gets the intent id a decision
        // on that mint would have had
        let intent = if intent == 0 {
            IntentId::new(&mint, &self.owner).0
        } else {
            intent
        };
        let bc = pda::bonding_curve(&mint);
        let keys = MintKeys::curve(
            mint,
            bc,
            pda::associated_bonding_curve(&mint, &TOKEN_2022_PROGRAM_ID),
            pda::ata_spl(&bc, &WSOL_MINT),
            pda::ata_token_2022(&self.owner, &mint),
        );
        let pos = Position::pending(
            mint,
            self.owner,
            intent,
            self.cfg.policy,
            BundleSet::default(),
            Pubkey::default(),
            self.cfg.fees,
            pump_codec::curve::TOKEN_TOTAL_SUPPLY,
        );
        let slot = self.slot_now;
        self.slots[free] = Some(Slot {
            pos,
            intent,
            op_slot: 0,
            create_slot: slot,
            spend: 0,
            v0: 0,
            t0: 0,
            min_out: 0,
            min_out_e2: 0,
            nonce_idx: u16::MAX,
            keys,
            sig0: [0; 64],
            filled: true,
            real: true,
            deadline: u64::MAX,
            buf: [None; TRADE_BUF],
            buf_len: 0,
            attempts: [None; MAX_ATTEMPTS],
            e1: false,
            replaying: false,
            replay_slot: slot,
            feat: None,
            own_sigs: [[0u8; 64]; OWN_SIGS],
            own_n: 0,
        });
        let now = Now {
            ms: crate::now_ms(),
            slot,
        };
        self.exit_event(free, ExitEvent::Balance { tokens, slot }, now);
        self.send_cold(ColdMsg::WatchMint(mint));
        self.send_cold(ColdMsg::RealHeld { intent, mint });
        self.stats.positions_open.set(self.positions() as i64);
        true
    }

    /// Test and replay hook: hand a cold-lane message to the loop directly.
    pub fn inject(&mut self, m: HotIn) {
        self.on_hot_in(m);
    }

    /// Record the shadow entry (rslog + cold) of a resolved E1 intent.
    fn record_shadow(&mut self, i: usize, outcome: ShadowOutcome, info: ShadowInfo, cost: u64) {
        let Some(s) = self.slots[i].as_ref() else {
            return;
        };
        let (intent, mint, spend) = (s.intent, s.pos.mint, s.spend);
        self.rslog.push(Rec::Shadow {
            intent,
            mint,
            model: 1,
            outcome: outcome as u8,
            create_slot: info.create_slot,
            create_index: info.create_index.unwrap_or(u32::MAX),
            block_txs: info.block_txs.unwrap_or(0),
            bundle_trades: info.bundle_trades.min(255) as u8,
            bundle_wallets: info.bundle_wallets.min(255) as u8,
            our_index: info.our_index.unwrap_or(u32::MAX),
            tp_ahead: info.tp_ahead.min(255) as u8,
            tp_behind: info.tp_behind.min(255) as u8,
            s0_failed: info.s0_failed.min(255) as u8,
            premium_bps: info.premium_bps,
            post_bundle_v: info.post_bundle_v,
            post_bundle_t: info.post_bundle_t,
            post_bundle_mcap: info.post_bundle_mcap,
        });
        if !matches!(
            outcome,
            ShadowOutcome::Filled | ShadowOutcome::Unresolved | ShadowOutcome::Filtered
        ) {
            self.rslog.push(Rec::FailedBuy {
                intent,
                mint,
                reason: outcome as u8,
                cost,
                slot: info.create_slot,
                premium_bps: info.premium_bps,
            });
        }
        self.send_cold(ColdMsg::Shadow {
            intent,
            mint,
            e1: true,
            outcome,
            info,
            spend,
            cost,
        });
    }

    /// The entry filter on an E1 verdict (docs/04 section 2): score the
    /// intent's features (allocation-free), record the score and its tertile
    /// (rslog `Filter`, cold `FilterScore`) and, in `skip` mode, turn an
    /// attempt in the bottom tertile into `Filtered` (no send, no cost).
    /// Without a model the verdict passes unchanged.
    fn filter_verdict(&mut self, i: usize, outcome: ShadowOutcome) -> ShadowOutcome {
        let Some(m) = self.cfg.filter.as_deref() else {
            return outcome;
        };
        let Some(s) = self.slots[i].as_ref() else {
            return outcome;
        };
        let (intent, mint, feat) = (s.intent, s.pos.mint, s.feat);
        let scored = feat.map(|(f, op_n, bw_n)| {
            let score = m.score(&f);
            (score, m.tertile(score), op_n, bw_n)
        });
        let attempt = outcome == ShadowOutcome::Filled || outcome.is_failed_attempt();
        let skipped = self.cfg.filter_mode == FilterMode::Skip
            && attempt
            && scored.is_some_and(|(_, t, _, _)| t == 0);
        let (score, tertile, op_n, bw_n) = scored.unwrap_or((f64::NAN, u8::MAX, 0, 0));
        self.rslog.push(Rec::Filter {
            intent,
            mint,
            score_bits: score.to_bits(),
            tertile,
            mode: (self.cfg.filter_mode == FilterMode::Skip) as u8,
            skipped: skipped as u8,
            op_lead_n: op_n,
            bw_lead_n: bw_n,
        });
        self.send_cold(ColdMsg::FilterScore {
            intent,
            mint,
            scored: scored.is_some(),
            score,
            tertile,
            skipped,
        });
        if skipped {
            ShadowOutcome::Filtered
        } else {
            outcome
        }
    }

    /// The E1 verdict: open the position at the post-bundle state with the
    /// premium, or record the failed attempt and its cost.
    fn e1_open(&mut self, i: usize, outcome: ShadowOutcome, info: ShadowInfo) {
        let fees = self.cfg.fees;
        let outcome = self.filter_verdict(i, outcome);
        let Some(s) = self.slots[i].as_ref() else {
            return;
        };
        let mut outcome = outcome;
        let mut fill = None;
        if outcome == ShadowOutcome::Filled {
            let state = CurveState {
                virtual_sol: info.post_bundle_v,
                virtual_token: info.post_bundle_t,
                real_sol: 0,
                real_token: u64::MAX,
                complete: false,
                creator: s.pos.creator,
            };
            // the premium: fewer tokens for the same SOL (tokens / (1 + p))
            let q = state.quote_buy_exact_sol_in(s.spend, fees).map(|q| {
                let tokens =
                    (q.tokens as u128 * 10_000 / (10_000 + info.premium_bps as u128)) as u64;
                (q, tokens)
            });
            match q {
                // the real buy's on-chain bound (min_tokens_out at V_cap)
                Some((q, tokens)) if tokens >= s.min_out && tokens > 0 => fill = Some((q, tokens)),
                _ => outcome = ShadowOutcome::Slippage,
            }
        }
        let cost = if outcome.is_failed_attempt() {
            // modelled per leader class (base fee + the matching variant's
            // priority fee; tips revert) unless a fixed override is set
            let n_sigs = self
                .tmpl
                .buys
                .first()
                .map_or(1, |t| t.table.n_sigs.clamp(1, 2) as u8);
            let class = self.slots[i]
                .as_ref()
                .and_then(|s| self.leaders.load().class_at(s.create_slot))
                .map(ClientClass::bid_class);
            self.cfg.failed_attempt_cost_at(
                class,
                self.cfg.bid_lamports.min(self.cfg.max_bid),
                n_sigs,
            )
        } else {
            0
        };
        self.record_shadow(i, outcome, info, cost);
        let Some(s) = self.slots[i].as_mut() else {
            return;
        };
        let (mint, intent, sig0, slot0) = (s.pos.mint, s.intent, s.sig0, s.create_slot);
        let Some((q, tokens)) = fill else {
            if outcome == ShadowOutcome::Filtered {
                self.counters.filtered += 1;
            } else {
                self.counters.e1_failed += 1;
            }
            self.counters.unfilled += 1;
            self.stats.unfilled.inc();
            self.slots[i] = None;
            self.stats.positions_open.set(self.positions() as i64);
            // a filtered intent was never sent: no fill row
            if outcome != ShadowOutcome::Unresolved && outcome != ShadowOutcome::Filtered {
                self.send_cold(ColdMsg::Fill {
                    intent,
                    mint,
                    sig: sig0,
                    slot: slot0,
                    tokens: 0,
                    sol_delta: -(cost as i64),
                    filled: false,
                    late: false,
                    buyers_ahead: info.tp_ahead,
                    rival_sol_ahead: 0,
                    fail: Some(outcome),
                    fail_cost: cost,
                });
            }
            self.send_cold(ColdMsg::UnwatchMint(mint));
            return;
        };
        // open at the post-bundle state; others' later trades come without us
        s.filled = true;
        s.e1 = true;
        s.replaying = true;
        s.replay_slot = slot0;
        s.pos.bundle = BundleSet::default(); // the lane sends the insiders next
        let (v_ref, t_ref) = (info.post_bundle_v, info.post_bundle_t);
        let t_after = t_ref.saturating_sub(tokens).max(1);
        let v_after = ((v_ref as u128) * (t_ref as u128) / t_after as u128) as u64;
        let own = Trade {
            venue: Venue::Curve,
            mint,
            user: self.owner,
            is_buy: true,
            sol_amount: q.sol_cost,
            token_amount: tokens,
            fee_lamports: q.protocol_fee + q.creator_fee,
            reserve_sol: v_after,
            reserve_token: t_after,
            slot: slot0,
        };
        self.counters.fills += 1;
        self.stats.fills.inc();
        self.send_cold(ColdMsg::Fill {
            intent,
            mint,
            sig: sig0,
            slot: slot0,
            tokens,
            sol_delta: -(q.total as i64),
            filled: true,
            late: false,
            buyers_ahead: info.tp_ahead,
            rival_sol_ahead: 0,
            fail: None,
            fail_cost: 0,
        });
        let now = match self.slots[i].as_ref() {
            Some(s) => self.e1_now(s, slot0),
            None => return,
        };
        self.exit_event(i, ExitEvent::Trade(own), now);
    }

    /// One tape trade: a tick for every slot without trades before it, then
    /// the trade on the counterfactual curve.
    fn replay_trade(&mut self, i: usize, trade: Trade) {
        let Some(s) = self.slots[i].as_ref() else {
            return;
        };
        if !s.replaying {
            return;
        }
        for x in s.replay_slot + 1..trade.slot {
            let Some(s) = self.slots[i].as_ref() else {
                return;
            };
            let now = self.e1_now(s, x);
            self.exit_event(i, ExitEvent::Tick, now);
        }
        let Some(s) = self.slots[i].as_mut() else {
            return;
        };
        s.replay_slot = s.replay_slot.max(trade.slot);
        let tr = Self::counterfactual(s, trade);
        let now = e1_clock(s.create_slot, self.cfg.slot_ms, trade.slot);
        self.counters.e1_replayed_trades += 1;
        self.exit_event(i, ExitEvent::Trade(tr), now);
    }

    /// The tape is complete up to `horizon`: tick through it, then hand the
    /// position to the live stream (buffered trades after the horizon first).
    fn replay_end(&mut self, i: usize, horizon: u64) {
        let Some(s) = self.slots[i].as_ref() else {
            return;
        };
        if !s.replaying {
            return;
        }
        for x in s.replay_slot + 1..=horizon {
            let Some(s) = self.slots[i].as_ref() else {
                return;
            };
            let now = self.e1_now(s, x);
            self.exit_event(i, ExitEvent::Tick, now);
        }
        let Some(s) = self.slots[i].as_mut() else {
            return;
        };
        s.replaying = false;
        s.replay_slot = s.replay_slot.max(horizon);
        let (buf, n) = (s.buf, s.buf_len);
        s.buf_len = 0;
        for tr in buf[..n].iter().flatten() {
            let Some(s) = self.slots[i].as_ref() else {
                return;
            };
            if tr.slot > s.replay_slot {
                let (t2, now) = (Self::counterfactual(s, *tr), self.e1_now(s, tr.slot));
                self.exit_event(i, ExitEvent::Trade(t2), now);
            }
        }
    }

    /// Simulated buy fill at the E2 state `(v, t)`.
    fn buy_fill(
        &mut self,
        i: usize,
        v: u64,
        t: u64,
        fill_slot: u64,
        late: bool,
        buyers_ahead: u32,
        rival_sol_ahead: u64,
    ) {
        let fees = self.cfg.fees;
        let Some(s) = self.slots[i].as_mut() else {
            return;
        };
        let state = CurveState {
            virtual_sol: v,
            virtual_token: t,
            real_sol: 0,
            real_token: u64::MAX,
            complete: false,
            creator: s.pos.creator,
        };
        let q = state
            .quote_buy_exact_sol_in(s.spend, fees)
            .filter(|q| q.tokens >= s.min_out_e2);
        let (mint, intent, sig0) = (s.pos.mint, s.intent, s.sig0);
        let Some(q) = q else {
            // E2 above v_max: the on-chain bound would have failed the buy
            self.counters.unfilled += 1;
            self.stats.unfilled.inc();
            self.slots[i] = None;
            self.stats.positions_open.set(self.positions() as i64);
            self.send_cold(ColdMsg::Fill {
                intent,
                mint,
                sig: sig0,
                slot: fill_slot,
                tokens: 0,
                sol_delta: 0,
                filled: false,
                late,
                buyers_ahead,
                rival_sol_ahead,
                fail: None,
                fail_cost: 0,
            });
            self.send_cold(ColdMsg::UnwatchMint(mint));
            return;
        };
        s.filled = true;
        let own = Trade {
            venue: Venue::Curve,
            mint,
            user: self.owner,
            is_buy: true,
            sol_amount: q.sol_cost,
            token_amount: q.tokens,
            fee_lamports: q.protocol_fee + q.creator_fee,
            reserve_sol: v.saturating_add(q.sol_cost),
            reserve_token: t.saturating_sub(q.tokens),
            slot: fill_slot,
        };
        let buf = s.buf;
        let n = s.buf_len;
        s.buf_len = 0;
        self.counters.fills += 1;
        self.stats.fills.inc();
        if late {
            self.stats.fills_late.inc();
        }
        self.send_cold(ColdMsg::Fill {
            intent,
            mint,
            sig: sig0,
            slot: fill_slot,
            tokens: q.tokens,
            sol_delta: -(q.total as i64),
            filled: true,
            late,
            buyers_ahead,
            rival_sol_ahead,
            fail: None,
            fail_cost: 0,
        });
        let now_ms = crate::now_ms();
        self.exit_event(
            i,
            ExitEvent::Trade(own),
            Now {
                ms: now_ms,
                slot: fill_slot,
            },
        );
        // trades seen while the fill was pending, after the fill slot
        for tr in buf[..n].iter().flatten() {
            if tr.slot > fill_slot && self.slots[i].is_some() {
                self.exit_event(
                    i,
                    ExitEvent::Trade(*tr),
                    Now {
                        ms: now_ms,
                        slot: tr.slot,
                    },
                );
            }
        }
    }

    /// E2 from the buffered trades (deadline fallback): the last trade at or
    /// before slot+1, else the state before the first later trade (late),
    /// else V0 (late).
    fn local_e2(s: &Slot) -> (u64, u64, bool) {
        let s1 = s.create_slot + 1;
        let mut last_le = None;
        let mut first_gt = None;
        for tr in s.buf[..s.buf_len].iter().flatten() {
            if tr.slot <= s1 {
                last_le = Some(*tr);
            } else if first_gt.is_none() {
                first_gt = Some(*tr);
            }
        }
        if let Some(tr) = last_le {
            return (tr.reserve_sol, tr.reserve_token, false);
        }
        if let Some(tr) = first_gt {
            let (v, t) = if tr.is_buy {
                (
                    tr.reserve_sol.saturating_sub(tr.sol_amount),
                    tr.reserve_token.saturating_add(tr.token_amount),
                )
            } else {
                (
                    tr.reserve_sol.saturating_add(tr.sol_amount),
                    tr.reserve_token.saturating_sub(tr.token_amount),
                )
            };
            return (v, t, true);
        }
        (s.v0, s.t0, true)
    }

    fn tick(&mut self, now_ns: u64) {
        if self.cfg.require_bundle {
            self.expire_parked();
        }
        let now = Now {
            ms: crate::now_ms(),
            slot: self.slot_now,
        };
        for i in 0..MAX_POS {
            let Some(s) = self.slots[i].as_ref() else {
                continue;
            };
            if !s.filled {
                // The local E2 fallback is a paper fill model. A real intent's
                // position opens only on its own TradeEvent or a token balance,
                // never on a send (docs/06 section 8), so it is never simulated.
                if !s.real && self.slot_now >= s.deadline {
                    if self.cfg.fill_lane && self.cfg.fill_model == FillModel::E1 {
                        // the E1 lane never answered: unresolved, no position
                        let info = ShadowInfo {
                            create_slot: s.create_slot,
                            ..Default::default()
                        };
                        self.e1_open(i, ShadowOutcome::Unresolved, info);
                    } else {
                        let (v, t, late) = Self::local_e2(s);
                        let fill_slot = s.create_slot + 1;
                        self.buy_fill(i, v, t, fill_slot, late, 0, 0);
                    }
                }
                continue;
            }
            if s.replaying {
                continue; // the replay drives this position's clock
            }
            let now = if s.e1 { self.e1_now(s, now.slot) } else { now };
            self.exit_event(i, ExitEvent::Tick, now);
        }
        // held real positions marked at their current curve (daily loss)
        if now_ns.saturating_sub(self.last_mark_ns) >= 1_000_000_000 {
            self.last_mark_ns = now_ns;
            for i in 0..MAX_POS {
                let snap = match self.slots[i].as_ref() {
                    Some(s) if s.real && s.filled => s.pos,
                    _ => continue,
                };
                self.send_cold(ColdMsg::Mark(snap));
            }
        }
        if now_ns.saturating_sub(self.last_gauge_ns) >= 2_000_000_000 {
            self.last_gauge_ns = now_ns;
            self.stats
                .decision_p50_ns
                .set(self.hist_decision.value_at_quantile(0.5) as i64);
            self.stats
                .decision_p99_ns
                .set(self.hist_decision.value_at_quantile(0.99) as i64);
            self.stats
                .event_p99_ns
                .set(self.hist_event.value_at_quantile(0.99) as i64);
            self.stats.rslog_dropped.set(self.rslog.dropped() as i64);
            self.stats.nonces_free.set(self.nonces.free_count() as i64);
        }
    }

    fn exit_event(&mut self, i: usize, ev: ExitEvent, now: Now) {
        self.try_sell_fills(i, now);
        self.apply_exit(i, ev, now);
    }

    fn apply_exit(&mut self, i: usize, ev: ExitEvent, now: Now) {
        let mut acts = std::mem::take(&mut self.acts);
        acts.clear();
        if let Some(s) = self.slots[i].as_mut() {
            decide_into(&mut s.pos, &ev, now, &mut acts);
        }
        for a in acts.iter() {
            self.on_action(i, *a, now);
        }
        self.acts = acts;
    }

    /// Conformance-tape fill rule: each attempt emitted at an earlier slot
    /// gets one chance, in `order_id` order, at the current reserves.
    fn try_sell_fills(&mut self, i: usize, now: Now) {
        loop {
            let Some(s) = self.slots[i].as_mut() else {
                return;
            };
            let mut best: Option<usize> = None;
            for (k, a) in s.attempts.iter().enumerate() {
                if let Some(a) = a {
                    if a.slot + self.cfg.sell_lat_slots < now.slot
                        && best.is_none_or(|b| {
                            s.attempts[b].is_some_and(|x| a.order.order_id < x.order.order_id)
                        })
                    {
                        best = Some(k);
                    }
                }
            }
            let Some(k) = best else {
                return;
            };
            let att = s.attempts[k].take().expect("due");
            let o = att.order;
            let p = &s.pos;
            if !p.state.is_active() || p.tokens < o.tokens || o.tokens == 0 {
                continue;
            }
            let (gross, net, post_v, post_t) = match p.venue {
                Venue::Curve => {
                    let c = CurveState {
                        virtual_sol: p.cur_v,
                        virtual_token: p.cur_t,
                        real_sol: 0,
                        real_token: 0,
                        complete: false,
                        creator: p.creator,
                    };
                    match c.quote_sell(o.tokens, p.fees()) {
                        Some(q) => (
                            q.sol_out,
                            q.net,
                            p.cur_v.saturating_sub(q.sol_out),
                            p.cur_t.saturating_add(o.tokens),
                        ),
                        None => continue,
                    }
                }
                Venue::PumpSwap => {
                    let net = p.market().sell_net(o.tokens, p.policy.amm_fee_bps);
                    let gross = rugsnipe_exit::bounds::amm_sell_net(o.tokens, p.cur_v, p.cur_t, 0);
                    (
                        gross,
                        net,
                        p.cur_v.saturating_sub(gross),
                        p.cur_t.saturating_add(o.tokens),
                    )
                }
            };
            if net < o.min_sol_output {
                continue;
            }
            let fill = Trade {
                venue: p.venue,
                mint: p.mint,
                user: p.owner,
                is_buy: false,
                sol_amount: gross,
                token_amount: o.tokens,
                fee_lamports: gross - net,
                reserve_sol: post_v,
                reserve_token: post_t,
                slot: now.slot,
            };
            let (mint, owner) = (p.mint, p.owner);
            // the slot whose closing state filled the sell (E1 replays tick
            // every slot, so that is exactly trigger + latency)
            let fill_slot = if s.e1 {
                att.slot + self.cfg.sell_lat_slots
            } else {
                now.slot
            };
            self.counters.sell_fills += 1;
            self.stats.sell_fills.inc();
            self.stats.exits[reason_index(o.reason)].inc();
            self.send_cold(ColdMsg::Exit {
                mint,
                sig: if att.sig == [0u8; 64] {
                    sell_sig(o)
                } else {
                    att.sig
                },
                reason: o.reason,
                venue: fill.venue,
                tokens: o.tokens,
                sol_delta: net as i64,
                slot: fill_slot,
                owner,
                try_no: o.try_no,
            });
            self.apply_exit(i, ExitEvent::Trade(fill), now);
            // persist the new size (a close sends its own final snapshot)
            if let Some(s) = self.slots[i].as_ref() {
                let snap = s.pos;
                self.send_cold(ColdMsg::Position(snap));
            }
        }
    }

    fn on_action(&mut self, i: usize, a: ExitAction, now: Now) {
        let Some(s) = self.slots[i].as_ref() else {
            return;
        };
        let intent = s.intent;
        let real = s.real;
        match a {
            ExitAction::Opened { tokens, v_fill, .. } => {
                self.rslog.push(Rec::PosEvent {
                    intent,
                    kind: 1,
                    slot: now.slot,
                    v: v_fill,
                    tokens,
                });
                let snap = s.pos;
                self.send_cold(ColdMsg::Position(snap));
            }
            ExitAction::DumpSignal { reason, slot } => {
                self.stats.dump_signals.inc();
                self.rslog.push(Rec::PosEvent {
                    intent,
                    kind: 2,
                    slot,
                    v: reason_code(reason) as u64,
                    tokens: 0,
                });
                let idx = s.nonce_idx;
                self.send_cold(ColdMsg::BurnNonce { idx, intent });
            }
            ExitAction::Sell(order) => self.sell(i, order, now),
            ExitAction::VenueChanged { .. } => {
                self.rslog.push(Rec::PosEvent {
                    intent,
                    kind: 3,
                    slot: now.slot,
                    v: 0,
                    tokens: s.pos.tokens,
                });
            }
            ExitAction::Closed { slot, .. } => {
                let pos = s.pos;
                let pnl = pos
                    .realized_lamports()
                    .clamp(i64::MIN as i128, i64::MAX as i128) as i64;
                let op_slot = s.op_slot;
                self.rslog.push(Rec::PosEvent {
                    intent,
                    kind: 4,
                    slot,
                    v: pos.cur_v,
                    tokens: 0,
                });
                self.counters.closed += 1;
                self.stats.realized_lamports.inc_by(pnl);
                self.send_cold(ColdMsg::Position(pos));
                self.send_cold(ColdMsg::Closed {
                    mint: pos.mint,
                    op_slot,
                    pnl,
                });
                self.send_cold(ColdMsg::UnwatchMint(pos.mint));
                if real {
                    self.risk.on_position_closed(op_slot, pnl, now.ms / 1_000);
                }
                self.slots[i] = None;
                self.stats.positions_open.set(self.positions() as i64);
            }
            ExitAction::Alert(al) => {
                self.stats.alerts.inc();
                let (kind, v) = match al {
                    rugsnipe_exit::Alert::SellExhausted { order_id, .. } => (0x41u8, order_id),
                    rugsnipe_exit::Alert::SellFailing { order_id, .. } => (0x42u8, order_id),
                };
                self.rslog.push(Rec::Risk {
                    kind,
                    value: v as i64,
                });
            }
        }
    }

    fn sell(&mut self, i: usize, order: SellOrder, now: Now) {
        self.counters.sells += 1;
        self.stats.sells.inc();
        let owned = self.shared.exits_owned.load(Acquire);
        let Some(s) = self.slots[i].as_mut() else {
            return;
        };
        // the sell's timing against the policy, for the journal
        let k8 = key8(&s.pos.mint);
        journal(format_args!(
            "sell {} reason {:?} try {} opened_slot {} trigger_slot {} slot_now {} held_ms {} idle_ms {} tokens {}",
            std::str::from_utf8(&k8).unwrap_or("?"),
            order.reason,
            order.try_no,
            s.pos.opened_slot,
            order.trigger_slot,
            self.slot_now,
            now.ms.saturating_sub(s.pos.opened_ms),
            now.ms.saturating_sub(s.pos.last_trade_ms),
            order.tokens
        ));
        // a paper position (paper mode, or forced to paper in a real mode) never
        // reaches a real sender
        let paper = !(self.cfg.mode.is_real() && s.real);
        let mut sig = [0u8; 64];
        // a real sell needs a recent blockhash (final sells close the
        // account; the sell nonce pool is not built yet)
        let lifetime = if paper {
            Some(PAPER_BLOCKHASH)
        } else {
            self.shared.fresh_blockhash()
        };
        if !owned {
            self.stats.sells_not_owner.inc();
        } else if lifetime.is_none() && order.venue == Venue::Curve {
            self.counters.sells_no_blockhash += 1;
            self.rslog.push(Rec::Risk {
                kind: 0x43,
                value: order.order_id as i64,
            });
        } else if order.venue == Venue::Curve {
            let lifetime = lifetime.unwrap_or(PAPER_BLOCKHASH);
            let t = if order.close_account {
                &mut self.tmpl.sell_final
            } else {
                &mut self.tmpl.sell_partial
            };
            // the sell bid goes through the bid policy: priority floor + tip on
            // a tip-ordered class, the bid as the priority fee elsewhere; never
            // the same bid as both (docs/03 section 6.1)
            let sell_class = self.leaders.load().class_at(self.slot_now + 1);
            let plan = self.cfg.sell_plan_at(
                sell_class.map(ClientClass::bid_class),
                order.tip_lamports,
                t.table.n_sigs.clamp(1, 2) as u8,
            );
            let (prio, tip) = (plan.priority_lamports, plan.tip_lamports);
            t.patch(&Patch {
                keys: &s.keys,
                lifetime: &lifetime,
                amount: order.tokens,
                limit: order.min_sol_output,
                priority_lamports: prio,
                tip_lamports: tip,
                cu_limit: None,
            });
            t.sign(&self.signer, None);
            let v = Variant::from_template(
                t,
                s.intent,
                0x80 | (order.try_no.min(127) as u8),
                VariantKind::Sell,
                prio,
                tip,
                Some(self.tmpl.tip_account),
            );
            sig = v.sig;
            if paper {
                let _ = self.senders.paper.send(&v);
                self.stats.sends[SenderId::Paper as usize].inc();
            } else {
                let job = FireJob {
                    tmpl: t.clone(),
                    intent: s.intent,
                    idx: v.idx,
                    prio,
                    tip,
                    tip_account: self.tmpl.tip_account,
                    bid: order.tip_lamports,
                    real: true,
                    leader: sell_class.map_or(LEADER_UNKNOWN, |c| c as u8),
                };
                if self.fire.push(job).is_err() {
                    self.counters.ring_full += 1;
                }
            }
        }
        self.rslog.push(Rec::ExitFired {
            intent: s.intent,
            reason: reason_code(order.reason),
            tokens: order.tokens,
            min_out: order.min_sol_output,
            sig,
        });
        if owned && paper {
            let same = s
                .attempts
                .iter()
                .position(|a| a.is_some_and(|a| a.order.order_id == order.order_id));
            // a re-send of an order still waiting for its fill (escalation to
            // dump bounds, venue change) does not delay it: the first variant
            // is already on its way, so the attempt keeps the earliest slot
            let slot = same
                .and_then(|k| s.attempts[k])
                .map_or(now.slot, |a| a.slot.min(now.slot));
            let slot_k = same
                .or_else(|| s.attempts.iter().position(|a| a.is_none()))
                .unwrap_or(0);
            s.attempts[slot_k] = Some(Attempt { slot, order, sig });
        }
    }

    /// Positions still waiting or open (state, tokens) for reports.
    pub fn position_states(&self) -> Vec<(Pubkey, PositionState, bool)> {
        self.slots
            .iter()
            .flatten()
            .map(|s| (s.pos.mint, s.pos.state, s.filled))
            .collect()
    }
}

/// Slot clock of a replayed E1 position: `ms = (slot - landing) * slot_ms`.
#[inline]
fn e1_clock(create_slot: u64, slot_ms: u64, slot: u64) -> Now {
    Now {
        ms: E1_MS_BASE + slot.saturating_sub(create_slot) * slot_ms,
        slot,
    }
}

/// Ledger key for a paper sell without a transaction (PumpSwap venue).
fn sell_sig(o: SellOrder) -> [u8; 64] {
    let mut s = [0u8; 64];
    s[..4].copy_from_slice(b"PSEL");
    s[4..8].copy_from_slice(&o.order_id.to_le_bytes());
    s[8..12].copy_from_slice(&o.try_no.to_le_bytes());
    s[12..20].copy_from_slice(&o.trigger_slot.to_le_bytes());
    s[20..28].copy_from_slice(&o.tokens.to_le_bytes());
    s
}

#[cfg(test)]
mod bundle_gate_tests {
    use super::*;
    use crate::convert::hit_from_lite;
    use pump_codec::constants::TOKEN_2022_PROGRAM_ID;
    use rugsnipe_feed::event::{hit_flags as hf, CreateHitLite};
    use rugsnipe_feed::Sig;

    fn hit(user: Pubkey, creator: Pubkey) -> CreateHit {
        hit_from_lite(&CreateHitLite {
            mint: Pubkey::new_unique(),
            bc: Pubkey::new_unique(),
            abc: Pubkey::new_unique(),
            user,
            creator,
            token_program: TOKEN_2022_PROGRAM_ID,
            quote_mint: Pubkey::default(),
            mayhem: false,
            slot: 100,
            sig: Sig([5; 64]),
            intx_sol_in: 1_000_000_000,
            intx_tokens_out: 34_612_903_225_806,
            intx_buys: 1,
            v_after: 31_000_000_000,
            flags: hf::TOKEN_PROGRAM_KNOWN | hf::QUOTE_KNOWN | hf::CREATOR_KNOWN | hf::ABC_KNOWN,
        })
    }

    fn trade(mint: Pubkey, user: Pubkey, is_buy: bool, slot: u64, index: u32) -> TradeLite {
        TradeLite {
            mint,
            user,
            is_buy,
            sol_amount: 500_000_000,
            token_amount: 1,
            fee: 0,
            virtual_sol_reserves: 31_500_000_000,
            virtual_token_reserves: 1,
            slot,
            sig: Sig([9; 64]),
            ordinal: 0,
            index,
        }
    }

    #[test]
    fn only_a_bundle_buy_right_behind_the_create_fires_a_parked_create() {
        let (owner, dev, creator, other) = (
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        );
        let h = hit(dev, creator);
        // the create is transaction 10 of its block; the bundle follows it
        let ci = 10;
        let ok = |t: &TradeLite| bundle_buy_triggers(&h, 100, ci, t, &owner);
        assert!(ok(&trade(h.mint, other, true, 100, ci + 1)));
        assert!(ok(&trade(h.mint, other, true, 100, ci + RUN_MAX_AFTER)));
        // the dev's own buy, the creator's, ours: not a bundle
        assert!(!ok(&trade(h.mint, dev, true, 100, ci + 1)));
        assert!(!ok(&trade(h.mint, creator, true, 100, ci + 1)));
        assert!(!ok(&trade(h.mint, owner, true, 100, ci + 1)));
        // a sell, a later slot, another mint
        assert!(!ok(&trade(h.mint, other, false, 100, ci + 1)));
        assert!(!ok(&trade(h.mint, other, true, 101, ci + 1)));
        assert!(!ok(&trade(Pubkey::new_unique(), other, true, 100, ci + 1)));
        // a sniper further down the block, a buy at or before the create, unknown indices
        assert!(!ok(&trade(
            h.mint,
            other,
            true,
            100,
            ci + RUN_MAX_AFTER + 1
        )));
        assert!(!ok(&trade(h.mint, other, true, 100, ci)));
        assert!(!ok(&trade(h.mint, other, true, 100, u32::MAX)));
        assert!(!bundle_buy_triggers(
            &h,
            100,
            u32::MAX,
            &trade(h.mint, other, true, 100, 11),
            &owner
        ));
    }

    fn parked(h: &CreateHit, ci: u32) -> Parked {
        Parked {
            hit: *h,
            src: FeedSrc::Grpc,
            recv_ns: 0,
            slot: 100,
            bank_id: 0,
            t0: 0,
            intent: 1,
            rates: (None, None),
            parked_ns: 0,
            create_index: ci,
            pb: None,
            pb_index: 0,
            bundle_ns: 0,
            bundle_buys: 0,
            wallets: [Pubkey::default(); RUN_MAX_AFTER as usize],
            n_wallets: 0,
            behind: false,
        }
    }

    fn trade_v(mint: Pubkey, user: Pubkey, is_buy: bool, index: u32, v: u64) -> TradeLite {
        let mut t = trade(mint, user, is_buy, 100, index);
        t.virtual_sol_reserves = v;
        t
    }

    const WAIT: u64 = 2_000_000;

    #[test]
    fn the_park_holds_the_runs_last_state_and_fires_at_its_end() {
        let (owner, dev, creator, w1, w2) = (
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        );
        let h = hit(dev, creator);
        let ci = 10;
        let mut p = parked(&h, ci);
        // the dev's own buy at +1: the state moves, no bundle buy yet
        let s = park_trade(
            &mut p,
            &trade_v(h.mint, dev, true, ci + 1, 32_000_000_000),
            &owner,
            WAIT,
            1_000,
        );
        assert_eq!(s, ParkStep::Hold);
        assert_eq!((p.bundle_buys, p.pb), (0, Some((32_000_000_000, 1))));
        assert!(
            !park_due(&p, WAIT, 1_000 + WAIT),
            "nothing to fire without a bundle buy"
        );
        // the first bundle wallet at +2 arms the park and starts the wait
        let s = park_trade(
            &mut p,
            &trade_v(h.mint, w1, true, ci + 2, 33_000_000_000),
            &owner,
            WAIT,
            5_000,
        );
        assert_eq!(s, ParkStep::Hold);
        assert_eq!((p.bundle_buys, p.bundle_ns, p.pb_index), (1, 5_000, ci + 2));
        assert!(!park_due(&p, WAIT, 5_000 + WAIT - 1));
        assert!(park_due(&p, WAIT, 5_000 + WAIT));
        // the second at +3: the state is the highest index's
        let s = park_trade(
            &mut p,
            &trade_v(h.mint, w2, true, ci + 3, 34_000_000_000),
            &owner,
            WAIT,
            6_000,
        );
        assert_eq!(s, ParkStep::Hold);
        assert_eq!(
            (p.bundle_buys, p.bundle_ns, p.pb),
            (2, 5_000, Some((34_000_000_000, 1)))
        );
        // a copy of +2 arriving late never regresses the state
        let s = park_trade(
            &mut p,
            &trade_v(h.mint, w1, true, ci + 2, 33_000_000_000),
            &owner,
            WAIT,
            6_500,
        );
        assert_eq!(s, ParkStep::Hold);
        assert_eq!(p.pb, Some((34_000_000_000, 1)));
        // a bundle wallet's sell at +4 is the curve we buy on
        let s = park_trade(
            &mut p,
            &trade_v(h.mint, w1, false, ci + 4, 33_500_000_000),
            &owner,
            WAIT,
            7_000,
        );
        assert_eq!(s, ParkStep::Hold);
        assert_eq!((p.bundle_buys, p.pb), (3, Some((33_500_000_000, 1))));
        // a trade at +5: the run cannot grow, fire at that state
        let s = park_trade(
            &mut p,
            &trade_v(h.mint, w2, true, ci + 5, 35_000_000_000),
            &owner,
            WAIT,
            8_000,
        );
        assert_eq!(s, ParkStep::Fire);
        assert_eq!(p.pb, Some((35_000_000_000, 1)));
        // the two bundle wallets, once each, for the position's bundle set
        // (four qualifying buys: +2, +3, the late copy of +2, +5)
        assert_eq!(
            (&p.wallets[..p.n_wallets as usize], p.bundle_buys),
            (&[w1, w2][..], 4)
        );
    }

    #[test]
    fn a_trade_beyond_the_run_skips_the_park_and_other_slots_are_ignored() {
        let (owner, dev, creator, w1, other) = (
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        );
        let h = hit(dev, creator);
        let ci = 10;
        let mut p = parked(&h, ci);
        assert_eq!(
            park_trade(
                &mut p,
                &trade_v(h.mint, w1, true, ci + 1, 33_000_000_000),
                &owner,
                WAIT,
                1_000
            ),
            ParkStep::Hold
        );
        // a sniper at +6 executed before we fired: rank 1 is gone
        assert_eq!(
            park_trade(
                &mut p,
                &trade_v(h.mint, other, true, ci + 6, 36_000_000_000),
                &owner,
                WAIT,
                2_000
            ),
            ParkStep::NotFirst
        );
        // the same beyond-the-run trade on an unarmed park: remembered, the
        // park stays (the stream may be out of order); a bundle buy that
        // arrives after it cannot make us first
        let mut q = parked(&h, ci);
        assert_eq!(
            park_trade(
                &mut q,
                &trade_v(h.mint, other, true, ci + 7, 36_000_000_000),
                &owner,
                WAIT,
                2_000
            ),
            ParkStep::Hold
        );
        assert!(q.behind && q.bundle_buys == 0 && q.pb.is_none());
        assert_eq!(
            park_trade(
                &mut q,
                &trade_v(h.mint, w1, true, ci + 1, 33_000_000_000),
                &owner,
                WAIT,
                2_500
            ),
            ParkStep::NotFirst
        );
        assert_eq!(q.bundle_buys, 0);
        // another slot, no block order, the create's own index, no create index
        let mut r = parked(&h, ci);
        let mut t = trade_v(h.mint, w1, true, ci + 1, 33_000_000_000);
        t.slot = 101;
        assert_eq!(
            park_trade(&mut r, &t, &owner, WAIT, 1_000),
            ParkStep::Ignore
        );
        let t = trade_v(h.mint, w1, true, u32::MAX, 33_000_000_000);
        assert_eq!(
            park_trade(&mut r, &t, &owner, WAIT, 1_000),
            ParkStep::Ignore
        );
        let t = trade_v(h.mint, w1, true, ci, 33_000_000_000);
        assert_eq!(
            park_trade(&mut r, &t, &owner, WAIT, 1_000),
            ParkStep::Ignore
        );
        let mut u = parked(&h, u32::MAX);
        let t = trade_v(h.mint, w1, true, ci + 1, 33_000_000_000);
        assert_eq!(
            park_trade(&mut u, &t, &owner, WAIT, 1_000),
            ParkStep::Ignore
        );
        assert_eq!((r.bundle_buys, r.pb, u.bundle_buys), (0, None, 0));
    }

    #[test]
    fn no_wait_fires_on_the_first_bundle_buy() {
        let (owner, dev, creator, w1) = (
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        );
        let h = hit(dev, creator);
        let ci = 10;
        let mut p = parked(&h, ci);
        assert_eq!(
            park_trade(
                &mut p,
                &trade_v(h.mint, dev, true, ci + 1, 32_000_000_000),
                &owner,
                0,
                1_000
            ),
            ParkStep::Hold,
            "the creator's buy is not a bundle buy"
        );
        assert_eq!(
            park_trade(
                &mut p,
                &trade_v(h.mint, w1, true, ci + 2, 33_000_000_000),
                &owner,
                0,
                2_000
            ),
            ParkStep::Fire
        );
        assert_eq!(p.pb, Some((33_000_000_000, 1)));
    }
}
