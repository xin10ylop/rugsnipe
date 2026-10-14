//! Property tests: sells never exceed the balance, the spend cap holds, and a
//! dump trigger always precedes (and suppresses) a take-profit in the same slot.

use proptest::prelude::*;
use pump_codec::curve::{CurveState, FeeBps, K_INITIAL};
use pump_codec::Pubkey;
use rugsnipe_exit::*;

const SOL: u64 = 1_000_000_000;
const SUPPLY: u64 = 1_000_000_000_000_000;

#[derive(Debug, Clone)]
enum Step {
    /// A buy of `sol` lamports by user index (0 = dev/bundle, 1 = stranger, 2 = us).
    Buy(u8, u64),
    /// A sell of `bps` of the curve's distance to the floor by user index.
    Sell(u8, u16),
    /// Transfer of `pct_milli` thousandths of supply to sink 0/1.
    Transfer(u8, u16),
    /// Advance `n` slots (and 400 ms each, plus `extra_ms`) then tick.
    Advance(u8, u32),
    /// Chain balance changes by `delta` thousandths (+ or -).
    Balance(i16),
    /// The oldest in-flight order lands (if `fill`), else the sender reports a failure.
    Settle(bool),
    Complete,
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        4 => (0u8..3, 1u64..30 * SOL).prop_map(|(u, s)| Step::Buy(u, s)),
        3 => (0u8..3, 1u16..10_000).prop_map(|(u, b)| Step::Sell(u, b)),
        1 => (0u8..2, 1u16..300).prop_map(|(k, p)| Step::Transfer(k, p)),
        5 => (0u8..4, 0u32..200_000).prop_map(|(n, ms)| Step::Advance(n, ms)),
        1 => (-1000i16..1000).prop_map(Step::Balance),
        4 => any::<bool>().prop_map(Step::Settle),
        1 => Just(Step::Complete),
    ]
}

fn policy() -> impl Strategy<Value = ExitPolicy> {
    (
        prop::bool::ANY,
        0u64..3 * SOL,
        1u32..16,
        0u64..40_000_000,
        prop::sample::select(vec![0u32, 10, 20, 50]),
    )
        .prop_map(|(three, big_lo, tries, spend, cons)| {
            let tp = if three {
                vec![[1.3, 0.25], [2.0, 0.5], [3.0, 1.0]]
            } else {
                vec![[2.0, 0.5], [3.0, 1.0]]
            };
            let o = ExitOverrides {
                tp: Some(tp),
                big_buy_lamports: Some([big_lo, 0]),
                consolidation_pct: Some(cons),
                ..Default::default()
            };
            let mut p = ExitPolicy::default().with_overrides(&o).unwrap();
            p.retry_max_tries = tries;
            p.max_sell_spend_lamports = spend.max(1);
            p
        })
}

struct World {
    pos: Position,
    users: [Pubkey; 3],
    sinks: [Pubkey; 2],
    v: u64,
    slot: u64,
    ms: u64,
    /// Every action emitted, with the slot it was emitted in.
    log: Vec<(u64, ExitAction)>,
    dump_slot: Option<u64>,
}

impl World {
    fn new(p: ExitPolicy) -> World {
        let users = [
            Pubkey::new_from_array([3; 32]),
            Pubkey::new_from_array([4; 32]),
            Pubkey::new_from_array([5; 32]),
        ];
        World {
            pos: Position::pending(
                Pubkey::new_from_array([1; 32]),
                users[2],
                1,
                p,
                BundleSet::new(&[users[0]]),
                users[0],
                FeeBps::DEFAULT,
                SUPPLY,
            ),
            users,
            sinks: [
                Pubkey::new_from_array([8; 32]),
                Pubkey::new_from_array([9; 32]),
            ],
            v: 30 * SOL,
            slot: 100,
            ms: 0,
            log: Vec::new(),
            dump_slot: None,
        }
    }

    fn t(&self) -> u64 {
        (K_INITIAL / self.v as u128) as u64
    }

