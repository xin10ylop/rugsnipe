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
//! 8  V0 (curve after every in-tx buy) < op.v_max
//! 9  A/B skip (probe/live only)
//! 10 nonce claim xxh3(mint) % N, value older than the create slot (paper: simulated pool)
//! 11 size, min_tokens_out at v_max
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
//!   operator's `v_max` bound fails it: `slippage`), then the lane's tape of
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
use crate::msgs::{reason_code, reason_index, ColdMsg, FireJob, HotIn, ShadowInfo};
use crate::setup::{min_tokens_out, op_armed, ArmState, Templates};
use crate::stats::{code, HotStats};
use arc_swap::ArcSwap;
use hdrhistogram::Histogram;
use pump_codec::constants::{TOKEN_2022_PROGRAM_ID, WSOL_MINT};
use pump_codec::create_hit::CreateHit;
use pump_codec::curve::{CurveState, FeeBps};
use pump_codec::{pda, Pubkey};
use rugsnipe_core::config::{Config, FillModel};
use rugsnipe_core::risk::{DecideInput, Decision, RiskGate};
use rugsnipe_core::rslog::{CreateHitRec, Rec, RslogProducer, ShadowOutcome};
use rugsnipe_core::watchlist::{Match, Via, Watchlist};
use rugsnipe_core::{IntentId, Mode};
use rugsnipe_exit::{
    decide_into, BundleSet, ExitAction, ExitEvent, ExitPolicy, Now, Position, PositionState,
    SellOrder, Trade, Venue,
};
use rugsnipe_feed::clock::mono_ns;
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
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            slot_now: AtomicU64::new(0),
            heartbeat: AtomicU64::new(0),
            exits_owned: AtomicBool::new(true),
            stop: AtomicBool::new(false),
        }
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
    /// `[priority, tip]` fractions of the primary (Jito-Agave class) and secondary (BAM) variants.
    pub split_primary: [f64; 2],
    pub split_secondary: [f64; 2],
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
    /// Priority fee of a failed E1 attempt (`paper.failed_attempt_lamports`).
    pub failed_attempt_lamports: u64,
}

