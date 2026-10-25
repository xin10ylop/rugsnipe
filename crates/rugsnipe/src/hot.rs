//! The hot loop (docs/06 section 7.1), run to completion on one thread:
//!
//! ```text
//! 1  ev = feed.poll()                                  (FeedMux: cross-feed dedup by signature)
//! 2  FiredSet.first(mint, class)                       (free feeds use their own class: never claim a live slot)
//! 3  hit = ev.hit (logs) | create_hit(TxView)          (wire)
//! 4  class filters: SOL quote, not mayhem, Token-2022, not cashback
//! 5  watchlist: signer / fee payer = live; creator_arg only = shadow
//! 6  age <= max_age_slots
//! 6b presend gate (entry.presend_mode): scored before risk; `skip` below the cut sends nothing
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
use crate::filter::{presend_features, CreateRate, Feat, Model, OpStats, S0Trade};
use crate::leader::{ClientClass, LeaderSnapshot, UNKNOWN as LEADER_UNKNOWN};
use crate::msgs::{reason_code, reason_index, ColdMsg, FireJob, HotIn, ShadowInfo};
use crate::operators::{
    bundle_shape_ok, bundle_wallet_count, create_signer, in_creator_set, OpRule, Operators,
};
use crate::setup::{min_tokens_out, min_tokens_out_at, op_armed, ArmState, Templates};
use crate::stats::{code, HotStats};
use crate::tweet::{FailNote, TriesNote, TweetKind, TweetReq, TweetVerdict};
use crate::tweetfeed::{Classified, FeedCall, Posts};
use arc_swap::ArcSwap;
use hdrhistogram::Histogram;
use pump_codec::constants::{TOKEN_2022_PROGRAM_ID, WSOL_MINT};
use pump_codec::create_hit::CreateHit;
use pump_codec::curve::{CurveState, FeeBps};
use pump_codec::{pda, Pubkey};
use rugsnipe_core::bid::{BidPlan, BidPolicy, LeaderClass};
use rugsnipe_core::config::{
    Config, FillModel, FilterMode, LeaderMix, PresendCut, PresendMode, ProbeStage, TweetFeed,
    TweetGate,
};
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
use rugsnipe_tx::template::{MintKeys, Patch, Template};
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
/// Lifetime of a real sell handed to the sender thread with none: the
/// sender resolves a recent blockhash before signing (an all-zero
/// blockhash is never valid on chain).
pub const NO_LIFETIME: [u8; 32] = [0; 32];
/// A real position whose sells could not be sent for this long is handed
/// to the guard (journal line + `guard_handoff:<mint>` row); the hot loop
/// keeps retrying every slot.
pub const HANDOFF_MS: u64 = 5_000;
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
    /// The sell nonce pool (probe/live: `tx.sell_nonce_pool` accounts of
    /// the wallet, seeded at start; unset in paper). The hot loop claims
    /// from it when no blockhash is fresh; the cold thread watches / burns.
    pub sell_nonces: std::sync::OnceLock<Arc<NoncePool>>,
    /// How long no fresh blockhash has been available (ms; 0 = fresh now).
    pub blockhash_age_ms: AtomicU64,
    /// Buys paused: no fresh blockhash for more than
    /// `risk.max_blockhash_age_ms` and no free sell nonce.
    pub sell_blocked: AtomicBool,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            slot_now: AtomicU64::new(0),
            heartbeat: AtomicU64::new(0),
            exits_owned: AtomicBool::new(true),
            stop: AtomicBool::new(false),
            blockhash: std::sync::Mutex::new(None),
            sell_nonces: std::sync::OnceLock::new(),
            blockhash_age_ms: AtomicU64::new(0),
            sell_blocked: AtomicBool::new(false),
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
    /// Free sell nonces with a known value.
    pub fn free_sell_nonces(&self) -> usize {
        self.sell_nonces.get().map_or(0, |p| p.free_count())
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
    /// Buy size by the operator's confidence tier (`entry.stake_by_tier`,
    /// lamports for tiers 0..=2; `None` = off): narrows the ArmSet size.
    pub stake_by_tier: Option<[u64; 3]>,
    /// Base bid by tier (`bid.by_tier`; `None` = `bid_lamports`).
    pub bid_by_tier: Option<[u64; 3]>,
    /// The bid A/B's second arm by tier (`bid.ab_by_tier`): launches whose
    /// mint's last byte is odd bid it.
    pub bid_ab_by_tier: Option<[u64; 3]>,
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
    /// `risk.max_blockhash_age_ms`: buys pause while no fresh blockhash has
    /// been available for longer than this and the sell nonce pool is empty.
    pub max_blockhash_age_ms: u64,
    /// The sender thread resolves a blockhash for a real sell handed over
    /// without one ([`crate::sender::BlockhashResolver`]).
    pub sell_resolver: bool,
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
    /// `entry.dev_buy_sol` as lamport ranges `[lo, hi]`, plus the whole SOL
    /// amounts of `entry.dev_buy_round` (empty = any).
    pub dev_buy: Vec<(u64, u64)>,
    /// `entry.leader_regions` as a mask by [`crate::leader::Region`]
    /// (`None` = any leader; an unknown region is always allowed).
    pub leader_allowed: Option<[bool; crate::leader::Region::COUNT]>,
    /// `entry.dumper_skip` and the hosts its skip applies to
    /// (`entry.dumper_skip_hosts`, matched like `uri_allow`; empty = any).
    pub dumper_skip: rugsnipe_core::config::DumperSkip,
    pub dumper_skip_hosts: Vec<String>,
    /// `entry.hidden_dev_buy = "grpc"`: a create whose dev buy the first
    /// copy cannot see ([`dev_hidden`]) is parked without the dev buy check
    /// and judged on the executed trades ([`hidden_trade`]).
    pub hidden_dev_grpc: bool,
    /// The pre-send entry gate (`entry.presend_model`, loaded at startup;
    /// `None`: off), its mode and cut, and the per-signer operator features
    /// the cold thread publishes from the outcome store.
    pub presend: Option<Arc<Model>>,
    pub presend_mode: PresendMode,
    pub presend_cut: PresendCut,
    pub presend_ops: crate::filter::OpSnapshot,
    /// The operator table (`entry.operators_file`, loaded at startup;
    /// `None`: no table): a listed create signer's positions exit at
    /// `create + hold_slots` (docs/09).
    pub operators: Option<Operators>,
    /// `entry.operators_only`: skip (`skip_not_operator`) a create whose
    /// signer is not in the table.
    pub operators_only: bool,
    /// `entry.operators_exclude`: skip (`skip_not_operator`) a create whose
    /// signer IS in the table (the paper-wide instance, docs/09).
    pub operators_exclude: bool,
    /// `entry.operator_holds`: a listed signer's table hold replaces the
    /// default exit's `hold_to_create_slot` (false: the default for all).
    pub operator_holds: bool,
    /// `entry.min_stake_tier`: skip (`skip_low_tier`) a signer below it.
    pub min_stake_tier: u8,
    /// `entry.bundle_wallets`: the parked run's distinct bundle wallets a
    /// fire needs (empty = any; `skip_bundle_shape`).
    pub bundle_wallets: Vec<u32>,
    /// The pick template (docs/09): `entry.max_post_bundle_sol` and
    /// `entry.max_bundle_sol` in lamports (0 = off).
    pub max_post_bundle: u64,
    pub max_bundle: u64,
    /// `entry.uri_hosts` / `entry.uri_hosts_deny`, lower case, without a
    /// leading dot (empty = any / none).
    pub uri_allow: Vec<String>,
    pub uri_deny: Vec<String>,
    /// The tweet check (`entry.tweet_gate`, docs/13): off / log / on, the
    /// hosts whose creates are fetched (`entry.tweet_hosts`, else
    /// `entry.uri_hosts`; matched like `uri_allow`), the smallest dev buy
    /// fetched (lamports) and the slots after the create slot a passed
    /// verdict may still fire the buy.
    pub tweet_gate: TweetGate,
    pub tweet_hosts: Vec<String>,
    pub tweet_min_dev: u64,
    pub tweet_late_slots: u64,
    /// The tweet feed's copy check (`entry.tweet_feed`, docs/14 section 7)
    /// and how far back a post may match (ms).
    pub tweet_feed: TweetFeed,
    pub tweet_feed_window_ms: u64,
}

/// Three per-tier lamport values (validated by the config), else `None`.
fn tier3(v: &[u64]) -> Option<[u64; 3]> {
    (v.len() == 3).then(|| [v[0], v[1], v[2]])
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
            stake_by_tier: tier3(&cfg.entry.stake_by_tier),
            bid_by_tier: tier3(&cfg.bid.by_tier),
            bid_ab_by_tier: tier3(&cfg.bid.ab_by_tier),
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
            max_blockhash_age_ms: cfg.risk.max_blockhash_age_ms,
            sell_resolver: false,
            bundle_lamports: cfg.entry.bundle_lamports,
            bundle_wait_ns: if cfg.entry.bundle_wait_us > 0 {
                cfg.entry.bundle_wait_us.saturating_mul(1_000)
            } else {
                cfg.entry.bundle_wait_ms.saturating_mul(1_000_000)
            },
            dev_buy: {
                let e = &cfg.entry;
                let mut r = dev_buy_ranges(&e.dev_buy_sol, e.dev_buy_tol_sol);
                if e.dev_buy_round {
                    r.extend(dev_buy_round_ranges(
                        e.dev_buy_round_min,
                        e.dev_buy_round_max,
                        e.dev_buy_tol_sol,
                    ));
                }
                r.extend(
                    e.dev_buy_ranges_sol
                        .iter()
                        .map(|[lo, hi]| (sol_lamports(*lo), sol_lamports(*hi))),
                );
                r
            },
            leader_allowed: leader_mask(&cfg.entry.leader_regions),
            dumper_skip: cfg.entry.dumper_skip,
            dumper_skip_hosts: host_patterns(&cfg.entry.dumper_skip_hosts),
            hidden_dev_grpc: cfg.entry.hidden_dev_buy == rugsnipe_core::config::HiddenDevBuy::Grpc,
            require_bundle: cfg.entry.require_bundle,
            presend: None,
            presend_mode: cfg.entry.presend_mode,
            presend_cut: cfg.entry.presend_cut,
            presend_ops: Arc::new(ArcSwap::from_pointee(Default::default())),
            operators: None,
            operators_only: cfg.entry.operators_only,
            operators_exclude: cfg.entry.operators_exclude,
            operator_holds: cfg.entry.operator_holds,
            min_stake_tier: cfg.entry.min_stake_tier,
            bundle_wallets: cfg.entry.bundle_wallets.clone(),
            max_post_bundle: sol_lamports(cfg.entry.max_post_bundle_sol),
            max_bundle: sol_lamports(cfg.entry.max_bundle_sol),
            uri_allow: host_patterns(&cfg.entry.uri_hosts),
            uri_deny: host_patterns(&cfg.entry.uri_hosts_deny),
            tweet_gate: cfg.entry.tweet_gate,
            tweet_hosts: host_patterns(if cfg.entry.tweet_hosts.is_empty() {
                &cfg.entry.uri_hosts
            } else {
                &cfg.entry.tweet_hosts
            }),
            tweet_min_dev: sol_lamports(cfg.entry.tweet_min_dev_sol),
            tweet_late_slots: cfg.entry.tweet_late_slots,
            tweet_feed: cfg.entry.tweet_feed,
            tweet_feed_window_ms: cfg.entry.tweet_feed_window_s.saturating_mul(1_000),
        }
    }

    /// Any pick-template gate is on (`dev_buy` covers `entry.dev_buy_sol`
    /// too): the status line's `template` fragment.
    pub fn template_on(&self) -> bool {
        self.max_post_bundle > 0
            || self.max_bundle > 0
            || !self.dev_buy.is_empty()
            || !self.uri_allow.is_empty()
            || !self.uri_deny.is_empty()
    }

    /// The operator table's rule for a create's signer ([`create_signer`]:
    /// the fee payer, else the create's user); allocation-free.
    #[inline]
    pub fn operator_rule(&self, hit: &CreateHit) -> Option<OpRule> {
        self.operators.as_deref()?.get(&create_signer(hit)).copied()
    }

    /// The exit policy of a position on `hit`: the configured one, with the
    /// operator table's `hold_slots` as its `hold_to_create_slot` when the
    /// create's signer is listed (docs/09) and `entry.operator_holds` is on.
    #[inline]
    pub fn policy_for(&self, rule: Option<OpRule>) -> ExitPolicy {
        let mut p = self.policy;
        if let Some(r) = rule.filter(|_| self.operator_holds) {
            p.hold_to_create_slot = Some(r.hold_slots);
        }
        p
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
    /// Real sells: the sell-pool nonce this position's sells ride when no
    /// blockhash is fresh (every retry reuses it, so at most one lands).
    sell_claim: Option<rugsnipe_tx::nonce::Claim>,
    /// A sell with the claimed nonce went to the sender (else the claim is
    /// given back unsent at the close).
    sell_claim_sent: bool,
    /// Wall clock (ms) since which no sell of this position could be sent
    /// (0 = the last one went out).
    unsold_since_ms: u64,
    /// Handed to the guard ([`HANDOFF_MS`]).
    handed_off: bool,
    /// Wall clock (ms) of the first try of the sell still pending (sent or
    /// not; 0 = no sell pending): [`HANDOFF_MS`] later without it landing,
    /// the position is handed to the guard, whatever the reason.
    first_sell_ms: u64,
}