    fn feed(&mut self, ev: ExitEvent) {
        let before_tokens = self.pos.tokens;
        let acts = decide(
            &mut self.pos,
            &ev,
            Now {
                ms: self.ms,
                slot: self.slot,
            },
        );
        self.check(&acts, before_tokens);
        for a in acts {
            if let ExitAction::DumpSignal { slot, .. } = a {
                self.dump_slot.get_or_insert(slot);
            }
            self.log.push((self.slot, a));
        }
    }

    /// Invariants after every decision.
    fn check(&self, acts: &[ExitAction], _before: u64) {
        let p = &self.pos;
        assert!(
            p.inflight_tokens() <= p.tokens,
            "in flight {} > balance {}",
            p.inflight_tokens(),
            p.tokens
        );
        assert!(p.sell_spend <= p.policy.max_sell_spend_lamports);
        let mut seen_dump_for: Vec<u64> = Vec::new();
        for a in acts {
            if let ExitAction::Sell(o) = a {
                assert!(o.tokens > 0, "empty sell");
                assert!(
                    o.tokens <= p.tokens,
                    "sell {} > balance {}",
                    o.tokens,
                    p.tokens
                );
                assert!(o.try_no <= p.policy.retry_max_tries);
                assert_ne!(p.state, PositionState::Pending);
                if o.kind == SellKind::Dump {
                    seen_dump_for.push(o.trigger_slot);
                }
                if matches!(o.reason, ExitReason::TakeProfit(_)) && o.try_no == 1 {
                    // in one action list a dump for the same trigger slot comes first
                    // (and in fact never coexists with a take-profit)
                    assert!(!seen_dump_for.contains(&o.trigger_slot));
                }
            }
            if p.state == PositionState::Closed {
                assert!(p.pending.iter().all(|x| x.is_none()));
            }
        }
        // distinct orders in flight fit in the balance
        let ids: Vec<u32> = p
            .pending
            .iter()
            .flatten()
            .map(|x| x.order.order_id)
            .collect();
        let mut dedup = ids.clone();
        dedup.dedup();
        assert_eq!(ids.len(), dedup.len());
    }

    fn trade(&mut self, user: Pubkey, v_after: u64) {
        let t0 = self.t();
        let is_buy = v_after >= self.v;
        let sol = self.v.abs_diff(v_after);
        self.v = v_after;
        let t1 = self.t();
        let tr = Trade {
            venue: self.pos.venue,
            mint: self.pos.mint,
            user,
            is_buy,
            sol_amount: sol,
            token_amount: t0.abs_diff(t1),
            fee_lamports: sol / 80,
            reserve_sol: self.v,
            reserve_token: t1,
            slot: self.slot,
        };
        self.feed(ExitEvent::Trade(tr));
    }