impl HotCfg {
    /// From the TOML. `bid_lamports` defaults to the measured median
    /// first-winner bid (docs/03 section 6: 0.0034 SOL), capped by `max_bid`.
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
            bid_lamports: 3_400_000u64.min(max_bid),
            max_bid,
            split_primary: cfg.bid.split.jito_agave,
            split_secondary: cfg.bid.split.bam,
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
        }
    }

    fn frozen(&self, slot: u64) -> bool {
        self.freeze.iter().any(|[a, b]| (*a..=*b).contains(&slot))
    }
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
    min_out: u64,
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
    /// E1 verdicts other than a fill (premium, slippage, not in slot 0, class, unresolved).
    pub e1_failed: u64,
    pub e1_replayed_trades: u64,
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
        FeedSrc::Grpc | FeedSrc::Shreds => LIVE_CLASS,
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
        }
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
        let now = mono_ns();
        if now.saturating_sub(self.last_tick_ns) >= self.cfg.tick_ms * 1_000_000 {
            self.last_tick_ns = now;
            self.tick(now);
        }
        self.shared.heartbeat.fetch_add(1, Relaxed);
        st
    }

    /// Process one feed event.
    pub fn on_event(&mut self, ev: &FeedEvent, buf: &FeedBuf, t0: u64) {
        self.counters.events += 1;
        self.stats.feed_events[ev.src as usize].inc();
        if ev.slot > self.slot_now {
            self.slot_now = ev.slot;
            self.shared.slot_now.store(ev.slot, Relaxed);
        }
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
                );
            }
            Payload::Wire(w) => {
                let raw = buf.wire(w);
                match pump_codec::tx::TxView::parse(raw) {
                    Ok(tx) => match pump_codec::create_hit(&tx, None, &[], self.cfg.fees) {
                        Ok(Some(mut hit)) => {
                            hit.slot = Some(ev.slot);
                            self.on_create(ev.src, ev.recv_ns, ev.slot, ev.bank_id, &hit, t0);
                        }
                        Ok(None) => {}
                        Err(_) => {
                            self.stats.wire_misses.inc();
                        }
                    },
                    Err(_) => {
                        self.stats.wire_misses.inc();
                    }
                }
            }
            Payload::Trade(t) => self.on_trade(trade_from_lite(&t)),
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

    fn on_create(
        &mut self,
        src: FeedSrc,
        recv_ns: u64,
        slot: u64,
        bank_id: u64,
        hit: &CreateHit,
        t0: u64,
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
        let intent = IntentId::new(&hit.mint, &self.owner).0;
        // 2: first-wins per mint and feed class
        if let Fired::Already { .. } = self.fired.first(&hit.mint, src_class(src), src) {
            return self.decision(intent, hit, code::ALREADY_FIRED, 0, 0, 0, 0, 0);
        }
        // 4: class filters
        if !hit.is_sol_quote()
            || hit.mayhem
            || hit.token_prog != TOKEN_2022_PROGRAM_ID
            || hit.cashback
        {
            return self.decision(intent, hit, code::CLASS, 0, 0, 0, 0, 0);
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
                    Mode::Probe => arm.probe_allowed,
                    Mode::Paper => true,
                },
            frozen: self.cfg.frozen(slot),
            spend: op.size,
            bid,
            v_entry: v0,
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
        if v0 >= op.v_max {
            if real {
                self.risk.release_unfilled();
            }
            return self.decision(intent, hit, code::TOO_HIGH, would, op.size, op.v_max, 0, 0);
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
        // over the four PDA bumps, clamped to [tx.cu_limit floor, 1.4M]),
        // before any nonce is claimed
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
        let Some(cu_limit) =
            keys.snipe_cu_limit(&self.owner, &TOKEN_2022_PROGRAM_ID, self.tmpl.cu_floor)
        else {
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
        // 11: size and the on-chain bound at v_max
        let spend = op.size;
        let min_out = min_tokens_out(spend, op.v_max, hit.creator_arg, self.cfg.fees);
        // 12: patch (keys and the per-mint CU limit were resolved before the claim)
        let prio0 = (bid as f64 * self.cfg.split_primary[0]) as u64;
        let tip0 = (bid as f64 * self.cfg.split_primary[1]) as u64;
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
                real: true,
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
        if self.cfg.secondary_variant {
            let job = FireJob {
                tmpl: t.clone(),
                intent,
                idx: 1,
                prio: (bid as f64 * self.cfg.split_secondary[0]) as u64,
                tip: (bid as f64 * self.cfg.split_secondary[1]) as u64,
                tip_account,
                // a paper decision's secondary variant is recorded, never sent
                real,
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
                prio: prio0,
                tip: tip0,
                sent_ns,
                result,
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
        });
        self.send_cold(ColdMsg::WatchMint(hit.mint));
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
        }
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
        if outcome != ShadowOutcome::Filled && outcome != ShadowOutcome::Unresolved {
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

    /// The E1 verdict: open the position at the post-bundle state with the
    /// premium, or record the failed attempt and its cost.
    fn e1_open(&mut self, i: usize, outcome: ShadowOutcome, info: ShadowInfo) {
        let fees = self.cfg.fees;
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
                // the operator's on-chain bound (min_tokens_out at v_max)
                Some((q, tokens)) if tokens >= s.min_out && tokens > 0 => fill = Some((q, tokens)),
                _ => outcome = ShadowOutcome::Slippage,
            }
        }
        let cost = if outcome.is_failed_attempt() {
            self.cfg.failed_attempt_lamports
        } else {
            0
        };
        self.record_shadow(i, outcome, info, cost);
        let Some(s) = self.slots[i].as_mut() else {
            return;
        };
        let (mint, intent, sig0, slot0) = (s.pos.mint, s.intent, s.sig0, s.create_slot);
        let Some((q, tokens)) = fill else {
            self.counters.e1_failed += 1;
            self.counters.unfilled += 1;
            self.stats.unfilled.inc();
            self.slots[i] = None;
            self.stats.positions_open.set(self.positions() as i64);
            if outcome != ShadowOutcome::Unresolved {
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
            .filter(|q| q.tokens >= s.min_out);
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
        // a paper position (paper mode, or forced to paper in a real mode) never
        // reaches a real sender
        let paper = !(self.cfg.mode.is_real() && s.real);
        let mut sig = [0u8; 64];
        if !owned {
            self.stats.sells_not_owner.inc();
        } else if order.venue == Venue::Curve {
            let t = if order.close_account {
                &mut self.tmpl.sell_final
            } else {
                &mut self.tmpl.sell_partial
            };
            t.patch(&Patch {
                keys: &s.keys,
                lifetime: &PAPER_BLOCKHASH,
                amount: order.tokens,
                limit: order.min_sol_output,
                priority_lamports: order.tip_lamports,
                tip_lamports: order.tip_lamports,
                cu_limit: None,
            });
            t.sign(&self.signer, None);
            let v = Variant::from_template(
                t,
                s.intent,
                0x80 | (order.try_no.min(127) as u8),
                VariantKind::Sell,
                order.tip_lamports,
                order.tip_lamports,
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
                    prio: order.tip_lamports,
                    tip: order.tip_lamports,
                    tip_account: self.tmpl.tip_account,
                    real: true,
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
