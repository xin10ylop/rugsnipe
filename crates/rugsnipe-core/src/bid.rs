//! Per-leader-class bid policy and the cost of a failed attempt
//! (docs/03 section 6 "Pay on success", docs/04 section 7, docs/06 section 6).
//!
//! A slippage-capped snipe that loses the race lands and fails on chain (or
//! is dropped). What it costs depends only on where the bid sits:
//!
//! * the **base fee** (5,000 lamports per signature) and the **priority
//!   fee** are charged whether the transaction succeeds or fails
//!   (solana.com/docs/core/transactions: "Fees are still charged on
//!   failure");
//! * a **tip** is a System transfer inside the same transaction. A failed
//!   transaction reverts every instruction ("If any instruction fails, the
//!   entire transaction fails and all state changes are reverted"), so the
//!   tip only moves when the buy succeeds. The templates put it **after** the
//!   buy (rugsnipe-tx `template`), never in a separate transaction (a
//!   stand-alone tip transaction can be unbundled, docs.jito.wtf);
//! * a **Jito bundle** (or `bundleOnly` send) that fails is not committed at
//!   all: it costs nothing.
//!
//! On chain this is visible in gap-2: slippage losers paid a median priority
//! fee of 0.00015 SOL (p75 0.0028, p90 0.0125) and no tip; gap-5 counts a
//! failed transaction as "untipped" for exactly that reason.
//!
//! So the bid goes into the tip wherever the leader class orders by it
//! (Jito-Agave via the block engine, BAM via its auction: both through the
//! Jito tip accounts) and the header priority fee stays at the scheduling
//! floor `bid.min_priority_lamports`. Harmonic (and Firedancer / other
//! clients) order by priority fee: there the bid is the priority fee and it
//! is at risk on failure, clamped so that a failed attempt never costs more
//! than `bid.failed_attempt_lamports_max`.

use crate::config::{BidCfg, LeaderMix};

/// Base fee per signature (lamports).
pub const BASE_FEE_PER_SIGNATURE: u64 = 5_000;

/// Who orders the leader's block (docs/03 section 6; `getClusterNodes`
/// client id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LeaderClass {
    /// Jito-Agave (`JitoLabs`): arrival order; tips count through the block engine.
    JitoAgave,
    /// BAM (`AgaveBam`, `FireBAM`): 20-50 ms batch auctions run by the BAM node.
    Bam,
    /// Harmonic (`HarmonicAgave`, `HarmonicFiredancer`): priority-fee batches.
    Harmonic,
    /// Firedancer, Frankendancer, plain Agave, unknown: priority fee.
    Other,
}

impl LeaderClass {
    pub const ALL: [LeaderClass; 4] = [Self::JitoAgave, Self::Bam, Self::Harmonic, Self::Other];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::JitoAgave => "jito_agave",
            Self::Bam => "bam",
            Self::Harmonic => "harmonic",
            Self::Other => "other",
        }
    }

    /// From a `getClusterNodes` `clientId` (docs/03 section 6, gap-5).
    pub fn from_client_id(id: &str) -> Self {
        let l = id.to_ascii_lowercase();
        if l.contains("bam") {
            Self::Bam
        } else if l.contains("harmonic") {
            Self::Harmonic
        } else if l.contains("jito") {
            Self::JitoAgave
        } else {
            Self::Other
        }
    }

    /// The class orders by a tip to the Jito tip accounts, so the bid can be
    /// paid only on success.
    pub fn honours_tip(self) -> bool {
        matches!(self, Self::JitoAgave | Self::Bam)
    }
}

/// What one variant pays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BidPlan {
    pub class: LeaderClass,
    /// Total priority fee (v1 header total; legacy converts to a CU price).
    pub priority_lamports: u64,
    /// In-transaction tip after the buy (reverts with a failed buy).
    pub tip_lamports: u64,
    /// The priority fee was cut to respect `bid.failed_attempt_lamports_max`.
    pub clamped: bool,
}

impl BidPlan {
    /// Cost of this variant when it lands and fails: base fee and priority
    /// fee (the tip reverts).
    pub fn failed_cost(&self, n_sigs: u8) -> u64 {
        base_fee(n_sigs) + self.priority_lamports
    }
    /// Cost when it lands and the buy succeeds.
    pub fn landed_cost(&self, n_sigs: u8) -> u64 {
        self.failed_cost(n_sigs) + self.tip_lamports
    }
}

/// `5,000 x signatures` (at least one).
pub fn base_fee(n_sigs: u8) -> u64 {
    BASE_FEE_PER_SIGNATURE * n_sigs.max(1) as u64
}

/// The per-class split of a bid with the scheduling floor and the
/// failed-attempt cap (`[bid]`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BidPolicy {
    /// `[priority, tip]` fractions by class, in [`LeaderClass::ALL`] order.
    pub split: [[f64; 2]; 4],
    pub min_priority_lamports: u64,
    pub failed_attempt_lamports_max: u64,
}

impl BidPolicy {
    pub fn from_cfg(b: &BidCfg) -> Self {
        Self {
            split: [
                b.split.jito_agave,
                b.split.bam,
                b.split.harmonic,
                b.split.other,
            ],
            min_priority_lamports: b.min_priority_lamports,
            failed_attempt_lamports_max: b.failed_attempt_lamports_max,
        }
    }

    fn split_of(&self, c: LeaderClass) -> [f64; 2] {
        self.split[LeaderClass::ALL.iter().position(|x| *x == c).unwrap_or(3)]
    }