/// What the store knew about a position the wallet still holds at start
/// (recovery): kept, only the token amount comes from the wallet.
#[derive(Debug, Clone, Copy, Default)]
pub struct Known {
    pub cost: u64,
    pub opened_slot: u64,
    pub v_fill: u64,
    pub peak_v: u64,
    pub bundle: BundleSet,
    /// The row came from one of our intents (not an orphan balance).
    pub from_intent: bool,
}

/// Own-trade signatures remembered per position.
const OWN_SIGS: usize = 8;

/// `entry.require_bundle`: parked creates at most (one create slot's worth
/// at the highest launch rate seen; a full table drops the oldest).
pub const PARK_SLOTS: usize = 64;
/// A parked create older than this is skipped even without a slot event.
pub const PARK_MAX_NS: u64 = 1_000_000_000;
/// [`Parked::tw`]: no tweet check on this park / waiting for the verdict /
/// the verdict passed (the first coin of a fresh tweet).
pub const TW_OFF: u8 = 0;
pub const TW_WAIT: u8 = 1;
pub const TW_PASS: u8 = 2;
/// Verdicts kept for creates not parked yet when they came.
const TW_EARLY: usize = 16;

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
    /// The bundle run so far ([`derive_run`], re-derived from `run` on
    /// every trade): post-trade reserves `(v, t)` of the last creator-set
    /// or bundle trade of the mint in the create slot (the post-bundle
    /// state we buy on; never a trade by anyone else), that index (0: none
    /// yet), when the first bundle buy was seen and how many qualified.
    pub pb: Option<(u64, u64)>,
    pub pb_index: u32,
    pub bundle_ns: u64,
    pub bundle_buys: u8,
    /// The qualifying bundle buyers (distinct, at most 5): they join the
    /// position's bundle set so the LEAD exit's insider-sell rule sees
    /// their sells (the sim's bundle wallets).
    pub wallets: [Pubkey; RUN_MAX_AFTER as usize],
    pub n_wallets: u8,
    /// A trade of the mint beyond the run was delivered before any bundle
    /// buy (out-of-order stream): a bundle buy that arrives later cannot
    /// make us first any more.
    pub behind: bool,
    /// A buy inside the run by a wallet outside the creator set and the
    /// contiguous bundle (or a run the park could not hold): someone else
    /// bought ahead of us, the fire is `skip_not_first`.
    pub outside: bool,
    /// Lamports into the curve of the run's creator-set and bundle buys
    /// (`entry.max_bundle_sol`, with the create's own buy).
    pub run_sol: u64,
    /// Every trade of the mint inside the run as delivered (the pre-send
    /// gate's bundle block).
    pub run: RunTrades,
    /// The tweet check (`entry.tweet_gate = "on"`): [`TW_OFF`], [`TW_WAIT`]
    /// or [`TW_PASS`]; `run_ready`: the run is complete but the verdict has
    /// not come (the fire waits for it).
    pub tw: u8,
    pub run_ready: bool,
    /// The create's dev buy is hidden from the copy that parked it
    /// ([`dev_hidden`], `entry.hidden_dev_buy = "grpc"`): only executed
    /// trades shape the run, and `dev_sol` sums the create transaction's own
    /// executed buys (`dev_seen` once one came).
    pub dev_hidden: bool,
    pub dev_sol: u64,
    pub dev_seen: bool,
}

/// A create whose dev buy the decoder could not see: no buy found in the
/// transaction, and another program there may buy by CPI (`intx_exact`
/// false: the shreds copy of a launch tool that buys from its own program).
pub fn dev_hidden(hit: &CreateHit) -> bool {
    !hit.intx_exact && hit.intx_sol_in == 0
}

/// A trade of a park whose dev buy is hidden ([`Parked::dev_hidden`]):
/// `Some(())` when it is handled here (a pre-execution quote, priced on a
/// curve without the hidden buy: dropped; the create transaction's own
/// executed buy: added to `dev_sol`), `None` for [`park_trade`].
pub fn hidden_trade(p: &mut Parked, t: &TradeLite, pre_exec: bool) -> Option<()> {
    if !p.dev_hidden || t.slot != p.slot {
        return None;
    }
    if pre_exec {
        return Some(());
    }
    if t.index == p.create_index && p.create_index != u32::MAX {
        if t.is_buy {
            p.dev_sol = p.dev_sol.saturating_add(t.sol_amount);
            p.dev_seen = true;
        }
        return Some(());
    }
    None
}

/// Trades of the run a park keeps: three per run index (a bundle
/// transaction can hold several wallets' buys of the mint). `PARK_RUN + 1`
/// stays within `filter::SMALL_TRADES` (the pre-send gate's
/// allocation-free path).
pub const PARK_RUN: usize = 3 * RUN_MAX_AFTER as usize;
const _: () = assert!(PARK_RUN < crate::filter::SMALL_TRADES);

/// Largest index step between consecutive buys of the bundle run behind
/// the create (the gap-3 run of `filter::bundle_features` and rt.json's
/// `n_bw`: a gap of 3 ends it).
pub const RUN_MAX_GAP: u32 = 2;

/// The run's trades of a parked mint (fixed arrays, `Copy`: no heap on the
/// create path), deduplicated by (transaction index, event ordinal).
#[derive(Debug, Clone, Copy, Default)]
pub struct RunTrades {
    pub t: [S0Trade; PARK_RUN],
    pub ord: [u8; PARK_RUN],
    pub n: u8,
    /// A trade did not fit: the bundle block is incomplete (unscored).
    pub full: bool,
}

impl RunTrades {
    /// Record a trade of the run (a copy already recorded is ignored).
    pub fn push(&mut self, t: &TradeLite) {
        let n = self.n as usize;
        if (0..n).any(|k| self.t[k].index == t.index && self.ord[k] == t.ordinal) {
            return;
        }
        if n == PARK_RUN {
            self.full = true;
            return;
        }
        self.t[n] = S0Trade {
            index: t.index,
            is_create: false,
            user: t.user,
            is_buy: t.is_buy,
            sol: t.sol_amount,
            tok: t.token_amount,
            v: t.virtual_sol_reserves,
            t: t.virtual_token_reserves,
        };
        self.ord[n] = t.ordinal;
        self.n += 1;
    }

