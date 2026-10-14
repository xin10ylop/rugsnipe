//! `RiskGate`: every buy passes `decide()`, lock-free (atomics only).
//!
//! * **kill latch**: buys stop, exits continue; only a local `unkill`
//!   clears it. Set by the kill file, signals, the daily-loss cap, error
//!   ratios (callers) and [`RiskGate::kill`].
//! * **caps** (docs/06 section 6): spend per snipe / per probe, bid, entry
//!   virtual SOL, open positions, buys per hour, buys per operator per day,
//!   probes per day, minimum hot balance, daily loss.
//! * **consecutive-loss latch per operator**: `max_consec_losses_per_op`
//!   losses in a row disarm the operator until the next UTC day.
//!
//! Counted caps are *reserved* with compare-and-swap at decision time, so
//! concurrent deciders can never exceed them; an intent that never fills
//! gives its position slot back with [`RiskGate::release_unfilled`].
//! In paper mode nothing is reserved: the decision reports what a real mode
//! would have done (`would_skip`) so paper P&L reflects the caps.

use crate::config::RiskCfg;
use crate::mode::Mode;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, AtomicU8, Ordering::*};

/// Per-operator slots (op ids are mapped to slots by the caller).
pub const MAX_OPS: usize = 1024;

/// Why a buy is skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SkipReason {
    Killed = 1,
    SpendCap,
    BidCap,
    EntryTooHigh,
    BalanceLow,
    DailyLoss,
    OpLatched,
    MaxOpenPositions,
    MaxBuysPerHour,
    MaxBuysPerOpPerDay,
    MaxProbesPerDay,
    BadOpSlot,
}

/// Why a buy is only recorded (paper).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PaperReason {
    ModePaper = 1,
    RegimeFrozen,
    /// The operator's ArmSet state does not allow real buys in this mode.
    OpNotArmed,
    /// ArmSet missing, unsigned, stale or gate not passed.
    ArmSetGate,
}

/// Outcome of [`RiskGate::decide`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Real buy; caps reserved.
    Fire,
    /// Record only. `would_skip`: what a real mode would have done.
    Paper {
        reason: PaperReason,
        would_skip: Option<SkipReason>,
    },
    Skip(SkipReason),
}

impl Decision {
    pub fn is_fire(&self) -> bool {
        matches!(self, Self::Fire)
    }
    /// Compact code for rslog: 0 fire, 0x10 | paper reason, 0x20 | skip reason.
    pub fn code(&self) -> u8 {
        match self {
            Self::Fire => 0,
            Self::Paper { reason, .. } => 0x10 | *reason as u8,
            Self::Skip(r) => 0x20 | *r as u8,
        }
    }
}

/// Inputs of one decision.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecideInput {
    pub mode: Mode,
    /// Slot in [`MAX_OPS`] for the operator.
    pub op_slot: usize,
    /// ArmSet state allows real buys for this op in `mode`.
    pub op_armed: bool,
    /// ArmSet verified, fresh and `gate_passed` (live) / verified (probe).
    pub armset_ok: bool,
    /// `mode.regime_freeze_slots` contains the create slot.
    pub frozen: bool,
    /// Lamports spent on the buy (gross).
    pub spend: u64,
    /// Priority fee + tip of the costliest variant.
    pub bid: u64,
    /// Expected virtual SOL at entry (after in-tx buys).
    pub v_entry: u64,
    pub now_unix: u64,
}

#[derive(Debug, Default)]
struct OpRisk {
    consec_losses: AtomicU32,
    /// `day << 32 | buys`.
    buys_day: AtomicU64,
    latched_until_day: AtomicU32,
}

#[derive(Debug)]
pub struct RiskGate {
    caps: RiskCfg,
    killed: AtomicBool,
    kill_reason: AtomicU8,
    open_positions: AtomicU64,
    /// `hour << 32 | buys`.
    buys_hour: AtomicU64,
    /// `day << 32 | probes`.
    probes_day: AtomicU64,
    loss_day: AtomicU32,
    realized_today: AtomicI64,
    hot_balance: AtomicU64,
    ops: Box<[OpRisk]>,
}

/// Why the kill latch was set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum KillReason {
    None = 0,
    KillFile,
    Signal,
    Remote,
    DailyLoss,
    UnexplainedDrop,
    SendErrors,
    DecodeMiss,
    ProgramUpgrade,
    StaleArmSet,
    Disk,
    Clock,
    Drift,
    Manual,
}

