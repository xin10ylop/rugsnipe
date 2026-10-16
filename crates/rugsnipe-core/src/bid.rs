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
//! (Jito-Agave via the block engine, through the Jito tip accounts) and the
//! header priority fee stays at the scheduling floor
//! `bid.min_priority_lamports`. BAM, Harmonic (and Firedancer / other
//! clients) order by priority fee: there the bid is the priority fee and it
//! is at risk on failure, clamped so that a failed attempt never costs more
//! than `bid.failed_attempt_lamports_max`. BAM is in the priority group
//! because our own measurement (docs/03 section 6: the earlier transaction
//! inside a BAM round had the higher compute-unit price 86-89% of the time)
//! shows CU-price ordering; it stays there until the probe measures which
//! variant lands per leader class.
//!
//! Sells go through the same split ([`BidPolicy::plan_sell`]): a
//! tip-ordered class pays the priority floor plus the sell bid as a tip, a
//! priority-ordered class pays the sell bid as the priority fee and no tip.
//! The bid is never paid twice.

use crate::config::{BidCfg, LeaderMix};

/// Base fee per signature (lamports).
pub const BASE_FEE_PER_SIGNATURE: u64 = 5_000;

/// Who orders the leader's block (docs/03 section 6; `getClusterNodes`
/// client id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LeaderClass {
    /// Jito-Agave (`JitoLabs`): arrival order; tips count through the block engine.
    JitoAgave,
    /// BAM (`AgaveBam`, `FireBAM`): 20-50 ms batch auctions run by the BAM
    /// node; measured compute-unit-price order inside a round (86-89%).
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
    /// paid only on success. Only Jito-Agave (through the block engine): BAM
    /// rounds were measured in compute-unit-price order (docs/03 section 6),
    /// so BAM bids with the priority fee until the probe measures otherwise.
    pub fn honours_tip(self) -> bool {
        matches!(self, Self::JitoAgave)
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
    /// Per-class override of the cap in [`LeaderClass::ALL`] order (0 = the
    /// global cap; `bid.failed_attempt_max_by_class`).
    pub class_cap: [u64; 4],
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
            class_cap: [
                b.failed_attempt_max_by_class.jito_agave,
                b.failed_attempt_max_by_class.bam,
                b.failed_attempt_max_by_class.harmonic,
                b.failed_attempt_max_by_class.other,
            ],
        }
    }

    /// The failed-attempt cap of class `c`: its override, else the global cap.
    pub fn cap_of(&self, c: LeaderClass) -> u64 {
        match self.class_cap[LeaderClass::ALL.iter().position(|x| *x == c).unwrap_or(3)] {
            0 => self.failed_attempt_lamports_max,
            v => v,
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
        let room = self.cap_of(c).saturating_sub(base_fee(n_sigs));
        BidPlan {
            class: c,
            priority_lamports: want.min(room),
            tip_lamports: (bid as f64 * t) as u64,
            clamped: want > room,
        }
    }

    /// The variant of a **sell** for a leader of class `c` (`None`: unknown,
    /// planned as a priority-ordered class). `bid` is the exit engine's sell
    /// bid (`SellOrder::tip_lamports`, escalated on retries and bounded by
    /// `risk.max_sell_spend_lamports`). It is split like a buy bid, so the
    /// bid is paid once: a tip-ordered class pays the priority floor and the
    /// bid as a tip after the sell (reverts if the sell fails), a
    /// priority-ordered class pays the bid as the priority fee and no tip.
    /// Unlike a buy, the priority fee is not clamped to
    /// `failed_attempt_lamports_max`: a sell that does not land leaves the
    /// position open, and the exit engine already bounds its spend.
    pub fn plan_sell(&self, c: Option<LeaderClass>, bid: u64, _n_sigs: u8) -> BidPlan {
        let c = c.unwrap_or(LeaderClass::Other);
        let [p, t] = self.split_of(c);
        BidPlan {
            class: c,
            priority_lamports: ((bid as f64 * p) as u64).max(self.min_priority_lamports),
            tip_lamports: (bid as f64 * t) as u64,
            clamped: false,
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
    fn the_cap_can_be_raised_per_class_without_changing_the_default() {
        let mut b = BidCfg::default();
        let pol = BidPolicy::from_cfg(&b);
        let bid = 3_400_000;
        // default: every class at the global cap (990,000 of priority)
        for c in LeaderClass::ALL {
            assert_eq!(pol.cap_of(c), b.failed_attempt_lamports_max);
        }
        assert_eq!(
            pol.plan(LeaderClass::Harmonic, bid, 1).priority_lamports,
            995_000
        );
        // probe-A arm: Harmonic and BAM may risk the whole bid
        b.failed_attempt_max_by_class.harmonic = 5_000_000;
        b.failed_attempt_max_by_class.bam = 2_000_000;
        let pol = BidPolicy::from_cfg(&b);
        let h = pol.plan(LeaderClass::Harmonic, bid, 2);
        assert_eq!((h.priority_lamports, h.clamped), (bid, false));
        let m = pol.plan(LeaderClass::Bam, bid, 2);
        assert_eq!((m.priority_lamports, m.clamped), (2_000_000 - 10_000, true));
        // the others keep the global cap; sizing (the bid) is untouched
        let o = pol.plan(LeaderClass::Other, bid, 2);
        assert_eq!(o.priority_lamports, b.failed_attempt_lamports_max - 10_000);
        assert_eq!(pol.plan(LeaderClass::JitoAgave, bid, 2).tip_lamports, bid);
    }

    #[test]
    fn tip_classes_pay_on_success_and_priority_classes_are_capped() {
        let b = BidCfg::default();
        let pol = BidPolicy::from_cfg(&b);
        let bid = 3_400_000;
        // the tip-ordered class: Jito-Agave
        let c = LeaderClass::JitoAgave;
        let p = pol.plan(c, bid, 1);
        assert!(c.honours_tip());
        assert_eq!(p.priority_lamports, b.min_priority_lamports);
        assert_eq!(p.tip_lamports, bid);
        assert_eq!(p.failed_cost(1), 5_000 + b.min_priority_lamports);
        assert_eq!(p.landed_cost(1), 5_000 + b.min_priority_lamports + bid);
        assert!(!p.clamped);
        // BAM is priority-ordered by default (docs/03 section 6: CU-price
        // order inside BAM rounds), like Harmonic
        for c in [LeaderClass::Bam, LeaderClass::Harmonic, LeaderClass::Other] {
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
        let want = mix.share(LeaderClass::JitoAgave) * tip as f64
            + (mix.share(LeaderClass::Bam)
                + mix.share(LeaderClass::Harmonic)
                + mix.share(LeaderClass::Other))
                * prio as f64;
        let got = pol.expected_failed_cost(&mix, 3_400_000, 1);
        assert!((got as f64 - want).abs() <= 1.0, "{got} vs {want}");
        // docs/04 section 4: 0.272 x 15,000 + 0.728 x 1,000,000 = 732,080
        // lamports (0.00073 SOL) at the default stake mix and cap
        assert_eq!(got, 732_080);
        // the earlier pay-on-success BAM split gave 0.00036 SOL
        let mut tipbam = b.clone();
        tipbam.split.bam = [0.0, 1.0];
        let pol2 = BidPolicy::from_cfg(&tipbam);
        assert_eq!(pol2.expected_failed_cost(&mix, 3_400_000, 1), 358_765);
        // all-priority policy at the old 0.003 SOL: every failure pays it
        let mut old = b.clone();
        old.split.jito_agave = [1.0, 0.0];
        old.split.bam = [1.0, 0.0];
        old.failed_attempt_lamports_max = 3_005_000;
        let pol = BidPolicy::from_cfg(&old);
        assert_eq!(pol.expected_failed_cost(&mix, 3_000_000, 1), 3_005_000);
    }

    #[test]
    fn sells_pay_the_bid_once_through_the_class_split() {
        let b = BidCfg::default();
        let pol = BidPolicy::from_cfg(&b);
        let floor = b.min_priority_lamports;
        for bid in [0u64, 1, 5_000, 1_000_000, 3_400_000, 20_000_000] {
            for c in LeaderClass::ALL.map(Some).into_iter().chain([None]) {
                let p = pol.plan_sell(c, bid, 1);
                // never the bid as both priority fee and tip
                assert!(
                    p.priority_lamports + p.tip_lamports <= bid.max(floor) + floor,
                    "{c:?} {bid}: {p:?}"
                );
                assert!(p.priority_lamports >= floor);
                assert!(!p.clamped);
                if c.is_some_and(|c| c.honours_tip()) {
                    // tip-ordered: priority at the floor, the bid in the tip
                    assert_eq!((p.priority_lamports, p.tip_lamports), (floor, bid));
                } else {
                    // priority-ordered (BAM, Harmonic, other) or unknown: the
                    // bid as the priority fee, no tip
                    assert_eq!((p.priority_lamports, p.tip_lamports), (bid.max(floor), 0));
                }
            }
        }
        // a sell bid above the buy cap is not clamped: the exit engine
        // bounds the sell spend (risk.max_sell_spend_lamports)
        let p = pol.plan_sell(Some(LeaderClass::Harmonic), 5_000_000, 1);
        assert_eq!(p.priority_lamports, 5_000_000);
        assert!(p.priority_lamports > b.failed_attempt_lamports_max);
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