    /// The creation slot as seen so far, in block order: the create's own
    /// trade (if any) then the run's (stable by index) into `out`; the count.
    pub fn collect(&self, create: Option<S0Trade>, out: &mut [S0Trade; PARK_RUN + 1]) -> usize {
        let mut k = 0;
        if let Some(c) = create {
            out[0] = c;
            k = 1;
        }
        let start = k;
        for t in &self.t[..self.n as usize] {
            let mut j = k;
            while j > start && out[j - 1].index > t.index {
                out[j] = out[j - 1];
                j -= 1;
            }
            out[j] = *t;
            k += 1;
        }
        k
    }
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
    /// Inside the run: recorded (the run re-derived: the post-bundle
    /// anchor, the bundle buys, an outside buy), still waiting for the run
    /// to complete. Also a trade beyond the run on a park with no bundle
    /// buy yet (the park only remembers it).
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

/// A trade of the parked mint: recorded in the run, which is re-derived
/// ([`derive_run`]: the post-bundle anchor is the last creator-set or
/// bundle trade, never anyone else's); the first bundle buy arms the park.
/// The park fires here when the run cannot grow (a trade at `create +
/// RUN_MAX_AFTER`) or, with `wait_ns == 0`, on the first bundle buy;
/// otherwise the hot loop fires it on the stream's run-complete signal
/// ([`RunDoneLite`]) or when [`park_due`]. A buy by an outside wallet
/// inside the run marks the park `outside`; the fire then skips it as
/// `skip_not_first` (decided at the fire, so a bundle trade delivered out
/// of order still joins the run first).
pub fn park_trade(
    p: &mut Parked,
    t: &TradeLite,
    owner: &Pubkey,
    wait_ns: u64,
    now_ns: u64,
) -> ParkStep {
    // the tweet check's wait runs past the create slot: a later buy by a
    // wallet outside the creator set and the bundle is ahead of ours
    if p.tw != TW_OFF && t.mint == p.hit.mint && t.slot > p.slot {
        let outside = t.is_buy
            && t.user != *owner
            && !in_creator_set(&p.hit, &t.user)
            && !p.wallets[..p.n_wallets as usize].contains(&t.user);
        return if outside && p.bundle_buys > 0 {
            ParkStep::NotFirst
        } else {
            ParkStep::Ignore
        };
    }
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
    p.run.push(t);
    let armed = p.bundle_buys > 0;
    derive_run(p, owner);
    if p.bundle_buys > 0 && !armed {
        if p.behind {
            return ParkStep::NotFirst;
        }
        p.bundle_ns = now_ns;
    }
    if p.bundle_buys > 0 && (t.index == last || wait_ns == 0) {
        ParkStep::Fire
    } else {
        ParkStep::Hold
    }
}

/// Re-derive a park's bundle from every run trade seen so far, in block
/// order (so a trade delivered out of order lands where it belongs). The
/// bundle is the creator set's trades ([`in_creator_set`]: the signer,
/// `creator`, the fee payer, the create transaction's signers and buyers)
/// and the buys chained behind the create at most [`RUN_MAX_GAP`] indices
/// apart (within [`RUN_MAX_AFTER`], the window [`bundle_buy_triggers`]
/// checks); a sell moves the anchor only when it is the creator set's or a
/// bundle wallet's. The post-bundle anchor `pb` is the last bundle trade.
/// Any other buy ends the run and marks the park `outside` (someone else
/// bought first: the anchor never moves to its price), as does a run that
/// did not fit the park (it cannot be checked). The bundle buys and wallets
/// are the bundle's buys by wallets other than the signer and `creator`
/// (ours never count). No allocation.
pub fn derive_run(p: &mut Parked, owner: &Pubkey) {
    let mut buf = [S0Trade::default(); PARK_RUN + 1];
    let n = p.run.collect(None, &mut buf);
    let mut prev = p.create_index;
    p.pb = None;
    p.pb_index = 0;
    p.bundle_buys = 0;
    p.n_wallets = 0;
    p.run_sol = 0;
    p.outside = p.run.full;
    for t in &buf[..n] {
        if t.user == *owner {
            continue;
        }
        let cset = in_creator_set(&p.hit, &t.user);
        if t.is_buy {
            if !cset && t.index.saturating_sub(prev) > RUN_MAX_GAP {
                p.outside = true;
                break;
            }
            prev = prev.max(t.index);
            p.run_sol = p.run_sol.saturating_add(t.sol);
            if t.user != p.hit.user && t.user != p.hit.creator_arg {
                p.bundle_buys = p.bundle_buys.saturating_add(1);
                let k = p.n_wallets as usize;
                if !p.wallets[..k].contains(&t.user) && k < p.wallets.len() {
                    p.wallets[k] = t.user;
                    p.n_wallets += 1;
                }
            }
        } else if !(cset || p.wallets[..p.n_wallets as usize].contains(&t.user)) {
            continue;
        }
        p.pb = Some((t.v, t.t));
        p.pb_index = t.index;
    }
}

/// The wait after the first bundle buy is over.
/// `entry.dev_buy_sol` +- `tol` as lamport ranges.
pub fn dev_buy_ranges(sol: &[f64], tol: f64) -> Vec<(u64, u64)> {
    sol.iter()
        .map(|v| {
            let lo = ((v - tol).max(0.0) * 1e9).round() as u64;
            let hi = ((v + tol) * 1e9).round() as u64;
            (lo, hi)
        })
        .collect()
}

/// `entry.dev_buy_round`: every whole number of SOL in `[min, max]` +-
/// `tol` as lamport ranges (empty when there is none).
pub fn dev_buy_round_ranges(min: f64, max: f64, tol: f64) -> Vec<(u64, u64)> {
    if !(min.is_finite() && max.is_finite()) || min.ceil() > max.floor() {
        return vec![];
    }
    let (lo, hi) = (min.ceil().max(0.0) as u64, max.floor() as u64);
    let whole: Vec<f64> = (lo..=hi).map(|k| k as f64).collect();
    dev_buy_ranges(&whole, tol)
}

/// SOL as lamports (negative and NaN: 0).
pub fn sol_lamports(sol: f64) -> u64 {
    (sol.max(0.0) * 1e9).round() as u64
}

/// `entry.uri_hosts` entries as the hot loop matches them: trimmed, lower
/// case, without a leading dot.
pub fn host_patterns(hosts: &[String]) -> Vec<String> {
    hosts
        .iter()
        .map(|h| h.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|h| !h.is_empty())
        .collect()
}

/// `host` ends in `pat` on a label boundary (`uxento.io` matches
/// `uxento.io` and `meta.uxento.io`, not `xuxento.io`), ASCII
/// case-insensitive; a `:port` on the host is ignored. No allocation.
#[inline]
pub fn host_matches(host: &str, pat: &str) -> bool {
    let h = host.as_bytes();
    let h = match h.iter().position(|b| *b == b':') {
        Some(i) => &h[..i],
        None => h,
    };
    let p = pat.as_bytes();
    if p.is_empty() || p.len() > h.len() {
        return false;
    }
    let k = h.len() - p.len();
    h[k..].eq_ignore_ascii_case(p) && (k == 0 || h[k - 1] == b'.')
}

/// `entry.uri_hosts` / `entry.uri_hosts_deny` on a create's uri host
/// ([`pump_codec::create_hit::TxShape::uri_host`]): in the allow list
/// (empty = any; a create without a host fails a non-empty list) and not
/// in the deny list. No allocation.
#[inline]
pub fn uri_host_ok(allow: &[String], deny: &[String], host: Option<&str>) -> bool {
    match host {
        Some(h) => {
            (allow.is_empty() || allow.iter().any(|p| host_matches(h, p)))
                && !deny.iter().any(|p| host_matches(h, p))
        }
        None => allow.is_empty(),
    }
}

/// `entry.max_bundle_sol`'s amount: the create's own buys plus the parked
/// run's creator-set and bundle buys (lamports into the curve; without a
/// park, the dev buy alone).
#[inline]
pub fn dev_and_bundle_lamports(hit: &CreateHit, parked: Option<&Parked>) -> u64 {
    hit.intx_sol_in.saturating_add(parked.map_or(0, |p| {
        // a hidden dev buy: the executed one ([`hidden_trade`])
        p.run_sol
            .saturating_add(if p.dev_hidden { p.dev_sol } else { 0 })
    }))
}

/// `entry.max_post_bundle_sol`'s reserve: the parked run's post-bundle
/// anchor, else V0 (the curve after the create's own buy).
#[inline]
pub fn post_bundle_sol(hit: &CreateHit, parked: Option<&Parked>) -> u64 {
    parked
        .and_then(|p| p.pb)
        .map_or_else(|| v0_of(hit).0, |(v, _)| v)
}

/// `entry.leader_regions` as an allow mask by region (`None` when empty);
/// an unknown region is always allowed.
pub fn leader_mask(regions: &[String]) -> Option<[bool; crate::leader::Region::COUNT]> {
    if regions.is_empty() {
        return None;
    }
    let mut m = [false; crate::leader::Region::COUNT];
    for r in regions {
        if let Some(reg) = crate::leader::Region::parse(r) {
            m[reg as usize] = true;
        }
    }
    m[crate::leader::Region::Unknown as usize] = true;
    Some(m)
}

/// The skip code of a tweet verdict that does not pass.
pub fn tweet_code(k: TweetKind) -> u8 {
    match k {
        TweetKind::First => code::FIRE,
        TweetKind::Copy => code::TWEET_COPY,
        TweetKind::Stale => code::TWEET_STALE,
        TweetKind::NoTweet => code::TWEET_NONE,
        TweetKind::Error => code::TWEET_ERROR,
    }
}

/// The create's dev buy (lamports) is one of the listed amounts (an empty
/// list admits every create).
#[inline]
pub fn dev_buy_ok(ranges: &[(u64, u64)], lamports: u64) -> bool {
    ranges.is_empty()
        || ranges
            .iter()
            .any(|(lo, hi)| (*lo..=*hi).contains(&lamports))
}

pub fn park_due(p: &Parked, wait_ns: u64, now_ns: u64) -> bool {
    p.bundle_buys > 0 && now_ns.saturating_sub(p.bundle_ns) >= wait_ns
}

/// Whether `t` can be the bundle buy a parked create waits for: a buy of
/// the mint in the create slot, within [`RUN_MAX_AFTER`] transactions after
/// the create in block order (the dev's bundle, the in-class launch:
/// `feed_bundle.bundle_wallets`), by a wallet that is neither ours nor the
/// creator's (the create's signer or `creator` argument). A buy further
/// down the block is another sniper, not the bundle: buying behind one is
/// the entry the LEAD policy was not scored on. Unknown indices never
/// trigger. The park also needs the buy chained to the run (at most
/// [`RUN_MAX_GAP`] behind the previous bundle buy, [`derive_run`]): a buy
/// in the window after a longer gap is an outside buyer ahead of us.
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

/// The journal line of a pick-template skip at the fire point (rare: a
/// parked create that reached its fire), allocation-free.
fn template_line(what: &str, hit: &CreateHit, got: u64, cap: u64) {
    let k8 = key8(&hit.mint);
    journal(format_args!(
        "template {} {what} {:.9} > {:.9} SOL",
        std::str::from_utf8(&k8).unwrap_or("?"),
        got as f64 / 1e9,
        cap as f64 / 1e9
    ));
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
    /// Creates whose dev, creator or a bundle wallet is a known dumper.
    pub dumper_hits: u64,
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
    /// Real sells sent on a sell-pool nonce / handed to the sender without
    /// a lifetime (it resolves a blockhash) / not sent by the sender (no
    /// blockhash within its budget).
    pub sells_on_nonce: u64,
    pub sells_resolve: u64,
    pub sells_unsent: u64,
    /// The pre-send entry gate: decisions scored (bundle block present) /
    /// unscored, skipped (`skip` mode), and the scored ones per tertile
    /// (low, mid, high).
    pub presend_scored: u64,
    pub presend_unscored: u64,
    pub presend_skipped: u64,
    pub presend_tertiles: [u64; 3],
    /// The pick-template gates (docs/09): creates skipped as
    /// `skip_bundle_too_big`, `skip_bundle_sol`, `skip_dev_buy`
    /// (`entry.dev_buy_sol` or `entry.dev_buy_round`) and `skip_uri_host`.
    pub skip_too_big: u64,
    pub skip_bundle_sol: u64,
    pub skip_dev_buy: u64,
    pub skip_uri_host: u64,
    /// Creates parked with a hidden dev buy (`entry.hidden_dev_buy`), and
    /// how their fire went: fired (the executed dev buy passed the
    /// template) or skipped (it failed, or never showed).
    pub hidden_dev: [u64; 3],
    /// The tweet check (`entry.tweet_gate`): creates handed to the lane,
    /// and not handed for a full ring; verdicts by kind (`TweetKind`
    /// order: first, copy, stale, no_tweet, error); verdicts that came in
    /// the create slot / the next / later; gated parks skipped on a verdict
    /// or for want of one, and fired on a pass.
    pub tweet_asked: u64,
    pub tweet_ring_full: u64,
    pub tweet_kinds: [u64; TweetKind::COUNT],
    pub tweet_at: [u64; 3],
    pub tweet_skipped: u64,
    pub tweet_late: u64,
    pub tweet_fired: u64,
    /// `"exit"`: positions whose hold a copy / stale / no-tweet verdict cut
    /// to the next slot.
    pub tweet_cut: u64,
    /// The tweet feed (`entry.tweet_feed`): calls by kind (`FeedCall`
    /// order: none, first, copy) and copies skipped (`"on"`).
    pub feed_calls: [u64; 3],
    pub feed_skipped: u64,
    /// Parked creates at the fire point by the park's bundle-wallet count
    /// (`operators::bundle_wallet_count`, 0..=5): what `entry.bundle_wallets`
    /// is checked against.
    pub fire_wallets: [u64; 6],
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
    /// The operator-dump set (cold thread publishes).
    pub dumpers: crate::dumpers::DumperSnapshot,
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
    parked: Box<[Option<Parked>]>,
    /// Recent trades of mints with no park and no position ([`PREPARK`]).
    /// (trade, a pre-execution quote)
    prepark: [Option<(TradeLite, bool)>; PREPARK],
    prepark_at: usize,
    leaders: LeaderSnapshot,
    dumpers: crate::dumpers::DumperSnapshot,
    /// Scratch for the ALT-loaded addresses of a wire event (no allocation).
    loaded: Box<[Pubkey]>,
    last_mark_ns: u64,
    /// Final / partial sell templates per sell-pool nonce (index = nonce).
    sell_tmpls: Vec<(Template, Template)>,
    /// Wall clock (ms) of the last tick that saw a fresh blockhash.
    bh_fresh_ms: u64,
    /// Buys paused for want of any sell lifetime (see [`HotCfg::max_blockhash_age_ms`]).
    sell_blocked: bool,
    /// The tweet lane's rings (`entry.tweet_gate` != off; `None`: no lane).
    tweet_req: Option<rtrb::Producer<TweetReq>>,
    tweet_in: Option<rtrb::Consumer<TweetVerdict>>,
    /// Verdicts that came before their create was parked.
    tw_early: [Option<TweetVerdict>; TW_EARLY],
    tw_early_at: usize,
    /// Verdict delay: the request's push to the verdict's pop (us).
    hist_tweet: Histogram<u64>,
    /// The tweet feed's posts (`entry.tweet_feed` != off; `None`: no feed).
    feed_posts: Option<Posts>,
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
        FeedSrc::Grpc | FeedSrc::GrpcB | FeedSrc::Shreds | FeedSrc::ShredStream => LIVE_CLASS,
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
            // on the heap (PARK_SLOTS entries of a few KB each, allocated
            // once here): the loop itself stays small enough to build and
            // move on a 2 MB thread stack
            parked: vec![None; PARK_SLOTS].into_boxed_slice(),
            prepark: [None; PREPARK],
            prepark_at: 0,
            leaders: p.leaders,
            dumpers: p.dumpers,
            loaded: vec![Pubkey::default(); MAX_LOADED].into_boxed_slice(),
            last_mark_ns: 0,
            sell_tmpls: Vec::new(),
            bh_fresh_ms: 0,
            sell_blocked: false,
            tweet_req: None,
            tweet_in: None,
            tw_early: [None; TW_EARLY],
            tw_early_at: 0,
            hist_tweet: hist(),
            feed_posts: None,
        }
    }

    /// Connect the tweet feed's post index (`entry.tweet_feed` != off).
    pub fn set_tweet_feed(&mut self, posts: Posts) {
        self.feed_posts = Some(posts);
    }

    /// `entry.tweet_feed` as configured.
    pub fn tweet_feed_mode(&self) -> &'static str {
        self.cfg.tweet_feed.as_str()
    }

    /// Posts in the tweet feed's index (`None`: no feed).
    pub fn feed_posts_len(&self) -> Option<usize> {
        self.feed_posts.as_ref().map(|p| p.load().len())
    }

    /// Connect the tweet lane (`entry.tweet_gate` != off): the requests the
    /// hot loop pushes and the verdicts it pops ([`crate::tweet::spawn`]).
    pub fn set_tweet_lane(
        &mut self,
        req: rtrb::Producer<TweetReq>,
        verdicts: rtrb::Consumer<TweetVerdict>,
    ) {
        self.tweet_req = Some(req);
        self.tweet_in = Some(verdicts);
    }

    /// The tweet check's mode and its verdict delay p50 / p90 (ms), for the
    /// status line (`None`: off).
    pub fn tweet_desc(&self) -> Option<(&'static str, u64, u64)> {
        (self.cfg.tweet_gate != TweetGate::Off).then(|| {
            let h = &self.hist_tweet;
            let q = |p: f64| {
                if h.is_empty() {
                    0
                } else {
                    h.value_at_quantile(p) / 1_000
                }
            };
            (self.cfg.tweet_gate.as_str(), q(0.5), q(0.9))
        })
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

    /// Sell templates riding the sell-pool nonces (`(final, partial)` per
    /// nonce of [`Shared::sell_nonces`], same order); without them a stale
    /// blockhash falls through to the sender's resolver.
    pub fn set_sell_nonce_templates(&mut self, t: Vec<(Template, Template)>) {
        self.sell_tmpls = t;
    }

    /// Buys are paused for want of any sell lifetime.
    pub fn sell_blocked(&self) -> bool {
        self.sell_blocked
    }

    /// Real modes, every tick: how long no blockhash has been fresh, and
    /// the buy pause (no fresh blockhash for more than
    /// `risk.max_blockhash_age_ms` AND no free sell nonce: a position bought
    /// now could not be sold). Transient: it clears on the next fresh
    /// blockhash or a freed sell nonce; one journal line when it starts and
    /// one when it ends; never the kill latch.
    fn blockhash_watch(&mut self, now_ms: u64) {
        if self.shared.fresh_blockhash().is_some() || self.bh_fresh_ms == 0 {
            self.bh_fresh_ms = now_ms;
        }
        let age = now_ms.saturating_sub(self.bh_fresh_ms);
        self.shared.blockhash_age_ms.store(age, Relaxed);
        let free = self.shared.free_sell_nonces();
        let blocked = age > self.cfg.max_blockhash_age_ms && free == 0;
        if blocked != self.sell_blocked {
            self.sell_blocked = blocked;
            self.shared.sell_blocked.store(blocked, Relaxed);
            if blocked {
                journal(format_args!(
                    "buys paused (skip_no_blockhash): no fresh blockhash for {age} ms (> {}) and no free sell nonce",
                    self.cfg.max_blockhash_age_ms
                ));
            } else {
                journal(format_args!(
                    "buys resumed: blockhash age {age} ms, free sell nonces {free}"
                ));
            }
        }
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
        while let Some(v) = self.tweet_in.as_mut().and_then(|c| c.pop().ok()) {
            self.on_tweet(v);
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
            Payload::Trade(t) if ev.src.pre_exec() => {
                // a quote of a trade the chain has not run yet (a shreds
                // feed): the bundle gate's input, never a position's. A
                // sniper's buy behind us that then fails its bound would
                // otherwise mark the position up and fire a take-profit
                // into a price that never existed (2026-10-23).
                if self.cfg.require_bundle
                    && !self.parked_trade(&t, true)
                    && t.index != u32::MAX
                    && self.find_mint(&t.mint).is_none()
                {
                    self.prepark[self.prepark_at % PREPARK] = Some((t, true));
                    self.prepark_at = self.prepark_at.wrapping_add(1);
                }
            }
            Payload::Trade(t) if t.user == self.owner && self.cfg.mode.is_real() => {
                // our own TradeEvent from the feed: opens / sizes a real position
                self.on_own_trade(t.sig.0, trade_from_lite(&t), false);
            }
            Payload::Trade(t) => {
                if !(self.cfg.require_bundle && self.parked_trade(&t, false)) {
                    if self.cfg.require_bundle
                        && t.index != u32::MAX
                        && self.find_mint(&t.mint).is_none()
                    {
                        // a bundle buy polled before its create's wire
                        // record: replayed when the create is parked
                        self.prepark[self.prepark_at % PREPARK] = Some((t, false));
                        self.prepark_at = self.prepark_at.wrapping_add(1);
                    }
                    self.on_trade(trade_from_lite(&t));
                }
            }
            Payload::RunDone(r) => {
                if self.cfg.require_bundle {
                    self.parked_run_done(&r, ev.src.pre_exec());
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

    /// The pre-send entry gate at the fire point (`entry.presend_mode` log
    /// or skip): assemble the pre-send vector, score it, record it (rslog
    /// `Presend`, cold `Presend` for the `presend` row) and say whether
    /// `skip` drops the create (`score < cut`; an unscored create, one
    /// without its own trade or with an overflowing run, is never dropped).
    /// No allocation.
    #[allow(clippy::too_many_arguments)]
    fn presend_gate(
        &mut self,
        intent: u64,
        hit: &CreateHit,
        slot: u64,
        create_index: u32,
        rates: (Option<u32>, Option<u32>),
        parked: Option<&Parked>,
        now_ms: u64,
    ) -> bool {
        let mode = self.cfg.presend_mode;
        if mode == PresendMode::Off || self.cfg.presend.is_none() {
            return false;
        }
        let mut buf = [S0Trade::default(); PARK_RUN + 1];
        let create = crate::filter::create_trade(hit, create_index);
        let (n, full) = match parked {
            Some(p) => (p.run.collect(create, &mut buf), p.run.full),
            None => (RunTrades::default().collect(create, &mut buf), false),
        };
        let op = self
            .cfg
            .presend_ops
            .load()
            .get(&hit.user)
            .copied()
            .unwrap_or_else(|| OpStats::default().features());
        let tx_index = (create_index != u32::MAX).then_some(create_index);
        let (feat, bundle) = presend_features(
            &buf[..n],
            hit,
            tx_index,
            &op,
            (now_ms / 1_000) as i64,
            slot,
            rates,
        );
        let scored = bundle && !full;
        let Some(m) = self.cfg.presend.as_deref() else {
            return false;
        };
        let score = m.score(&feat);
        let tertile = m.tertile(score);
        let cut = match self.cfg.presend_cut {
            PresendCut::T1 => m.t1,
            PresendCut::T2 => m.t2,
        };
        let skipped = mode == PresendMode::Skip && scored && score < cut;
        let c = &mut self.counters;
        if scored {
            c.presend_scored += 1;
            c.presend_tertiles[(tertile as usize).min(2)] += 1;
        } else {
            c.presend_unscored += 1;
        }
        c.presend_skipped += skipped as u64;
        self.rslog.push(Rec::Presend {
            intent,
            mint: hit.mint,
            score_bits: score.to_bits(),
            tertile,
            mode: mode as u8,
            cut: match self.cfg.presend_cut {
                PresendCut::T1 => 1,
                PresendCut::T2 => 2,
            },
            scored: scored as u8,
            skipped: skipped as u8,
        });
        self.send_cold(ColdMsg::Presend {
            intent,
            mint: hit.mint,
            scored,
            score,
            tertile,
            skipped,
            feat,
        });
        skipped
    }

    /// The pick-template gates are on (status line `template` fragment).
    pub fn template_on(&self) -> bool {
        self.cfg.template_on()
    }

    /// `entry.hidden_dev_buy = "grpc"` (the status line's counts).
    pub fn hidden_dev_on(&self) -> bool {
        self.cfg.hidden_dev_grpc
    }

    /// `(mode, cut)` of the pre-send gate when it runs (status line).
    pub fn presend_desc(&self) -> Option<(&'static str, &'static str)> {
        (self.cfg.presend_mode != PresendMode::Off && self.cfg.presend.is_some()).then(|| {
            (
                self.cfg.presend_mode.as_str(),
                self.cfg.presend_cut.as_str(),
            )
        })
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
        // the tweet check's request (entry.tweet_gate): before any skip, so
        // every coin of a tweet host ranks on its tweet, skipped or not
        let asked = self.ask_tweet(hit, slot, create_index);
        // the tweet feed's call (entry.tweet_feed), on the same coins and for
        // the same reason: every coin takes the posts it matches
        let feed = self.feed_call(hit);
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
        // 4a: the operator template filter (entry.dev_buy_sol): the create's
        // own buy must be one of the listed amounts
        // (entry.dev_buy_round adds the whole SOL amounts as an alternative);
        // a dev buy hidden from this copy (entry.hidden_dev_buy = "grpc") is
        // checked at the fire, on the executed trades
        let hidden = self.cfg.hidden_dev_grpc
            && dev_hidden(hit)
            && self.cfg.require_bundle
            && self.cfg.mode.is_real()
            && src_class(src) != FREE_CLASS;
        if !hidden && !dev_buy_ok(&self.cfg.dev_buy, hit.intx_sol_in) {
            self.counters.skip_dev_buy += 1;
            return self.decision(intent, hit, code::DEV_BUY, 0, 0, 0, 0, 0);
        }
        // 4a''': the uri host (entry.uri_hosts / entry.uri_hosts_deny), as
        // the wire decoder kept it
        if (!self.cfg.uri_allow.is_empty() || !self.cfg.uri_deny.is_empty())
            && !uri_host_ok(
                &self.cfg.uri_allow,
                &self.cfg.uri_deny,
                hit.tx_shape.uri_host(),
            )
        {
            self.counters.skip_uri_host += 1;
            return self.decision(intent, hit, code::URI_HOST, 0, 0, 0, 0, 0);
        }
        // 4a'': the operator table (entry.operators_only): only the launches
        // of a listed create signer go on (nothing parked otherwise)
        if self.cfg.operators_only && self.cfg.operator_rule(hit).is_none() {
            return self.decision(intent, hit, code::NOT_OPERATOR, 0, 0, 0, 0, 0);
        }
        // its mirror (entry.operators_exclude, the paper-wide instance): only
        // the launches the live bot leaves out
        if self.cfg.operators_exclude && self.cfg.operator_rule(hit).is_some() {
            return self.decision(intent, hit, code::NOT_OPERATOR, 0, 0, 0, 0, 0);
        }
        // the tier floor (entry.min_stake_tier, docs/09 "stake tier"): only
        // the operators the copied wallet stakes at that tier or above
        if self.cfg.min_stake_tier > 0
            && self.cfg.operator_rule(hit).map_or(0, |r| r.stake_tier) < self.cfg.min_stake_tier
        {
            return self.decision(intent, hit, code::LOW_TIER, 0, 0, 0, 0, 0);
        }
        // 4a': the leader filter (entry.leader_regions): a create whose slot
        // leader the box cannot reach in time is skipped, nothing sent
        if let Some(allowed) = self.cfg.leader_allowed {
            if let Some(r) = self.leaders.load().region_at(slot) {
                if !allowed[r as usize] {
                    return self.decision(intent, hit, code::FAR_LEADER, 0, 0, 0, 0, 0);
                }
            }
        }
        // 4b: the bundle gate (real only): park until another wallet's buy
        // of the mint shows in the create slot and the bundle run is
        // complete, then fire at its post-trade state; the slot moving on
        // without a bundle buy skips the launch (docs/04 section 2: the
        // LEAD policy's population is the bundled launch, entered right
        // behind the whole bundle, bound at that price + max_premium)
        if self.cfg.require_bundle && self.cfg.mode.is_real() && src_class(src) != FREE_CLASS {
            // the tweet check (on): a create the lane was not asked about has
            // no verdict to wait for; a verdict that already came decides now
            let mut tw = TW_OFF;
            // the tweet feed (on): a coin on posts an earlier coin took is a copy
            if self.cfg.tweet_feed == TweetFeed::On
                && feed.is_some_and(|c| c.call == FeedCall::Copy)
            {
                self.counters.feed_skipped += 1;
                return self.decision(intent, hit, code::TWEETFEED_COPY, 0, 0, 0, 0, 0);
            }
            if self.cfg.tweet_gate == TweetGate::Exit {
                if let Some(v) = self.take_early_verdict(&hit.mint) {
                    if matches!(
                        v.kind,
                        TweetKind::Copy | TweetKind::Stale | TweetKind::NoTweet
                    ) {
                        self.counters.tweet_skipped += 1;
                        return self.decision(intent, hit, tweet_code(v.kind), 0, 0, 0, 0, 0);
                    }
                }
            }
            if self.cfg.tweet_gate == TweetGate::On {
                if !asked {
                    self.counters.tweet_skipped += 1;
                    return self.decision(intent, hit, code::TWEET_NONE, 0, 0, 0, 0, 0);
                }
                tw = TW_WAIT;
                if let Some(v) = self.take_early_verdict(&hit.mint) {
                    if v.kind != TweetKind::First {
                        self.counters.tweet_skipped += 1;
                        return self.decision(intent, hit, tweet_code(v.kind), 0, 0, 0, 0, 0);
                    }
                    tw = TW_PASS;
                }
            }
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
                outside: false,
                run_sol: 0,
                run: RunTrades::default(),
                tw,
                run_ready: false,
                dev_hidden: hidden,
                dev_sol: 0,
                dev_seen: false,
            });
            if hidden {
                self.counters.hidden_dev[0] += 1;
            }
            return;
        }
        self.decide_from_watch(
            src,
            recv_ns,
            slot,
            bank_id,
            hit,
            t0,
            intent,
            rates,
            None,
            create_index,
        );
    }

    /// Hand a create to the tweet lane (`entry.tweet_gate` != off): a
    /// tweet host's uri, kept whole, and a dev buy of at least
    /// `entry.tweet_min_dev_sol`. `true` when the request went out.
    /// A create the tweet check fetches and the tweet feed matches: a tweet
    /// host's coin with a dev buy of at least `entry.tweet_min_dev_sol`.
    fn tweet_candidate(&self, hit: &CreateHit) -> bool {
        (hit.intx_sol_in >= self.cfg.tweet_min_dev || (self.cfg.hidden_dev_grpc && dev_hidden(hit)))
            && hit.tx_shape.uri_len != 0
            && uri_host_ok(&self.cfg.tweet_hosts, &[], hit.tx_shape.uri_host())
    }

    /// The tweet feed's call on a candidate create (`None`: no feed, or not
    /// a candidate), counted and journaled
    /// (`tweetfeed <mint8> <call> posts N post <id> age <ms>`).
    fn feed_call(&mut self, hit: &CreateHit) -> Option<Classified> {
        if self.feed_posts.is_none() || !self.tweet_candidate(hit) {
            return None;
        }
        let posts = self.feed_posts.as_ref()?.load();
        let c = crate::tweetfeed::classify(
            &posts,
            hit.tx_shape.sym_key(),
            hit.tx_shape.name_key(),
            crate::now_ms(),
            self.cfg.tweet_feed_window_ms,
        );
        self.counters.feed_calls[c.call as usize] += 1;
        let k8 = key8(&hit.mint);
        journal(format_args!(
            "tweetfeed {} {} posts {} post {} age {}ms",
            std::str::from_utf8(&k8).unwrap_or("?"),
            c.call.as_str(),
            c.matched,
            c.post,
            c.post_age_ms
        ));
        Some(c)
    }

    fn ask_tweet(&mut self, hit: &CreateHit, slot: u64, create_index: u32) -> bool {
        if self.tweet_req.is_none() || !self.tweet_candidate(hit) {
            return false;
        }
        let req = TweetReq {
            mint: hit.mint,
            slot,
            index: create_index,
            uri: hit.tx_shape.uri,
            uri_len: hit.tx_shape.uri_len,
            seen_ms: crate::now_ms(),
            push_ns: mono_ns(),
        };
        if self.tweet_req.as_mut().is_some_and(|r| r.push(req).is_ok()) {
            self.counters.tweet_asked += 1;
            true
        } else {
            self.counters.tweet_ring_full += 1;
            false
        }
    }

    /// A verdict that came before its create was parked, taken.
    fn take_early_verdict(&mut self, mint: &Pubkey) -> Option<TweetVerdict> {
        let i = self
            .tw_early
            .iter()
            .position(|v| v.is_some_and(|v| v.mint == *mint))?;
        self.tw_early[i].take()
    }

    /// A verdict from the tweet lane: counted and journaled; with the gate
    /// on it decides its park (fire on a pass once the run is complete,
    /// `skip_tweet_*` otherwise).
    fn on_tweet(&mut self, v: TweetVerdict) {
        let now = mono_ns();
        let us = now.saturating_sub(v.push_ns) / 1_000;
        self.hist_tweet.saturating_record(us.max(1));
        self.counters.tweet_kinds[v.kind as usize] += 1;
        let at = self.slot_now.saturating_sub(v.slot);
        self.counters.tweet_at[(at as usize).min(2)] += 1;
        let parked = self
            .parked
            .iter()
            .position(|x| x.is_some_and(|p| p.hit.mint == v.mint && p.tw == TW_WAIT));
        let fired = self.fired_or_held(&v.mint);
        let copy = matches!(
            v.kind,
            TweetKind::Copy | TweetKind::Stale | TweetKind::NoTweet
        );
        // "exit": a create still parked (its bundle run not complete) is
        // skipped on a copy verdict; it fires without waiting otherwise
        let exit_park = (self.cfg.tweet_gate == TweetGate::Exit && copy)
            .then(|| {
                self.parked
                    .iter()
                    .position(|x| x.is_some_and(|p| p.hit.mint == v.mint))
            })
            .flatten();
        if let Some(i) = exit_park {
            if let Some(p) = self.parked[i].take() {
                self.counters.tweet_skipped += 1;
                self.decision(p.intent, &p.hit, tweet_code(v.kind), 0, 0, 0, 0, 0);
            }
        }
        let what = match parked {
            Some(i) if v.kind == TweetKind::First => {
                let mut p = self.parked[i].take().expect("parked");
                p.tw = TW_PASS;
                if p.run_ready || self.slot_now > p.slot {
                    self.counters.tweet_fired += 1;
                    self.fire_parked(&p, now);
                    "fire"
                } else {
                    self.parked[i] = Some(p);
                    "pass, waiting for the run"
                }
            }
            Some(i) => {
                let p = self.parked[i].take().expect("parked");
                self.counters.tweet_skipped += 1;
                self.decision(p.intent, &p.hit, tweet_code(v.kind), 0, 0, 0, 0, 0);
                "skip"
            }
            None if exit_park.is_some() => "skip",
            None if fired && self.cfg.tweet_gate == TweetGate::Exit => {
                if copy && self.cut_hold(&v.mint) {
                    self.counters.tweet_cut += 1;
                    "ours, cut to this slot"
                } else {
                    "ours"
                }
            }
            None => {
                if matches!(self.cfg.tweet_gate, TweetGate::On | TweetGate::Exit) && !fired {
                    self.tw_early[self.tw_early_at % TW_EARLY] = Some(v);
                    self.tw_early_at = self.tw_early_at.wrapping_add(1);
                }
                if fired {
                    "ours"
                } else {
                    "-"
                }
            }
        };
        let k8 = key8(&v.mint);
        // an error says why after the decision ("(timeout)", "(http 404)")
        journal(format_args!(
            "tweet {} {} rank {} age {}s sid {} fetch {}ms verdict {}ms slot+{} {}{}{}",
            std::str::from_utf8(&k8).unwrap_or("?"),
            v.kind.as_str(),
            v.rank,
            v.age_s,
            v.sid,
            v.fetch_us / 1_000,
            us / 1_000,
            at,
            what,
            FailNote(v.fail, v.http),
            TriesNote(v.tries)
        ));
    }

    /// A position (open or pending) on `mint`.
    fn fired_or_held(&self, mint: &Pubkey) -> bool {
        self.find_mint(mint).is_some()
    }

    /// `entry.tweet_gate = "exit"`: the hold of the position on `mint` ends
    /// with the current slot (a pending buy sells once it opens): the exit
    /// engine's hold-to rule then sells it all at dump bounds at the slot
    /// close. `false` when there is no position or its hold already ends
    /// as soon.
    fn cut_hold(&mut self, mint: &Pubkey) -> bool {
        let slot_now = self.slot_now;
        let Some(s) = self.find_mint(mint).and_then(|i| self.slots[i].as_mut()) else {
            return false;
        };
        let base = if s.pos.create_slot > 0 {
            s.pos.create_slot
        } else {
            s.create_slot
        };
        let h = slot_now.saturating_sub(base).clamp(1, u16::MAX as u64) as u16;
        if s.pos.policy.hold_to_create_slot.is_some_and(|cur| cur <= h) {
            return false;
        }
        s.pos.policy.hold_to_create_slot = Some(h);
        true
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
            let Some((t, pre)) = self.prepark[k] else {
                continue;
            };
            if t.mint != mint || t.slot != slot {
                continue;
            }
            self.prepark[k] = None;
            if self.parked[i].is_none() || !self.parked_trade(&t, pre) {
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
            let Some(mut p) = self.parked[i] else {
                continue;
            };
            // the tweet check: a bundled park whose verdict has not come
            // waits up to entry.tweet_late_slots past its slot (the run ended
            // with the slot); one whose verdict passed fires at the run seen
            if p.tw != TW_OFF && p.slot < self.slot_now && p.bundle_buys > 0 && !p.outside {
                let late = self.slot_now > p.slot.saturating_add(self.cfg.tweet_late_slots)
                    || now.saturating_sub(p.parked_ns) > PARK_MAX_NS;
                if p.tw == TW_PASS && !late {
                    self.parked[i] = None;
                    self.fire_parked(&p, now);
                } else if late {
                    self.parked[i] = None;
                    self.counters.tweet_late += 1;
                    self.decision(p.intent, &p.hit, code::TWEET_LATE, 0, 0, 0, 0, 0);
                } else {
                    p.run_ready = true;
                    self.parked[i] = Some(p);
                }
                continue;
            }
            if p.slot < self.slot_now && p.bundle_buys > 0 {
                self.parked[i] = None;
                let c = if p.outside {
                    self.counters.not_first += 1;
                    code::NOT_FIRST
                } else {
                    code::LATE_SLOT
                };
                self.decision(p.intent, &p.hit, c, 0, 0, 0, 0, 0);
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
    fn parked_trade(&mut self, t: &TradeLite, pre_exec: bool) -> bool {
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
        if hidden_trade(&mut p, t, pre_exec).is_some() {
            self.parked[i] = Some(p);
            return true;
        }
        let now = mono_ns();
        match park_trade(&mut p, t, &self.owner, self.cfg.bundle_wait_ns, now) {
            ParkStep::Ignore => {}
            ParkStep::Hold => self.parked[i] = Some(p),
            ParkStep::Fire if p.tw == TW_WAIT => {
                // the run is complete: the fire waits for the tweet verdict
                p.run_ready = true;
                self.parked[i] = Some(p);
            }
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
    fn parked_run_done(&mut self, r: &RunDoneLite, pre_exec: bool) {
        self.counters.run_done += 1;
        let Some(i) = self
            .parked
            .iter()
            .position(|x| x.is_some_and(|p| p.hit.mint == r.mint && p.slot == r.slot))
        else {
            return;
        };
        // a hidden dev buy's run is the executed one: a shreds feed's end
        // of run follows trades this park dropped
        if pre_exec && self.parked[i].is_some_and(|p| p.dev_hidden) {
            return;
        }
        if self.parked[i].is_some_and(|p| p.bundle_buys == 0) {
            return;
        }
        if let Some(p) = self.parked[i].as_mut().filter(|p| p.tw == TW_WAIT) {
            p.run_ready = true;
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
                .is_some_and(|p| !p.run_ready && park_due(p, self.cfg.bundle_wait_ns, now))
            {
                if let Some(p) = self.parked[i].as_mut().filter(|p| p.tw == TW_WAIT) {
                    // the wait is over: the fire waits for the tweet verdict
                    p.run_ready = true;
                    continue;
                }
                if let Some(p) = self.parked[i].take() {
                    self.fire_parked(&p, now);
                }
            }
        }
    }

    /// Steps 5-11 for a parked create, bound at the bundle run's post-trade
    /// state; a run with an outside buy ahead of us is `skip_not_first`.
    fn fire_parked(&mut self, p: &Parked, now: u64) {
        if p.outside {
            self.counters.not_first += 1;
            return self.decision(p.intent, &p.hit, code::NOT_FIRST, 0, 0, 0, 0, 0);
        }
        // a hidden dev buy (entry.hidden_dev_buy = "grpc"): the template on
        // the create transaction's executed buy, unknown = skipped
        if p.dev_hidden {
            let ok = p.dev_seen && dev_buy_ok(&self.cfg.dev_buy, p.dev_sol);
            let k8 = key8(&p.hit.mint);
            journal(format_args!(
                "hiddendev {} dev {} {} host {}",
                std::str::from_utf8(&k8).unwrap_or("?"),
                if p.dev_seen { p.dev_sol as i64 } else { -1 },
                if ok { "fire" } else { "skip" },
                p.hit.tx_shape.uri_host().unwrap_or("-")
            ));
            if !ok {
                self.counters.hidden_dev[2] += 1;
                self.counters.skip_dev_buy += 1;
                return self.decision(p.intent, &p.hit, code::DEV_BUY, 0, 0, 0, 0, 0);
            }
            self.counters.hidden_dev[1] += 1;
        }
        self.counters.fire_wallets[(bundle_wallet_count(p) as usize).min(5)] += 1;
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
            p.create_index,
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
        create_index: u32,
    ) {
        let pb = parked.and_then(|p| p.pb);
        // 4c: the bundle shape (entry.bundle_wallets) at the fire point: the
        // park's distinct bundle wallets outside the creator set
        if let Some(p) = parked {
            if !bundle_shape_ok(&self.cfg.bundle_wallets, bundle_wallet_count(p)) {
                return self.decision(intent, hit, code::BUNDLE_SHAPE, 0, 0, 0, 0, 0);
            }
        }
        // 4d: the pick template's size bounds at the fire point, before any
        // risk or nonce: the post-bundle virtual SOL the buy would be bound
        // at (entry.max_post_bundle_sol; the run's anchor, else V0) and the
        // dev + bundle SOL (entry.max_bundle_sol)
        if self.cfg.max_post_bundle > 0 {
            let v = post_bundle_sol(hit, parked);
            if v > self.cfg.max_post_bundle {
                self.counters.skip_too_big += 1;
                template_line(
                    "skip_bundle_too_big post-bundle",
                    hit,
                    v,
                    self.cfg.max_post_bundle,
                );
                return self.decision(intent, hit, code::TOO_BIG, 0, 0, 0, 0, 0);
            }
        }
        if self.cfg.max_bundle > 0 {
            let s = dev_and_bundle_lamports(hit, parked);
            if s > self.cfg.max_bundle {
                self.counters.skip_bundle_sol += 1;
                template_line("skip_bundle_sol dev+bundle", hit, s, self.cfg.max_bundle);
                return self.decision(intent, hit, code::BUNDLE_SOL, 0, 0, 0, 0, 0);
            }
        }
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
        let Some(mut op) = arm.ops.get(&entry.op_id).copied() else {
            return self.decision(intent, hit, code::BAD_OP, 0, 0, 0, 0, 0);
        };
        // the operator's spend cap narrows the ArmSet size (2026-10-23: the
        // size is the one knob the box can turn without a new signed ArmSet;
        // a buy above the cap used to be skipped as skip_spend_cap)
        let cap = if self.cfg.mode == Mode::Probe {
            self.risk.caps().max_probe_lamports
        } else {
            self.risk.caps().max_snipe_lamports
        };
        if cap > 0 && op.size > cap {
            op.size = cap;
        }
        // the operator's confidence tier (docs/09 "stake tier"): the copied
        // wallet's stake level on this signer (an unlisted signer is tier 0)
        // picks the size and, below, the bid; the size only narrows, so the
        // signed ArmSet and the risk cap stay the upper bound
        let tier = self
            .cfg
            .operator_rule(hit)
            .map_or(0usize, |r| r.stake_tier.min(2) as usize);
        if let Some(t) = self.cfg.stake_by_tier {
            if t[tier] > 0 && op.size > t[tier] {
                op.size = t[tier];
            }
        }
        // 5b: the operator-dump skip (entry.dumper_skip): the dev, the
        // creator or a bundle wallet dumped an earlier launch; the skip
        // only on entry.dumper_skip_hosts when that is set
        if self.cfg.dumper_skip != rugsnipe_core::config::DumperSkip::Off {
            let d = self.dumpers.load();
            if !d.is_empty() {
                let dumper = d.contains(&hit.user)
                    || d.contains(&hit.creator_arg)
                    || parked.is_some_and(|p| {
                        p.wallets[..p.n_wallets as usize]
                            .iter()
                            .any(|w| d.contains(w))
                    });
                if dumper {
                    self.counters.dumper_hits += 1;
                    let k8 = key8(&hit.mint);
                    let host = hit.tx_shape.uri_host();
                    let skip = self.cfg.dumper_skip == rugsnipe_core::config::DumperSkip::Skip;
                    let listed = uri_host_ok(&self.cfg.dumper_skip_hosts, &[], host);
                    journal(format_args!(
                        "dumper {} {} host {}",
                        std::str::from_utf8(&k8).unwrap_or("?"),
                        match (skip, listed) {
                            (true, true) => "skipped",
                            (true, false) => "logged (other host)",
                            (false, _) => "logged (would skip)",
                        },
                        host.unwrap_or("-")
                    ));
                    if skip && listed {
                        return self.decision(
                            intent,
                            hit,
                            code::DUMPER,
                            0,
                            op.size,
                            op.v_max,
                            0,
                            0,
                        );
                    }
                }
            }
        }
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
        // 6b: the pre-send entry gate (entry.presend_mode), before the risk
        // gate so that every create reaching the decision is scored (log)
        // and a skipped one never reserves risk or a nonce: the create
        // scored on what is known now (the run seen so far, the create's
        // wire bytes, the signer's outcomes, the time); `skip` below the
        // cut sends nothing
        if self.presend_gate(intent, hit, slot, create_index, rates, parked, now_ms) {
            return self.decision(intent, hit, code::PRESEND, 0, op.size, op.v_max, 0, 0);
        }
        // 7: risk
        let (v0, t0_tok) = v0_of(hit);
        // the reserve we buy at: the bundle run's post-trade state when the
        // gate observed it (the create's state otherwise; the bound below
        // then adds the expected bundle)
        let v_entry = pb.map_or(v0, |(v, _)| v);
        let mut bid = self
            .cfg
            .bid_by_tier
            .map_or(self.cfg.bid_lamports, |t| {
                if t[tier] > 0 {
                    t[tier]
                } else {
                    self.cfg.bid_lamports
                }
            })
            .min(self.cfg.max_bid);
        // the bid A/B (bid.ab_by_tier): the mints whose first byte is odd bid
        // the second arm (sends.bid records which one each race carried); not
        // the last byte, whose low 4 bits the "pump" suffix fixes on every
        // pump.fun mint
        if let Some(t) = self.cfg.bid_ab_by_tier {
            if t[tier] > 0 && hit.mint.as_ref()[0] & 1 == 1 {
                bid = t[tier].min(self.cfg.max_bid);
            }
        }
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
        // The tweet check's pass may come after the create slot: the buy
        // stays bound at the post-bundle state, so it lands only if nobody
        // bought behind the bundle in the meantime.
        let late_ok = if parked.is_some_and(|p| p.tw == TW_PASS) {
            self.cfg.tweet_late_slots
        } else {
            0
        };
        if real && self.slot_now > slot.saturating_add(late_ok) {
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
        // 9b: no way to sell what this buy would hold (no fresh blockhash
        // for more than risk.max_blockhash_age_ms, no free sell nonce)
        if real && self.sell_blocked {
            self.risk.release_unfilled();
            return self.decision(intent, hit, code::NO_BLOCKHASH, 0, op.size, op.v_max, 0, 0);
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
        let min_out = match pb {
            // a zero premium binds at the exact post-bundle state the park
            // observed (the quote before our own trade): any buy ahead of
            // ours fails the buy on chain, cheaply (docs/09)
            Some((v, t)) if self.cfg.max_premium_bps == 0 => {
                min_tokens_out_at(spend, v, t, hit.creator_arg, self.cfg.fees)
            }
            _ => min_tokens_out(spend, v_cap, hit.creator_arg, self.cfg.fees),
        };
        let min_out_e2 = min_tokens_out(spend, op.v_max, hit.creator_arg, self.cfg.fees);
        // 12: patch (keys and the per-mint CU limit were resolved before the claim)
        // per-class bid: the tip (after the buy, reverts on failure) carries
        // the bid where the leader orders by it; the priority fee stays at the
        // floor there and is clamped by bid.failed_attempt_lamports_max elsewhere
        let n_sigs = self.tmpl.buys[claim.idx].table.n_sigs.clamp(1, 2) as u8;
        // the leaders of the create slot and the next one (schedule); a late
        // tweet-check fire bids for the leaders of the current slot and the next
        let (lc0, lc1) = self.leader_classes(slot.max(self.slot_now));
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
                bundle_prio: self.cfg.bid_policy.bundle_priority(plan0.class, bid),
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
                bundle_prio: self.cfg.bid_policy.bundle_priority(plan1.class, bid),
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
        // the exit: the operator table's hold for a listed signer (docs/09),
        // counted from the create slot
        let rule = self.cfg.operator_rule(hit);
        let pos = Position::pending(
            hit.mint,
            self.owner,
            intent,
            self.cfg.policy_for(rule),
            bundle,
            hit.creator_arg,
            self.cfg.fees,
            pump_codec::curve::TOKEN_TOTAL_SUPPLY,
        )
        .with_create_slot(slot);
        if let Some(r) = rule {
            let k8 = key8(&hit.mint);
            journal(format_args!(
                "operator {} hold_to create+{} (n {}, bundle_wallets {}, tier {})",
                std::str::from_utf8(&k8).unwrap_or("?"),
                r.hold_slots,
                r.n,
                r.bundle_wallets,
                r.stake_tier
            ));
        }
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
            sell_claim: None,
            sell_claim_sent: false,
            unsold_since_ms: 0,
            handed_off: false,
            first_sell_ms: 0,
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
            // the first slots of a bundled launch we do not hold: the
            // operator-dump tracker's input (cold thread)
            self.send_cold(ColdMsg::MintTrade(t));
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
            HotIn::SellUnsent { intent, idx } => {
                let Some(i) = self.find_intent(intent) else {
                    return;
                };
                self.counters.sells_unsent += 1;
                // the pending order whose attempt this was (try number in the variant index)
                let try_no = u32::from(idx & 0x7f);
                let order = self.slots[i].as_ref().and_then(|s| {
                    s.pos
                        .pending
                        .iter()
                        .flatten()
                        .find(|p| p.order.try_no.min(127) == try_no && !p.unsent)
                        .map(|p| p.order.order_id)
                });
                if let Some(order_id) = order {
                    let now = Now {
                        ms: crate::now_ms(),
                        slot: self.slot_now,
                    };
                    self.sell_unsent(i, order_id, now);
                }
            }
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
        self.adopt_known(mint, tokens, intent, None)
    }

    /// [`HotLoop::adopt`] with what the store knew about the position
    /// (recovery's open row for the mint): its cost, opened slot, fill price,
    /// peak and bundle are kept and only the token amount comes from the
    /// wallet (2026-10-23: two 0.5 SOL positions came back as cost 0 /
    /// opened_slot 0 and the guard's proceeds counted as pure profit).
    pub fn adopt_known(
        &mut self,
        mint: Pubkey,
        tokens: u64,
        intent: u64,
        known: Option<Known>,
    ) -> bool {
        if !self.adopt_inner(mint, tokens, intent) {
            return false;
        }
        let (Some(k), Some(i)) = (known, self.find_mint(&mint)) else {
            return true;
        };
        let Some(s) = self.slots[i].as_mut() else {
            return true;
        };
        let p = &mut s.pos;
        p.cost = k.cost;
        if k.opened_slot > 0 {
            p.opened_slot = k.opened_slot;
        }
        if k.v_fill > 0 {
            p.v_fill = k.v_fill;
            p.t_fill = (pump_codec::curve::K_INITIAL / k.v_fill as u128) as u64;
        }
        p.peak_v = p.peak_v.max(k.peak_v).max(p.v_fill);
        p.bundle = k.bundle;
        if k.from_intent {
            p.origin = Some(rugsnipe_exit::Origin::Intent);
        }
        let snap = *p;
        self.send_cold(ColdMsg::Position(snap));
        true
    }

    fn adopt_inner(&mut self, mint: Pubkey, tokens: u64, intent: u64) -> bool {
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
            sell_claim: None,
            sell_claim_sent: false,
            unsold_since_ms: 0,
            handed_off: false,
            first_sell_ms: 0,
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
        if self.cfg.mode.is_real() {
            self.blockhash_watch(now.ms);
        }
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
        if self.cfg.mode.is_real() {
            self.handoff_stuck_sells(now);
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
                self.sell_nonce_done(i);
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

    /// The lifetime of a real curve sell on a sell-pool nonce (a final sell
    /// first, any other when the lane's blockhash is not fresh): the
    /// position's claim, made on first need and kept for every retry so at
    /// most one of them lands. `None`: no free nonce or no nonce templates.
    /// Lock-free, no allocation.
    fn sell_nonce(&mut self, i: usize) -> Option<(usize, [u8; 32])> {
        let pool = self.shared.sell_nonces.get()?.clone();
        if pool.is_empty() || self.sell_tmpls.len() < pool.len() {
            return None;
        }
        let s = self.slots[i].as_mut()?;
        if let Some(c) = s.sell_claim {
            if pool.holds(&c, s.intent) {
                return Some((c.idx, c.value));
            }
            // advanced (a sell on it landed, or the keeper burned it)
            s.sell_claim = None;
            s.sell_claim_sent = false;
        }
        let c = pool.try_claim_any(s.intent, self.slot_now).ok()?;
        s.sell_claim = Some(c);
        s.sell_claim_sent = false;
        let intent = s.intent;
        let k8 = key8(&s.pos.mint);
        journal(format_args!(
            "sell {} rides sell nonce {}",
            std::str::from_utf8(&k8).unwrap_or("?"),
            c.idx
        ));
        self.send_cold(ColdMsg::SellNonce {
            idx: c.idx as u16,
            intent,
            on: true,
        });
        Some((c.idx, c.value))
    }

    /// The position is done with its sell-pool nonce (closed, or handed
    /// over): an unsent claim is given back, a sent one goes to the keeper
    /// (burned unless its sell already advanced it).
    fn sell_nonce_done(&mut self, i: usize) {
        let Some(s) = self.slots[i].as_mut() else {
            return;
        };
        let Some(c) = s.sell_claim.take() else {
            return;
        };
        let (sent, intent) = (s.sell_claim_sent, s.intent);
        if !sent {
            if let Some(p) = self.shared.sell_nonces.get() {
                p.release_unsent(&c);
            }
        }
        self.send_cold(ColdMsg::SellNonce {
            idx: c.idx as u16,
            intent,
            on: false,
        });
    }

    /// A sell of position `i` could not go out: the exit engine takes the
    /// attempt back (retried next slot, no try used), and after
    /// [`HANDOFF_MS`] without a sell the position is handed to the guard.
    fn sell_unsent(&mut self, i: usize, order_id: u32, now: Now) {
        let Some(s) = self.slots[i].as_mut() else {
            return;
        };
        rugsnipe_exit::unsent(&mut s.pos, order_id, now.slot);
        if s.unsold_since_ms == 0 {
            s.unsold_since_ms = now.ms.max(1);
            return;
        }
        let unsold_ms = now.ms.saturating_sub(s.unsold_since_ms);
        if unsold_ms < HANDOFF_MS || s.handed_off {
            return;
        }
        self.handoff(
            i,
            unsold_ms,
            "no sell sent: no lifetime, or no sender accepted it",
        );
    }

    /// Hand position `i` to the guard once: the journal line, the
    /// `guard_handoff:<mint>` row and the CRITICAL alert (cold thread). The
    /// guard sells the mint even while this process is alive; the hot loop
    /// keeps retrying every slot (a final sell lands at most once).
    fn handoff(&mut self, i: usize, unsold_ms: u64, why: &'static str) {
        let Some(s) = self.slots[i].as_mut() else {
            return;
        };
        if s.handed_off {
            return;
        }
        s.handed_off = true;
        let (mint, intent) = (s.pos.mint, s.intent);
        let k8 = key8(&mint);
        journal(format_args!(
            "handoff {} to guard: {why} for {unsold_ms} ms; retrying every slot",
            std::str::from_utf8(&k8).unwrap_or("?")
        ));
        self.send_cold(ColdMsg::GuardHandoff {
            mint,
            intent,
            unsold_ms,
            why,
        });
    }

    /// Real positions whose sell has not landed [`HANDOFF_MS`] after its
    /// first try (any reason: tries spent on failing senders, sent but
    /// dropped, never sent) go to the guard ([`Self::handoff`]).
    fn handoff_stuck_sells(&mut self, now: Now) {
        for i in 0..MAX_POS {
            let Some(s) = self.slots[i].as_mut() else {
                continue;
            };
            if !(s.real && s.filled) {
                continue;
            }
            if !s.pos.has_pending() {
                s.first_sell_ms = 0;
                continue;
            }
            if s.first_sell_ms == 0 || s.handed_off {
                continue;
            }
            let ms = now.ms.saturating_sub(s.first_sell_ms);
            if ms >= HANDOFF_MS {
                self.handoff(i, ms, "sell not landed after its first try");
            }
        }
    }

    fn sell(&mut self, i: usize, order: SellOrder, now: Now) {
        self.counters.sells += 1;
        self.stats.sells.inc();
        let owned = self.shared.exits_owned.load(Acquire);
        // a real curve sell's lifetime, first choice first (2026-10-23: the
        // lane's blockhash went stale and two positions sat unsold for 17
        // minutes): 1. the lane's fresh blockhash; 2. a sell-pool durable
        // nonce (this position's claim); 3. none here: the job goes to the
        // sender thread marked NO_LIFETIME and the sender fetches a recent
        // blockhash (300 ms budget, off the hot thread) before signing;
        // 4. no resolver either: not sent, the attempt is taken back
        // (rugsnipe_exit::unsent) and retried on the next slot.
        // A final sell (whole balance, token account closed) swaps 1 and 2:
        // it fans out as one variant per sender (sender::sell_fans_out), and
        // on a recent blockhash every copy lands, the first sells the
        // balance, the others fail on the closed account and each pays its
        // fee (2026-10-24: three failed copies per exit, 3.4M lamports each
        // on a priority-ordered leader, about 1.6% of a 0.25 SOL stake on
        // average). On the position's sell nonce at most one copy (and one
        // try) executes, as with the buys' nonce.
        let real_sell = self.cfg.mode.is_real()
            && self.slots[i]
                .as_ref()
                .is_some_and(|s| s.real && order.venue == Venue::Curve);
        let mut nonce_k: Option<usize> = None;
        let nonce_first = order.close_account;
        let lifetime = if !real_sell {
            Some(PAPER_BLOCKHASH)
        } else if let Some((k, v)) = (owned && nonce_first).then(|| self.sell_nonce(i)).flatten() {
            nonce_k = Some(k);
            Some(v)
        } else if let Some(h) = self.shared.fresh_blockhash() {
            Some(h)
        } else if let Some((k, v)) = (owned && !nonce_first)
            .then(|| self.sell_nonce(i))
            .flatten()
        {
            nonce_k = Some(k);
            Some(v)
        } else if self.cfg.sell_resolver {
            Some(NO_LIFETIME)
        } else {
            None
        };
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
        if !paper && owned && s.first_sell_ms == 0 {
            s.first_sell_ms = now.ms.max(1);
        }
        let mut sig = [0u8; 64];
        let mut unsent = false;
        if !owned {
            self.stats.sells_not_owner.inc();
        } else if lifetime.is_none() && order.venue == Venue::Curve {
            self.counters.sells_no_blockhash += 1;
            self.rslog.push(Rec::Risk {
                kind: 0x43,
                value: order.order_id as i64,
            });
            unsent = true;
        } else if order.venue == Venue::Curve {
            let lifetime = lifetime.unwrap_or(PAPER_BLOCKHASH);
            if !paper && lifetime == NO_LIFETIME {
                self.counters.sells_resolve += 1;
            } else if !paper {
                s.unsold_since_ms = 0;
            }
            let t = match (nonce_k, order.close_account) {
                (Some(k), true) => &mut self.sell_tmpls[k].0,
                (Some(k), false) => &mut self.sell_tmpls[k].1,
                (None, true) => &mut self.tmpl.sell_final,
                (None, false) => &mut self.tmpl.sell_partial,
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
                    bundle_prio: 0,
                };
                if self.fire.push(job).is_err() {
                    self.counters.ring_full += 1;
                    unsent = true;
                } else if nonce_k.is_some() {
                    s.sell_claim_sent = true;
                    self.counters.sells_on_nonce += 1;
                }
            }
        }
        if unsent {
            self.sell_unsent(i, order.order_id, now);
            return;
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

    /// Real positions with sell orders out, for the shutdown's booking
    /// ([`crate::inflight`]): a sell sent before the stop may land after the
    /// loop stopped reading its fill.
    pub fn inflight_sells(&self) -> Vec<crate::inflight::InflightSell> {
        self.slots
            .iter()
            .flatten()
            .filter(|s| s.real && s.filled)
            .filter_map(|s| {
                let p = &s.pos;
                let orders = || p.pending.iter().flatten().map(|o| o.order);
                let first = orders().min_by_key(|o| o.order_id)?;
                let tokens = orders()
                    .map(|o| o.tokens)
                    .fold(0u64, u64::saturating_add)
                    .min(p.tokens);
                Some(crate::inflight::InflightSell {
                    mint: p.mint,
                    reason: first.reason.label(),
                    venue: match first.venue {
                        Venue::Curve => "curve",
                        Venue::PumpSwap => "pumpswap",
                    },
                    tokens,
                    closes: tokens >= p.tokens,
                    try_no: first.try_no,
                })
            })
            .collect()
    }

    /// The position held or pending on `mint` (reports and tests; not on
    /// the hot path).
    pub fn position_of(&self, mint: &Pubkey) -> Option<Position> {
        self.slots
            .iter()
            .flatten()
            .find(|s| s.pos.mint == *mint)
            .map(|s| s.pos)
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
    #[test]
    fn dev_buy_whitelist_matches_within_the_tolerance_only() {
        let r = dev_buy_ranges(&[5.0, 2.96], 0.005);
        assert_eq!(
            r,
            vec![
                (4_995_000_000, 5_005_000_000),
                (2_955_000_000, 2_965_000_000)
            ]
        );
        assert!(dev_buy_ok(&r, 5_000_000_000));
        assert!(dev_buy_ok(&r, 2_960_000_000));
        assert!(dev_buy_ok(&r, 5_004_000_000));
        assert!(!dev_buy_ok(&r, 5_010_000_000));
        assert!(!dev_buy_ok(&r, 3_000_000_000));
        assert!(!dev_buy_ok(&r, 0));
        assert!(dev_buy_ok(&[], 123));
    }

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
            outside: false,
            run_sol: 0,
            run: RunTrades::default(),
            tw: TW_OFF,
            run_ready: false,
            dev_hidden: false,
            dev_sol: 0,
            dev_seen: false,
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
        // a copy of +2 arriving late never regresses the state (nor counts twice)
        let s = park_trade(
            &mut p,
            &trade_v(h.mint, w1, true, ci + 2, 33_000_000_000),
            &owner,
            WAIT,
            6_500,
        );
        assert_eq!(s, ParkStep::Hold);
        assert_eq!((p.bundle_buys, p.pb), (2, Some((34_000_000_000, 1))));
        // a bundle wallet's sell at +4 is the curve we buy on
        let s = park_trade(
            &mut p,
            &trade_v(h.mint, w1, false, ci + 4, 33_500_000_000),
            &owner,
            WAIT,
            7_000,
        );
        assert_eq!(s, ParkStep::Hold);
        assert_eq!((p.bundle_buys, p.pb), (2, Some((33_500_000_000, 1))));
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
        // (three qualifying buys: +2, +3, +5; the late copy of +2 is the
        // same trade), and the dev + bundle SOL: the dev's 1 SOL in the
        // create, its 0.5 at +1 and three bundle buys of 0.5
        assert_eq!(
            (&p.wallets[..p.n_wallets as usize], p.bundle_buys),
            (&[w1, w2][..], 3)
        );
        assert!(!p.outside);
        assert_eq!(p.run_sol, 2_000_000_000);
        assert_eq!(dev_and_bundle_lamports(&h, Some(&p)), 3_000_000_000);
        assert_eq!(dev_and_bundle_lamports(&h, None), 1_000_000_000);
        assert_eq!(post_bundle_sol(&h, Some(&p)), 35_000_000_000);
        assert_eq!(
            post_bundle_sol(&h, None),
            31_000_000_000,
            "V0 without a run"
        );
    }

    /// The post-bundle anchor moves only on the creator set's and the
    /// contiguous bundle's trades (2026-10-23, Sj3qBo: an outside 1.78 SOL
    /// buy inside the run re-anchored the bound, +11.4% over the
    /// post-bundle spot at max_premium = 0): a buy after a gap of 3 is an
    /// outside buyer ahead of us, the anchor stays at the bundle's last
    /// trade and the park is `outside` (the fire is `skip_not_first`).
    #[test]
    fn an_outside_buy_never_moves_the_anchor_and_marks_the_park() {
        let (owner, dev, creator, w1, w2, x) = (
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        );
        let h = hit(dev, creator);
        let ci = 10;
        let mut p = parked(&h, ci);
        for (u, i, v) in [(w1, ci + 1, 33_000_000_000), (w2, ci + 2, 34_000_000_000)] {
            park_trade(&mut p, &trade_v(h.mint, u, true, i, v), &owner, WAIT, 1_000);
        }
        assert_eq!(
            (p.pb, p.bundle_buys, p.outside),
            (Some((34_000_000_000, 1)), 2, false)
        );
        // the outside buy at +5 (a gap of 3 behind +2): the run's last
        // index, the park fires there and the fire refuses it
        let s = park_trade(
            &mut p,
            &trade_v(h.mint, x, true, ci + 5, 36_000_000_000),
            &owner,
            WAIT,
            2_000,
        );
        assert_eq!(s, ParkStep::Fire);
        assert!(p.outside);
        assert_eq!(p.pb, Some((34_000_000_000, 1)), "the post-bundle state");
        assert_eq!(
            (&p.wallets[..p.n_wallets as usize], p.bundle_buys),
            (&[w1, w2][..], 2)
        );
        assert_eq!(
            p.run_sol, 1_000_000_000,
            "the outside buy is not bundle SOL"
        );
        // a trade of a bundle wallet behind the outside buy moves nothing
        let mut q = parked(&h, ci);
        for (u, buy, i, v) in [
            (w1, true, ci + 1, 33_000_000_000),
            (x, true, ci + 4, 36_000_000_000),
            (w1, false, ci + 5, 35_000_000_000),
        ] {
            park_trade(&mut q, &trade_v(h.mint, u, buy, i, v), &owner, WAIT, 1_000);
        }
        assert!(q.outside);
        assert_eq!(q.pb, Some((33_000_000_000, 1)));
        // delivered out of order: the outside-looking buy at +4 before the
        // bundle's +2 is chained once +2 arrives (decided at the fire)
        let mut r = parked(&h, ci);
        park_trade(
            &mut r,
            &trade_v(h.mint, w1, true, ci + 1, 33_000_000_000),
            &owner,
            WAIT,
            1_000,
        );
        park_trade(
            &mut r,
            &trade_v(h.mint, w2, true, ci + 4, 35_000_000_000),
            &owner,
            WAIT,
            1_100,
        );
        assert!(r.outside && r.pb == Some((33_000_000_000, 1)));
        park_trade(
            &mut r,
            &trade_v(h.mint, w1, true, ci + 2, 34_000_000_000),
            &owner,
            WAIT,
            1_200,
        );
        assert!(!r.outside);
        assert_eq!((r.pb, r.bundle_buys), (Some((35_000_000_000, 1)), 3));
        // the creator set is never outside (the dev buying again after a
        // gap), a sell by an unknown wallet moves nothing, ours are ignored
        let mut u = parked(&h, ci);
        for (k, buy, i, v) in [
            (w1, true, ci + 1, 33_000_000_000),
            (dev, true, ci + 4, 34_000_000_000),
            (x, false, ci + 5, 33_900_000_000),
        ] {
            park_trade(&mut u, &trade_v(h.mint, k, buy, i, v), &owner, WAIT, 1_000);
        }
        assert!(!u.outside);
        assert_eq!((u.pb, u.pb_index), (Some((34_000_000_000, 1)), ci + 4));
        let mut o = parked(&h, ci);
        park_trade(
            &mut o,
            &trade_v(h.mint, w1, true, ci + 1, 33_000_000_000),
            &owner,
            WAIT,
            1_000,
        );
        park_trade(
            &mut o,
            &trade_v(h.mint, owner, true, ci + 5, 39_000_000_000),
            &owner,
            WAIT,
            1_000,
        );
        assert_eq!((o.outside, o.pb), (false, Some((33_000_000_000, 1))));
        // the limit of an index-only rule: an outside buy chained to the
        // bundle (gap <= 2) reads as a bundle wallet (rt.json's n_bw does
        // the same); entry.bundle_wallets / max_bundle_sol then see it
        let mut c = parked(&h, ci);
        park_trade(
            &mut c,
            &trade_v(h.mint, w1, true, ci + 1, 33_000_000_000),
            &owner,
            WAIT,
            1_000,
        );
        park_trade(
            &mut c,
            &trade_v(h.mint, x, true, ci + 3, 36_000_000_000),
            &owner,
            WAIT,
            1_000,
        );
        assert_eq!((c.outside, c.n_wallets), (false, 2));
    }

    #[test]
    fn uri_hosts_match_on_a_label_boundary_and_round_dev_buys_are_whole_sol() {
        let al = host_patterns(&["UXENTO.io".into(), ".j7tracker.io".into(), " gg.sc ".into()]);
        assert_eq!(al, vec!["uxento.io", "j7tracker.io", "gg.sc"]);
        assert!(host_matches("meta.uxento.io", "uxento.io"));
        assert!(host_matches("uxento.io", "uxento.io"));
        assert!(host_matches("META.UXENTO.IO", "uxento.io"));
        assert!(host_matches("meta.uxento.io:443", "uxento.io"));
        assert!(!host_matches("xuxento.io", "uxento.io"));
        assert!(!host_matches("uxento.io.evil", "uxento.io"));
        assert!(!host_matches("io", "uxento.io"));
        assert!(!host_matches("uxento.io", ""));
        let deny = host_patterns(&["ipfs.io".into()]);
        assert!(uri_host_ok(&al, &[], Some("metadata.j7tracker.io")));
        assert!(!uri_host_ok(&al, &[], Some("ipfs.io")));
        assert!(!uri_host_ok(&al, &[], None), "no host fails an allow list");
        assert!(uri_host_ok(&[], &deny, None), "and passes a deny list");
        assert!(!uri_host_ok(&[], &deny, Some("ipfs.io")));
        assert!(uri_host_ok(&[], &deny, Some("pump.mypinata.cloud")));
        assert!(!uri_host_ok(
            &al,
            &host_patterns(&["meta.uxento.io".into()]),
            Some("meta.uxento.io")
        ));
        assert!(uri_host_ok(&[], &[], None));
        // 1..=6 whole SOL +- 0.005
        let r = dev_buy_round_ranges(1.0, 6.0, 0.005);
        assert_eq!(r.len(), 6);
        for (lam, ok) in [
            (1_000_000_000u64, true),
            (995_000_000, true),
            (994_999_999, false),
            (1_005_000_000, true),
            (1_005_000_001, false),
            (3_000_000_000, true),
            (2_500_000_000, false),
            (6_005_000_000, true),
            (7_000_000_000, false),
            (500_000_000, false),
            (0, false),
        ] {
            assert_eq!(dev_buy_ok(&r, lam), ok, "{lam}");
        }
        assert_eq!(dev_buy_round_ranges(1.5, 1.9, 0.005), vec![]);
        assert_eq!(
            dev_buy_round_ranges(1.5, 2.0, 0.0),
            vec![(2_000_000_000, 2_000_000_000)]
        );
        assert_eq!(sol_lamports(47.5), 47_500_000_000);
        assert_eq!(sol_lamports(-1.0), 0);
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
        assert!(q.behind);
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

    // ------------------------------------------------------------- pre-send gate

    use crate::filter::{
        bundle_features, create_trade, create_tx_features, create_tx_features_wire, create_uri,
        funding_features, op_snapshot, time_features, Funding, OutcomeStore, BUNDLE, CREATE_TX,
        FUNDING, N_BUNDLE, N_CREATE, N_FUNDING, N_OPERATOR, N_TIME, OPERATOR, TIME,
    };
    use pump_codec::events::InnerInstruction;
    use pump_codec::tx::{TxVersion, TxView};
    use serde_json::{json, Value};

    /// A pump-codec transaction fixture (`getTransaction` base64 + meta).
    struct Fx {
        raw: Vec<u8>,
        result: Value,
        inner: Vec<(u8, u8, Vec<u8>, Vec<u8>)>,
        loaded: Vec<Pubkey>,
    }

    impl Fx {
        fn load(name: &str) -> Self {
            use base64::Engine as _;
            let p = format!(
                "{}/../pump-codec/fixtures/tx/{name}",
                env!("CARGO_MANIFEST_DIR")
            );
            let v: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
            let result = v["result"].clone();
            let raw = base64::engine::general_purpose::STANDARD
                .decode(result["transaction"][0].as_str().unwrap())
                .unwrap();
            let meta = &result["meta"];
            let mut inner = vec![];
            for g in meta["innerInstructions"].as_array().into_iter().flatten() {
                for ix in g["instructions"].as_array().unwrap() {
                    inner.push((
                        g["index"].as_u64().unwrap() as u8,
                        ix["programIdIndex"].as_u64().unwrap() as u8,
                        ix["accounts"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|a| a.as_u64().unwrap() as u8)
                            .collect(),
                        bs58::decode(ix["data"].as_str().unwrap())
                            .into_vec()
                            .unwrap(),
                    ));
                }
            }
            let mut loaded = vec![];
            if let Some(la) = meta.get("loadedAddresses").filter(|x| !x.is_null()) {
                for k in ["writable", "readonly"] {
                    for a in la[k].as_array().unwrap() {
                        loaded.push(a.as_str().unwrap().parse().unwrap());
                    }
                }
            }
            Self {
                raw,
                result,
                inner,
                loaded,
            }
        }
        fn inner(&self) -> Vec<InnerInstruction<'_>> {
            self.inner
                .iter()
                .map(|(o, p, a, d)| InnerInstruction {
                    outer_index: *o,
                    program_id_index: *p,
                    accounts: a,
                    data: d,
                })
                .collect()
        }
        fn loaded(&self) -> Option<&[Pubkey]> {
            (!self.loaded.is_empty()).then_some(&self.loaded[..])
        }
        /// The create as the hot loop decodes it from the gRPC copy (wire
        /// bytes + the meta's inner instructions and loaded addresses).
        fn hit(&self) -> Option<CreateHit> {
            let tx = TxView::parse(&self.raw).unwrap();
            pump_codec::create_hit(&tx, self.loaded(), &self.inner(), FeeBps::DEFAULT)
                .ok()
                .flatten()
        }
        /// The jsonParsed `getTransaction` result `create_tx_features`
        /// reads, built from the wire parse (System transfers parsed, every
        /// other instruction raw; fee and logs from the fixture's meta).
        fn json_parsed(&self) -> Value {
            let tx = TxView::parse(&self.raw).unwrap();
            let keys: Vec<Pubkey> = tx
                .static_account_keys()
                .iter()
                .chain(&self.loaded)
                .copied()
                .collect();
            let ix_json = |prog: Pubkey, accts: &[u8], data: &[u8]| -> Value {
                let a: Vec<Pubkey> = accts.iter().map(|i| keys[*i as usize]).collect();
                let u64_at = |o: usize| u64::from_le_bytes(data[o..o + 8].try_into().unwrap());
                if prog == pump_codec::constants::SYSTEM_PROGRAM_ID && data.len() >= 12 {
                    let tag = u32::from_le_bytes(data[..4].try_into().unwrap());
                    let parsed = match tag {
                        0 => Some(
                            json!({"type": "createAccount", "info": {"source": a[0].to_string(),
                            "newAccount": a[1].to_string(), "lamports": u64_at(4)}}),
                        ),
                        2 => Some(
                            json!({"type": "transfer", "info": {"source": a[0].to_string(),
                            "destination": a[1].to_string(), "lamports": u64_at(4)}}),
                        ),
                        3 => {
                            let n = u64_at(36) as usize;
                            Some(
                                json!({"type": "createAccountWithSeed", "info": {"source": a[0].to_string(),
                                "newAccount": a[1].to_string(), "lamports": u64_at(44 + n)}}),
                            )
                        }
                        11 => Some(
                            json!({"type": "transferWithSeed", "info": {"source": a[0].to_string(),
                            "destination": a[2].to_string(), "lamports": u64_at(4)}}),
                        ),
                        _ => None,
                    };
                    if let Some(p) = parsed {
                        return json!({"program": "system", "programId": prog.to_string(), "parsed": p});
                    }
                }
                json!({"programId": prog.to_string(),
                    "accounts": a.iter().map(|k| k.to_string()).collect::<Vec<_>>(),
                    "data": bs58::encode(data).into_string()})
            };
            let nreq = tx.header().num_required_signatures as usize;
            let account_keys: Vec<Value> = keys
                .iter()
                .enumerate()
                .map(|(i, k)| json!({"pubkey": k.to_string(), "signer": i < nreq}))
                .collect();
            let instructions: Vec<Value> = tx
                .instructions()
                .map(|ix| ix_json(*tx.program_id(&ix), ix.accounts, ix.data))
                .collect();
            let mut groups: Vec<(u8, Vec<Value>)> = vec![];
            for (o, p, a, d) in &self.inner {
                let v = ix_json(keys[*p as usize], a, d);
                match groups.last_mut() {
                    Some((g, list)) if *g == *o => list.push(v),
                    _ => groups.push((*o, vec![v])),
                }
            }
            let sigs: Vec<String> = (0..tx.num_signatures())
                .map(|i| bs58::encode(tx.signature(i).unwrap()).into_string())
                .collect();
            json!({
                "version": match tx.version() {
                    TxVersion::Legacy => json!("legacy"),
                    TxVersion::V0 => json!(0),
                    TxVersion::V1 => json!(1),
                },
                "meta": {
                    "fee": self.result["meta"]["fee"],
                    "logMessages": self.result["meta"]["logMessages"],
                    "innerInstructions": groups.iter().map(|(g, l)| json!({"index": g, "instructions": l})).collect::<Vec<_>>(),
                },
                "transaction": {"signatures": sigs, "message": {
                    "accountKeys": account_keys, "instructions": instructions}},
            })
        }
    }

    fn model() -> Model {
        Model::load(&crate::filter::resolve_path(
            "data/reports/strategy/entry_filter_v1.json",
        ))
        .unwrap()
    }

    /// `create_tx_features_wire` (the hot loop's decode) equals
    /// `create_tx_features` (the jsonParsed path) on every create fixture
    /// the decoder resolves, all 21 features (the uri block where the logs
    /// carry the CreateEvent).
    #[test]
    fn wire_create_features_match_the_json_path_on_the_fixtures() {
        let m = model();
        let dir = format!("{}/../pump-codec/fixtures/tx", env!("CARGO_MANIFEST_DIR"));
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        let (mut n, mut tips, mut prio, mut uris, mut v1) = (0, 0, 0, 0, 0);
        for name in &names {
            let fx = Fx::load(name);
            let Some(hit) = fx.hit() else { continue };
            let j = fx.json_parsed();
            let uri = create_uri(&j);
            let want = create_tx_features(
                &j,
                Some(77),
                &hit.creator_arg,
                &hit.user,
                &uri,
                &m.tip_accounts,
            );
            let got = create_tx_features_wire(&hit, Some(77));
            let k = if uri.is_empty() { 14 } else { N_CREATE };
            assert_eq!(got[..k], want[..k], "{name}");
            n += 1;
            tips += (got[2] > 0.0) as u32;
            prio += (got[4] > 0.0) as u32;
            uris += !uri.is_empty() as u32;
            v1 += (hit.version == Some(TxVersion::V1)) as u32;
        }
        assert!(n >= 20, "create fixtures decoded: {n}");
        assert!(
            tips >= 3 && prio >= 10 && uris >= 15 && v1 >= 5,
            "exercised: {tips} tips, {prio} priority fees, {uris} uris, {v1} v1"
        );
    }

    /// The pre-send vector of a fixture create and its bundle run, block
    /// by block: the bundle block is `bundle_features` on the same trades
    /// (delivered out of order, one twice), the create block the JSON
    /// path's, funding the masked defaults, operator the snapshot entry,
    /// time `time_features`.
    #[test]
    fn presend_vector_layout_on_a_fixture_create_and_run() {
        let m = model();
        let fx = Fx::load("fresh_v0alt_create_v2_buy18.json");
        let h = fx.hit().expect("the fixture is a create");
        assert!(h.intx_buys > 0 && h.tx_shape.tip_lamports > 0);
        let (owner, w1, w2, w3) = (
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        );
        let ci = 40;
        let mut p = parked(&h, ci);
        let (v0, t0) = crate::convert::v0_of(&h);
        // the run: w1 at +1, w2 at +3 (delivered first), a sell at +2, w3
        // at +4, the +1 copy again
        let tr = |user, is_buy, index, sol: u64, k: u64| {
            let mut t = trade(h.mint, user, is_buy, 100, index);
            t.sol_amount = sol;
            t.token_amount = sol * 31;
            t.virtual_sol_reserves = v0 + k * 1_000_000_000;
            t.virtual_token_reserves = t0 - k * 20_000_000_000_000;
            t
        };
        let run = [
            tr(w2, true, ci + 3, 750_000_000, 3),
            tr(w1, true, ci + 1, 500_000_000, 1),
            tr(w1, false, ci + 2, 100_000_000, 2),
            tr(w3, true, ci + 4, 500_000_000, 4),
            tr(w1, true, ci + 1, 500_000_000, 1),
        ];
        for t in &run {
            park_trade(&mut p, t, &owner, WAIT, 1_000);
        }
        assert_eq!((p.run.n, p.run.full, p.bundle_buys), (4, false, 3));
        let mut buf = [S0Trade::default(); PARK_RUN + 1];
        let n = p.run.collect(create_trade(&h, ci), &mut buf);
        // the same creation slot built independently, in block order
        let s0 = |t: &TradeLite| S0Trade {
            index: t.index,
            is_create: false,
            user: t.user,
            is_buy: t.is_buy,
            sol: t.sol_amount,
            tok: t.token_amount,
            v: t.virtual_sol_reserves,
            t: t.virtual_token_reserves,
        };
        let mut slot0 = vec![S0Trade {
            index: ci,
            is_create: true,
            user: h.user,
            is_buy: true,
            sol: h.intx_sol_in,
            tok: h.intx_tokens_out,
            v: v0,
            t: t0,
        }];
        slot0.extend([&run[1], &run[2], &run[0], &run[3]].map(s0));
        assert_eq!(&buf[..n], &slot0[..]);
        // the signer's outcomes as the cold thread publishes them
        let row = |intent, lead, ret| rugsnipe_state::rows::OutcomeRow {
            intent_id: intent,
            mint: Pubkey::new_unique(),
            signer: h.user,
            launch_ms: 0,
            outcome: "filled".into(),
            lead,
            ret,
            tp_hit: lead,
            s0_third: Some(2),
            bundle_wallets: vec![],
            written_ms: 0,
        };
        let store = OutcomeStore::from_rows(&[row(1, true, 0.12), row(2, false, 0.0)]);
        let snap = op_snapshot(&store);
        let op = snap.load().get(&h.user).copied().unwrap();
        assert_eq!(op, store.signer(&h.user).features());
        assert_ne!(op, OpStats::default().features());
        let (now, slot, rates) = (1_791_990_000i64, 456_794_475u64, (Some(41), None));
        let (f, scored) = presend_features(&buf[..n], &h, Some(ci), &op, now, slot, rates);
        assert!(scored);
        let cset = [h.creator_arg, h.user, h.fee_payer.unwrap()];
        let b = bundle_features(&slot0, &cset, None).unwrap();
        assert_eq!(b.wallets, vec![w1, w2, w3]);
        assert_eq!(f.x[BUNDLE..BUNDLE + N_BUNDLE], b.x);
        let j = fx.json_parsed();
        let want = create_tx_features(
            &j,
            Some(ci),
            &h.creator_arg,
            &h.user,
            &create_uri(&j),
            &m.tip_accounts,
        );
        assert_eq!(f.x[CREATE_TX..CREATE_TX + N_CREATE], want);
        assert_eq!(
            f.x[FUNDING..FUNDING + N_FUNDING],
            funding_features(&Funding::default())
        );
        assert_eq!(f.x[FUNDING..FUNDING + N_FUNDING], [0.0; N_FUNDING]);
        assert_eq!(f.x[OPERATOR..OPERATOR + N_OPERATOR], op);
        assert_eq!(
            f.x[TIME..TIME + N_TIME],
            time_features(now, slot, Some(41), None)
        );
        assert!((0.0..=1.0).contains(&m.score(&f)));
        // no create trade: the bundle block is zeros and the vector unscored
        let mut no_dev = h;
        no_dev.intx_buys = 0;
        let n = p.run.collect(create_trade(&no_dev, ci), &mut buf);
        let (f, scored) = presend_features(&buf[..n], &no_dev, Some(ci), &op, now, slot, rates);
        assert!(!scored);
        assert_eq!(f.x[BUNDLE..BUNDLE + N_BUNDLE], [0.0; N_BUNDLE]);
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

    /// `entry.bundle_wallets` counts the park's buyers outside the creator
    /// set (rt.json's `n_bw`): a buyer that is also a buyer or signer of the
    /// create transaction, or its fee payer, is not a bundle wallet.
    #[test]
    fn bundle_wallets_are_counted_outside_the_creator_set() {
        use crate::operators::{bundle_shape_ok, bundle_wallet_count};
        let (dev, cre, cobuyer) = (
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        );
        let mut h = hit(dev, cre);
        h.intx_buyers.push(cobuyer);
        let mut p = parked(&h, 10);
        let owner = Pubkey::new_unique();
        let (w1, w2) = (Pubkey::new_unique(), Pubkey::new_unique());
        for (k, u) in [w1, w2, cobuyer, w1].iter().enumerate() {
            let t = trade(h.mint, *u, true, 100, 11 + k as u32);
            park_trade(&mut p, &t, &owner, 1_000_000, 0);
        }
        // the park keeps distinct buyers (dev and creator never trigger)
        assert_eq!(p.n_wallets, 3);
        assert_eq!(
            bundle_wallet_count(&p),
            2,
            "the create tx's co-buyer is creator set"
        );
        assert!(bundle_shape_ok(&[2, 3, 4], 2));
        assert!(!bundle_shape_ok(&[3], 2));
        assert!(bundle_shape_ok(&[], 0), "empty = any");
    }
}