impl KillReason {
    fn from_u8(v: u8) -> Self {
        use KillReason::*;
        [
            None,
            KillFile,
            Signal,
            Remote,
            DailyLoss,
            UnexplainedDrop,
            SendErrors,
            DecodeMiss,
            ProgramUpgrade,
            StaleArmSet,
            Disk,
            Clock,
            Drift,
            Manual,
        ]
        .get(v as usize)
        .copied()
        .unwrap_or(Manual)
    }
}

#[inline]
fn day_of(unix: u64) -> u32 {
    (unix / 86_400) as u32
}
#[inline]
fn hour_of(unix: u64) -> u32 {
    (unix / 3_600) as u32
}

/// Count in a `bucket << 32 | count` word for `bucket` (0 if stale).
#[inline]
fn packed_count(word: u64, bucket: u32) -> u64 {
    if (word >> 32) as u32 == bucket {
        word & 0xffff_ffff
    } else {
        0
    }
}

/// Reserve one unit in a packed bucket counter if below `max`.
fn packed_try_inc(a: &AtomicU64, bucket: u32, max: u64) -> bool {
    a.fetch_update(AcqRel, Acquire, |w| {
        let n = packed_count(w, bucket);
        (n < max).then(|| ((bucket as u64) << 32) | (n + 1))
    })
    .is_ok()
}

fn packed_dec(a: &AtomicU64, bucket: u32) {
    let _ = a.fetch_update(AcqRel, Acquire, |w| {
        let n = packed_count(w, bucket);
        (n > 0).then(|| ((bucket as u64) << 32) | (n - 1))
    });
}

impl RiskGate {
    pub fn new(caps: RiskCfg) -> Self {
        Self {
            caps,
            killed: AtomicBool::new(false),
            kill_reason: AtomicU8::new(0),
            open_positions: AtomicU64::new(0),
            buys_hour: AtomicU64::new(0),
            probes_day: AtomicU64::new(0),
            loss_day: AtomicU32::new(0),
            realized_today: AtomicI64::new(0),
            hot_balance: AtomicU64::new(0),
            ops: (0..MAX_OPS).map(|_| OpRisk::default()).collect(),
        }
    }

    pub fn caps(&self) -> &RiskCfg {
        &self.caps
    }

    // ------------------------------------------------------------ kill
    /// Set the kill latch (idempotent; the first reason wins).
    pub fn kill(&self, reason: KillReason) {
        if !self.killed.swap(true, AcqRel) {
            self.kill_reason.store(reason as u8, Release);
        }
    }
    pub fn is_killed(&self) -> bool {
        self.killed.load(Acquire)
    }
    pub fn kill_reason(&self) -> Option<KillReason> {
        self.is_killed()
            .then(|| KillReason::from_u8(self.kill_reason.load(Acquire)))
    }
    /// Local, deliberate unkill (`ctl unkill` on the box only).
    pub fn unkill_local(&self) {
        self.kill_reason.store(0, Release);
        self.killed.store(false, Release);
    }

    // ------------------------------------------------------- state feed
    /// Latest wallet balance (lamports) from the chain.
    pub fn set_hot_balance(&self, lamports: u64) {
        self.hot_balance.store(lamports, Release);
    }
    pub fn open_positions(&self) -> u64 {
        self.open_positions.load(Acquire)
    }
    pub fn realized_today(&self, now_unix: u64) -> i64 {
        if self.loss_day.load(Acquire) == day_of(now_unix) {
            self.realized_today.load(Acquire)
        } else {
            0
        }
    }
    pub fn op_consec_losses(&self, op_slot: usize) -> u32 {
        self.ops
            .get(op_slot)
            .map(|o| o.consec_losses.load(Acquire))
            .unwrap_or(0)
    }

    /// An intent that never filled (failed, burned): give back its position slot.
    pub fn release_unfilled(&self) {
        let _ = self
            .open_positions
            .fetch_update(AcqRel, Acquire, |n| n.checked_sub(1));
    }