    /// Split the bid `bid` for a leader of class `c`: priority at least the
    /// floor, and never more than the cap leaves after the base fee; the tip
    /// is not capped (it is paid only on success, and `risk.max_bid_lamports`
    /// bounds the bid itself).
    pub fn plan(&self, c: LeaderClass, bid: u64, n_sigs: u8) -> BidPlan {
        let [p, t] = self.split_of(c);
        let want = ((bid as f64 * p) as u64).max(self.min_priority_lamports);
        let room = self
            .failed_attempt_lamports_max
            .saturating_sub(base_fee(n_sigs));
        BidPlan {
            class: c,
            priority_lamports: want.min(room),
            tip_lamports: (bid as f64 * t) as u64,
            clamped: want > room,
        }
    }

    /// Expected cost of a failed attempt when the leader class is unknown:
    /// the variant matching the leader is the one that lands, weighted by
    /// the classes' shares of leader slots (`paper.leader_mix`).
    pub fn expected_failed_cost(&self, mix: &LeaderMix, bid: u64, n_sigs: u8) -> u64 {
        let total = mix.total();
        if total <= 0.0 {
            return self
                .plan(LeaderClass::Other, bid, n_sigs)
                .failed_cost(n_sigs);
        }
        let e: f64 = LeaderClass::ALL
            .iter()
            .map(|c| mix.share(*c) * self.plan(*c, bid, n_sigs).failed_cost(n_sigs) as f64)
            .sum::<f64>()
            / total;
        e.round() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{BidCfg, LeaderMix};

    #[test]
    fn tip_classes_pay_on_success_and_priority_classes_are_capped() {
        let b = BidCfg::default();
        let pol = BidPolicy::from_cfg(&b);
        let bid = 3_400_000;
        for c in [LeaderClass::JitoAgave, LeaderClass::Bam] {
            let p = pol.plan(c, bid, 1);
            assert!(c.honours_tip());
            assert_eq!(p.priority_lamports, b.min_priority_lamports, "{c:?}");
            assert_eq!(p.tip_lamports, bid);
            assert_eq!(p.failed_cost(1), 5_000 + b.min_priority_lamports);
            assert_eq!(p.landed_cost(1), 5_000 + b.min_priority_lamports + bid);
            assert!(!p.clamped);
        }
        for c in [LeaderClass::Harmonic, LeaderClass::Other] {
            let p = pol.plan(c, bid, 1);
            assert!(!c.honours_tip());
            assert_eq!(p.tip_lamports, 0);
            // 3.4M wanted, the cap leaves cap - base
            assert_eq!(p.priority_lamports, b.failed_attempt_lamports_max - 5_000);
            assert_eq!(p.failed_cost(1), b.failed_attempt_lamports_max);
            assert!(p.clamped);
            // two signatures: the base fee doubles and the priority shrinks
            assert_eq!(
                pol.plan(c, bid, 2).failed_cost(2),
                b.failed_attempt_lamports_max
            );
        }
        // a small bid is not clamped, and never falls below the floor
        let p = pol.plan(LeaderClass::Harmonic, 500_000, 1);
        assert_eq!((p.priority_lamports, p.clamped), (500_000, false));
        let p = pol.plan(LeaderClass::Harmonic, 1, 1);
        assert_eq!(p.priority_lamports, b.min_priority_lamports);
    }

    #[test]
    fn no_plan_ever_loses_more_than_the_cap() {
        let mut b = BidCfg::default();
        for cap in [15_000u64, 20_000, 500_000, 3_000_000] {
            b.failed_attempt_lamports_max = cap;
            let pol = BidPolicy::from_cfg(&b);
            for bid in [0u64, 1, 10_000, 3_400_000, 25_000_000, u64::MAX / 4] {
                for c in LeaderClass::ALL {
                    for s in 1..=2u8 {
                        assert!(pol.plan(c, bid, s).failed_cost(s) <= cap.max(base_fee(s)));
                    }
                }
            }
        }
    }

    #[test]
    fn expected_cost_weights_the_classes() {
        let b = BidCfg::default();
        let pol = BidPolicy::from_cfg(&b);
        let mix = LeaderMix::default();
        let tip = 5_000 + b.min_priority_lamports;
        let prio = b.failed_attempt_lamports_max;
        let want = (mix.share(LeaderClass::JitoAgave) + mix.share(LeaderClass::Bam)) * tip as f64
            + (mix.share(LeaderClass::Harmonic) + mix.share(LeaderClass::Other)) * prio as f64;
        let got = pol.expected_failed_cost(&mix, 3_400_000, 1);
        assert!((got as f64 - want).abs() <= 1.0, "{got} vs {want}");
        // all-priority policy at the old 0.003 SOL: every failure pays it
        let mut old = b.clone();
        old.split.jito_agave = [1.0, 0.0];
        old.split.bam = [1.0, 0.0];
        old.failed_attempt_lamports_max = 3_005_000;
        let pol = BidPolicy::from_cfg(&old);
        assert_eq!(pol.expected_failed_cost(&mix, 3_000_000, 1), 3_005_000);
    }

    #[test]
    fn client_ids_map_to_classes() {
        assert_eq!(
            LeaderClass::from_client_id("JitoLabs"),
            LeaderClass::JitoAgave
        );
        assert_eq!(LeaderClass::from_client_id("AgaveBam"), LeaderClass::Bam);
        assert_eq!(LeaderClass::from_client_id("FireBAM"), LeaderClass::Bam);
        assert_eq!(
            LeaderClass::from_client_id("HarmonicAgave"),
            LeaderClass::Harmonic
        );
        assert_eq!(
            LeaderClass::from_client_id("HarmonicFiredancer"),
            LeaderClass::Harmonic
        );
        assert_eq!(
            LeaderClass::from_client_id("Firedancer"),
            LeaderClass::Other
        );
        assert_eq!(LeaderClass::from_client_id("Agave"), LeaderClass::Other);
    }
}