    fn run(&mut self, s: &Step) {
        match *s {
            Step::Buy(u, sol) => {
                let v = self.v.saturating_add(sol).min(115 * SOL);
                if v > self.v {
                    self.trade(self.users[u as usize], v);
                }
            }
            Step::Sell(u, bps) => {
                if u == 2 {
                    return; // our sells only happen through Settle
                }
                let room = self.v.saturating_sub(30 * SOL);
                let sol = (room as u128 * bps as u128 / 10_000) as u64;
                if sol > 0 {
                    self.trade(self.users[u as usize], self.v - sol);
                }
            }
            Step::Transfer(k, pm) => {
                let x = TransferEvent {
                    mint: self.pos.mint,
                    from_owner: self.users[1],
                    to_owner: self.sinks[k as usize],
                    amount: SUPPLY / 1000 * pm as u64,
                    slot: self.slot,
                };
                self.feed(ExitEvent::Transfer(x));
            }
            Step::Advance(n, extra) => {
                self.slot += n as u64;
                self.ms += 400 * n as u64 + extra as u64;
                self.feed(ExitEvent::Tick);
            }
            Step::Balance(d) => {
                if !self.pos.state.is_active() {
                    return;
                }
                let b = self.pos.tokens as i128 * (1000 + d as i128) / 1000;
                self.feed(ExitEvent::Balance {
                    tokens: b.max(0) as u64,
                    slot: self.slot,
                });
            }
            Step::Settle(fill) => {
                let Some(p) = self.pos.pending.iter().flatten().next().copied() else {
                    return;
                };
                if !fill {
                    self.feed(ExitEvent::SellFailed {
                        order_id: p.order.order_id,
                    });
                    return;
                }
                let tokens = p.order.tokens.min(self.pos.tokens);
                if self.pos.venue == Venue::PumpSwap {
                    let tr = Trade {
                        venue: Venue::PumpSwap,
                        mint: self.pos.mint,
                        user: self.users[2],
                        is_buy: false,
                        sol_amount: 1,
                        token_amount: tokens,
                        fee_lamports: 0,
                        reserve_sol: self.pos.cur_v,
                        reserve_token: self.pos.cur_t,
                        slot: self.slot,
                    };
                    self.feed(ExitEvent::Trade(tr));
                    return;
                }
                let q = CurveState {
                    virtual_sol: self.v,
                    virtual_token: self.t(),
                    real_sol: 0,
                    real_token: 0,
                    complete: false,
                    creator: self.users[0],
                }
                .quote_sell(tokens, FeeBps::DEFAULT);
                if let Some(q) = q {
                    let me = self.users[2];
                    let v_after = self.v - q.sol_out;
                    self.trade(me, v_after);
                }
            }
            Step::Complete => self.feed(ExitEvent::Complete { slot: self.slot }),
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 400, ..ProptestConfig::default() })]

    #[test]
    fn sells_never_exceed_balance(p in policy(), steps in prop::collection::vec(step(), 1..120)) {
        let mut w = World::new(p);
        // our fill opens the position
        let v = w.v + SOL;
        w.trade(w.users[2], v);
        for s in &steps {
            w.run(s);
        }
        // a closed position never sells again
        let closed_at = w.log.iter().position(|(_, a)| matches!(a, ExitAction::Closed { .. }));
        if let Some(i) = closed_at {
            prop_assert!(!w.log[i + 1..].iter().any(|(_, a)| matches!(a, ExitAction::Sell(_))));
        }
    }

    #[test]
    fn dump_always_precedes_tp_in_a_slot(p in policy(), steps in prop::collection::vec(step(), 1..120)) {
        let mut w = World::new(p);
        let v = w.v + SOL;
        w.trade(w.users[2], v);
        for s in &steps {
            w.run(s);
        }
        if let Some(d) = w.dump_slot {
            // no take-profit is ever triggered on the dump's slot or later
            for (_, a) in &w.log {
                if let ExitAction::Sell(o) = a {
                    if matches!(o.reason, ExitReason::TakeProfit(_)) && o.try_no == 1 {
                        prop_assert!(o.trigger_slot < d, "tp on slot {} after dump at {}", o.trigger_slot, d);
                    }
                }
            }
        }
    }

    /// Both conditions inside one slot, in either order: the dump wins.
    #[test]
    fn dump_and_tp_in_one_slot(pump_first in any::<bool>(), pump in 15u64..80, dump_sol in 1u64..5) {
        let mut w = World::new(ExitPolicy::default());
        let v = w.v + SOL;
        w.trade(w.users[2], v);
        w.slot += 1;
        let dev = w.users[0];
        let stranger = w.users[1];
        let target = w.v + pump * SOL; // >= x2 multiple for pump >= 15
        if pump_first {
            w.trade(stranger, target);
            w.trade(dev, w.v - dump_sol * SOL);
        } else {
            w.trade(dev, w.v - dump_sol.min(1) * SOL / 2 - SOL / 4);
            w.trade(stranger, target);
        }
        let s = w.slot;
        w.slot += 1;
        w.feed(ExitEvent::Tick);
        let dump_i = w.log.iter().position(|(_, a)| matches!(a, ExitAction::Sell(o) if o.kind == SellKind::Dump));
        let tp = w.log.iter().any(|(_, a)| matches!(a, ExitAction::Sell(o) if matches!(o.reason, ExitReason::TakeProfit(_)) && o.trigger_slot == s));
        prop_assert!(dump_i.is_some());
        prop_assert!(!tp);
    }
}