    /// A position closed with realized `pnl` lamports (chain-derived).
    /// Updates the daily loss (kills at the cap) and the operator's
    /// consecutive-loss latch.
    pub fn on_position_closed(&self, op_slot: usize, pnl: i64, now_unix: u64) {
        self.release_unfilled();
        let day = day_of(now_unix);
        if self.loss_day.swap(day, AcqRel) != day {
            self.realized_today.store(0, Release);
        }
        let total = self.realized_today.fetch_add(pnl, AcqRel) + pnl;
        if self.caps.max_daily_loss_lamports > 0
            && total <= -(self.caps.max_daily_loss_lamports as i64)
        {
            self.kill(KillReason::DailyLoss);
        }
        if let Some(op) = self.ops.get(op_slot) {
            if pnl < 0 {
                let n = op.consec_losses.fetch_add(1, AcqRel) + 1;
                if self.caps.max_consec_losses_per_op > 0
                    && n as u64 >= self.caps.max_consec_losses_per_op
                {
                    op.latched_until_day.store(day + 1, Release);
                }
            } else {
                op.consec_losses.store(0, Release);
            }
        }
    }

    /// Mark-to-market check (wallet + sell quotes + rent vs start): call with
    /// the current day's total P&L; kills at the cap.
    pub fn check_daily_mark(&self, pnl_today: i64) {
        if self.caps.max_daily_loss_lamports > 0
            && pnl_today <= -(self.caps.max_daily_loss_lamports as i64)
        {
            self.kill(KillReason::DailyLoss);
        }
    }

    // ---------------------------------------------------------- decide
    fn static_checks(&self, i: &DecideInput) -> Option<SkipReason> {
        let c = &self.caps;
        let spend_cap = if i.mode == Mode::Probe {
            c.max_probe_lamports
        } else {
            c.max_snipe_lamports
        };
        if i.spend == 0 || i.spend > spend_cap {
            return Some(SkipReason::SpendCap);
        }
        if i.bid > c.max_bid_lamports {
            return Some(SkipReason::BidCap);
        }
        if i.v_entry > c.max_entry_vsol_lamports {
            return Some(SkipReason::EntryTooHigh);
        }
        let need = c
            .min_hot_balance_lamports
            .saturating_add(i.spend)
            .saturating_add(i.bid);
        if self.hot_balance.load(Acquire) < need {
            return Some(SkipReason::BalanceLow);
        }
        if self.realized_today(i.now_unix) <= -(c.max_daily_loss_lamports as i64) {
            return Some(SkipReason::DailyLoss);
        }
        let op = self.ops.get(i.op_slot)?;
        if op.latched_until_day.load(Acquire) > day_of(i.now_unix) {
            return Some(SkipReason::OpLatched);
        }
        None
    }

    fn counted_would_skip(&self, i: &DecideInput) -> Option<SkipReason> {
        let c = &self.caps;
        if self.open_positions.load(Acquire) >= c.max_open_positions {
            return Some(SkipReason::MaxOpenPositions);
        }
        if packed_count(self.buys_hour.load(Acquire), hour_of(i.now_unix)) >= c.max_buys_per_hour {
            return Some(SkipReason::MaxBuysPerHour);
        }
        let op = self.ops.get(i.op_slot)?;
        if packed_count(op.buys_day.load(Acquire), day_of(i.now_unix)) >= c.max_buys_per_op_per_day
        {
            return Some(SkipReason::MaxBuysPerOpPerDay);
        }
        if i.mode == Mode::Probe
            && packed_count(self.probes_day.load(Acquire), day_of(i.now_unix))
                >= c.max_probes_per_day
        {
            return Some(SkipReason::MaxProbesPerDay);
        }
        None
    }

    /// Decide one buy. Real modes reserve counted caps on `Fire`.
    pub fn decide(&self, i: &DecideInput) -> Decision {
        if self.is_killed() {
            return Decision::Skip(SkipReason::Killed);
        }
        if i.op_slot >= MAX_OPS {
            return Decision::Skip(SkipReason::BadOpSlot);
        }
        let paper = if !i.mode.is_real() {
            Some(PaperReason::ModePaper)
        } else if i.frozen {
            Some(PaperReason::RegimeFrozen)
        } else if !i.armset_ok {
            Some(PaperReason::ArmSetGate)
        } else if !i.op_armed {
            Some(PaperReason::OpNotArmed)
        } else {
            None
        };
        if let Some(reason) = paper {
            let as_live = DecideInput {
                mode: if i.mode == Mode::Paper {
                    Mode::Live
                } else {
                    i.mode
                },
                ..*i
            };
            return Decision::Paper {
                reason,
                would_skip: self
                    .static_checks(&as_live)
                    .or_else(|| self.counted_would_skip(&as_live)),
            };
        }
        if let Some(r) = self.static_checks(i) {
            if r == SkipReason::DailyLoss {
                self.kill(KillReason::DailyLoss);
            }
            return Decision::Skip(r);
        }
        let c = &self.caps;
        let (day, hour) = (day_of(i.now_unix), hour_of(i.now_unix));
        let op = &self.ops[i.op_slot];
        // Reserve in order; roll back on the first failure.
        if self
            .open_positions
            .fetch_update(AcqRel, Acquire, |n| {
                (n < c.max_open_positions).then_some(n + 1)
            })
            .is_err()
        {
            return Decision::Skip(SkipReason::MaxOpenPositions);
        }
        if !packed_try_inc(&self.buys_hour, hour, c.max_buys_per_hour) {
            self.release_unfilled();
            return Decision::Skip(SkipReason::MaxBuysPerHour);
        }
        if !packed_try_inc(&op.buys_day, day, c.max_buys_per_op_per_day) {
            packed_dec(&self.buys_hour, hour);
            self.release_unfilled();
            return Decision::Skip(SkipReason::MaxBuysPerOpPerDay);
        }
        if i.mode == Mode::Probe && !packed_try_inc(&self.probes_day, day, c.max_probes_per_day) {
            packed_dec(&op.buys_day, day);
            packed_dec(&self.buys_hour, hour);
            self.release_unfilled();
            return Decision::Skip(SkipReason::MaxProbesPerDay);
        }
        Decision::Fire
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn caps() -> RiskCfg {
        RiskCfg {
            max_snipe_lamports: 250_000_000,
            max_probe_lamports: 20_000_000,
            max_probes_per_day: 3,
            max_open_positions: 4,
            max_daily_loss_lamports: 1_000_000_000,
            max_consec_losses_per_op: 2,
            max_buys_per_op_per_day: 10,
            max_buys_per_hour: 20,
            max_bid_lamports: 25_000_000,
            max_sell_spend_lamports: 20_000_000,
            max_entry_vsol_lamports: 40_000_000_000,
            max_hot_balance_lamports: 5_000_000_000,
            min_hot_balance_lamports: 1_000_000_000,
            ..Default::default()
        }
    }

    fn inp(mode: Mode) -> DecideInput {
        DecideInput {
            mode,
            op_slot: 1,
            op_armed: true,
            armset_ok: true,
            frozen: false,
            spend: 100_000_000,
            bid: 1_000_000,
            v_entry: 32_000_000_000,
            now_unix: 1_791_968_762,
        }
    }

    fn gate() -> RiskGate {
        let g = RiskGate::new(caps());
        g.set_hot_balance(3_000_000_000);
        g
    }

    #[test]
    fn paper_never_reserves_and_reports_would_skip() {
        let g = gate();
        for _ in 0..10 {
            assert_eq!(
                g.decide(&inp(Mode::Paper)),
                Decision::Paper {
                    reason: PaperReason::ModePaper,
                    would_skip: None
                }
            );
        }
        assert_eq!(g.open_positions(), 0);
        let mut big = inp(Mode::Paper);
        big.spend = 300_000_000;
        assert_eq!(
            g.decide(&big),
            Decision::Paper {
                reason: PaperReason::ModePaper,
                would_skip: Some(SkipReason::SpendCap)
            }
        );
    }

    #[test]
    fn gates_force_paper() {
        let g = gate();
        let mut i = inp(Mode::Live);
        i.frozen = true;
        assert!(matches!(
            g.decide(&i),
            Decision::Paper {
                reason: PaperReason::RegimeFrozen,
                ..
            }
        ));
        let mut i = inp(Mode::Live);
        i.armset_ok = false;
        assert!(matches!(
            g.decide(&i),
            Decision::Paper {
                reason: PaperReason::ArmSetGate,
                ..
            }
        ));
        let mut i = inp(Mode::Live);
        i.op_armed = false;
        assert!(matches!(
            g.decide(&i),
            Decision::Paper {
                reason: PaperReason::OpNotArmed,
                ..
            }
        ));
    }

    #[test]
    fn static_caps() {
        let g = gate();
        let mut i = inp(Mode::Live);
        i.bid = 30_000_000;
        assert_eq!(g.decide(&i), Decision::Skip(SkipReason::BidCap));
        let mut i = inp(Mode::Live);
        i.v_entry = 41_000_000_000;
        assert_eq!(g.decide(&i), Decision::Skip(SkipReason::EntryTooHigh));
        let mut i = inp(Mode::Probe);
        i.spend = 25_000_000;
        assert_eq!(g.decide(&i), Decision::Skip(SkipReason::SpendCap));
        g.set_hot_balance(1_050_000_000);
        assert_eq!(
            g.decide(&inp(Mode::Live)),
            Decision::Skip(SkipReason::BalanceLow)
        );
    }

    #[test]
    fn open_positions_cap_holds_under_concurrency() {
        let g = Arc::new(gate());
        let fired: usize = (0..8)
            .map(|_| {
                let g = g.clone();
                std::thread::spawn(move || {
                    (0..50)
                        .filter(|_| g.decide(&inp(Mode::Live)).is_fire())
                        .count()
                })
            })
            .map(|h| h.join().unwrap())
            .sum();
        assert_eq!(fired, 4);
        assert_eq!(g.open_positions(), 4);
        assert_eq!(
            g.decide(&inp(Mode::Live)),
            Decision::Skip(SkipReason::MaxOpenPositions)
        );
        g.release_unfilled();
        assert!(g.decide(&inp(Mode::Live)).is_fire());
    }

    #[test]
    fn probes_per_day_and_rollback() {
        let g = gate();
        for _ in 0..3 {
            let mut i = inp(Mode::Probe);
            i.spend = 10_000_000;
            assert!(g.decide(&i).is_fire());
            g.release_unfilled();
        }
        let mut i = inp(Mode::Probe);
        i.spend = 10_000_000;
        assert_eq!(g.decide(&i), Decision::Skip(SkipReason::MaxProbesPerDay));
        assert_eq!(g.open_positions(), 0, "rolled back");
        // next day resets
        i.now_unix += 86_400;
        assert!(g.decide(&i).is_fire());
    }

    #[test]
    fn consecutive_losses_latch_the_operator_until_tomorrow() {
        let g = gate();
        let now = inp(Mode::Live).now_unix;
        assert!(g.decide(&inp(Mode::Live)).is_fire());
        g.on_position_closed(1, -10, now);
        assert!(g.decide(&inp(Mode::Live)).is_fire());
        g.on_position_closed(1, -10, now);
        assert_eq!(g.op_consec_losses(1), 2);
        assert_eq!(
            g.decide(&inp(Mode::Live)),
            Decision::Skip(SkipReason::OpLatched)
        );
        // another operator is unaffected
        let mut other = inp(Mode::Live);
        other.op_slot = 2;
        assert!(g.decide(&other).is_fire());
        let mut tomorrow = inp(Mode::Live);
        tomorrow.now_unix += 86_400;
        assert!(g.decide(&tomorrow).is_fire());
        // a win resets the streak
        g.on_position_closed(1, 5, now);
        assert_eq!(g.op_consec_losses(1), 0);
    }

    #[test]
    fn daily_loss_kills_and_kill_stops_buys_only_unkill_local() {
        let g = gate();
        let now = inp(Mode::Live).now_unix;
        g.on_position_closed(3, -600_000_000, now);
        assert!(!g.is_killed());
        g.on_position_closed(4, -400_000_000, now);
        assert!(g.is_killed());
        assert_eq!(g.kill_reason(), Some(KillReason::DailyLoss));
        assert_eq!(
            g.decide(&inp(Mode::Live)),
            Decision::Skip(SkipReason::Killed)
        );
        assert_eq!(
            g.decide(&inp(Mode::Paper)),
            Decision::Skip(SkipReason::Killed)
        );
        g.kill(KillReason::KillFile); // first reason wins
        assert_eq!(g.kill_reason(), Some(KillReason::DailyLoss));
        g.unkill_local();
        assert!(!g.is_killed());
        // still at the daily loss cap today -> skipped and re-killed
        assert_eq!(
            g.decide(&inp(Mode::Live)),
            Decision::Skip(SkipReason::DailyLoss)
        );
        assert!(g.is_killed());
    }

    #[test]
    fn decision_codes_are_distinct() {
        let a = Decision::Fire.code();
        let b = Decision::Paper {
            reason: PaperReason::ModePaper,
            would_skip: None,
        }
        .code();
        let c = Decision::Skip(SkipReason::Killed).code();
        assert!(a != b && b != c && a != c);
    }
}
