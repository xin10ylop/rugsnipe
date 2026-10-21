//! Trigger-by-trigger scenarios on a synthetic constant-product curve, plus
//! real `TradeEvent`s from the pump-codec fixtures.

use pump_codec::curve::{CurveState, FeeBps, INITIAL_VIRTUAL_TOKEN_RESERVES, K_INITIAL};
use pump_codec::events::{decode_event_payload, scan_program_data_logs, EventSource, PumpEvent};
use pump_codec::Pubkey;
use rugsnipe_exit::*;

const SUPPLY: u64 = 1_000_000_000_000_000;

struct Sim {
    pos: Position,
    mint: Pubkey,
    me: Pubkey,
    dev: Pubkey,
    v: u64,
    slot: u64,
    ms: u64,
}

impl Sim {
    fn new(policy: ExitPolicy) -> Sim {
        let (mint, me, dev) = (
            Pubkey::new_from_array([1; 32]),
            Pubkey::new_from_array([2; 32]),
            Pubkey::new_from_array([3; 32]),
        );
        let pos = Position::pending(
            mint,
            me,
            7,
            policy,
            BundleSet::new(&[dev]),
            dev,
            FeeBps::DEFAULT,
            SUPPLY,
        );
        Sim {
            pos,
            mint,
            me,
            dev,
            v: 30_000_000_000,
            slot: 1000,
            ms: 1_000_000,
        }
    }

    fn t(&self) -> u64 {
        (K_INITIAL / self.v as u128) as u64
    }

    fn now(&self) -> Now {
        Now {
            ms: self.ms,
            slot: self.slot,
        }
    }

    fn ev(&mut self, ev: ExitEvent) -> Vec<ExitAction> {
        let now = self.now();
        decide(&mut self.pos, &ev, now)
    }

    /// A trade by `user` that moves virtual SOL to `v_after`.
    fn trade(&mut self, user: Pubkey, v_after: u64) -> Vec<ExitAction> {
        let t0 = self.t();
        let is_buy = v_after >= self.v;
        let sol = self.v.abs_diff(v_after);
        self.v = v_after;
        let t1 = self.t();
        let tr = Trade {
            venue: Venue::Curve,
            mint: self.mint,
            user,
            is_buy,
            sol_amount: sol,
            token_amount: t0.abs_diff(t1),
            fee_lamports: sol / 80,
            reserve_sol: self.v,
            reserve_token: t1,
            slot: self.slot,
        };
        self.ev(ExitEvent::Trade(tr))
    }

    fn open(&mut self, v_fill: u64) -> Vec<ExitAction> {
        let me = self.me;
        // our buy is 1 SOL into the curve
        self.v = self.v.min(v_fill - SOL);
        self.trade(me, v_fill)
    }

    fn other(&mut self, v_after: u64) -> Vec<ExitAction> {
        self.trade(Pubkey::new_from_array([9; 32]), v_after)
    }

    /// Our own sell of `tokens` lands.
    fn fill(&mut self, tokens: u64) -> Vec<ExitAction> {
        let s = CurveState {
            virtual_sol: self.v,
            virtual_token: self.t(),
            real_sol: 0,
            real_token: 0,
            complete: false,
            creator: self.dev,
        };
        let q = s.quote_sell(tokens, FeeBps::DEFAULT).unwrap();
        self.v -= q.sol_out;
        let tr = Trade {
            venue: Venue::Curve,
            mint: self.mint,
            user: self.me,
            is_buy: false,
            sol_amount: q.sol_out,
            token_amount: tokens,
            fee_lamports: q.sol_out - q.net,
            reserve_sol: self.v,
            reserve_token: self.t(),
            slot: self.slot,
        };
        self.ev(ExitEvent::Trade(tr))
    }

    fn next_slot(&mut self, n: u64) {
        self.slot += n;
        self.ms += 400 * n;
    }

    fn tick(&mut self) -> Vec<ExitAction> {
        self.ev(ExitEvent::Tick)
    }
}

fn sells(a: &[ExitAction]) -> Vec<SellOrder> {
    a.iter()
        .filter_map(|x| match x {
            ExitAction::Sell(o) => Some(*o),
            _ => None,
        })
        .collect()
}

const SOL: u64 = 1_000_000_000;

#[test]
fn opens_only_on_own_fill_never_on_others() {
    let mut s = Sim::new(ExitPolicy::default());
    assert!(s.other(31 * SOL).is_empty());
    assert_eq!(s.pos.state, PositionState::Pending);
    s.next_slot(1);
    let a = s.open(32 * SOL);
    assert!(matches!(
        a[0],
        ExitAction::Opened {
            origin: Origin::Intent,
            ..
        }
    ));
    assert_eq!(s.pos.state, PositionState::Open);
    assert_eq!(s.pos.v_fill, 32 * SOL);
    assert!(s.pos.tokens > 0 && s.pos.cost > 0);
}

#[test]
fn take_profit_ladder_partial_then_rest() {
    let mut s = Sim::new(ExitPolicy::default());
    s.open(30 * SOL);
    let held = s.pos.tokens;
    // price x2.25 (v x1.5) inside the slot: nothing until the slot closes
    assert!(sells(&s.other(45 * SOL)).is_empty());
    s.next_slot(1);
    let a = s.tick();
    let o = sells(&a);
    assert_eq!(o.len(), 1);
    assert_eq!(o[0].reason, ExitReason::TakeProfit(0));
    assert_eq!(o[0].kind, SellKind::Normal);
    assert_eq!(o[0].tokens, held / 2);
    assert!(!o[0].close_account);
    assert_eq!(o[0].trigger_slot, 1000);
    // bound = quote at V_trigger x 0.85
    let q = CurveState {
        virtual_sol: 45 * SOL,
        virtual_token: (K_INITIAL / (45 * SOL) as u128) as u64,
        real_sol: 0,
        real_token: 0,
        complete: false,
        creator: s.dev,
    }
    .quote_sell(held / 2, FeeBps::DEFAULT)
    .unwrap()
    .net;
    assert_eq!(o[0].min_sol_output, q * 85 / 100);
    s.fill(held / 2);
    assert_eq!(s.pos.state, PositionState::Open);
    // x3.24 (v 54): second level sells the rest and closes the account
    s.other(54 * SOL);
    s.next_slot(1);
    let o = sells(&s.tick());
    assert_eq!(o[0].reason, ExitReason::TakeProfit(1));
    assert_eq!(o[0].tokens, s.pos.tokens);
    assert!(o[0].close_account);
    let a = s.fill(o[0].tokens);
    assert!(matches!(
        a.last(),
        Some(ExitAction::Closed {
            cause: CloseCause::Sold(ExitReason::TakeProfit(1)),
            ..
        })
    ));
}

#[test]
fn jumping_two_levels_combines_fractions() {
    let p = ExitPolicy::default()
        .with_overrides(&ExitOverrides {
            tp: Some(vec![[2.0, 0.5], [3.0, 0.5], [6.0, 1.0]]),
            ..Default::default()
        })
        .unwrap();
    let mut s = Sim::new(p);
    s.open(30 * SOL);
    let held = s.pos.tokens;
    s.other(55 * SOL); // x3.36 crosses levels 0 and 1
    s.next_slot(1);
    let o = sells(&s.tick());
    assert_eq!(o[0].reason, ExitReason::TakeProfit(1));
    assert_eq!(o[0].tokens, (held as u128 * 7500 / 10_000) as u64);
    assert_eq!(s.pos.tp_done, 0b011);
}

#[test]
fn trailing_after_activation() {
    let mut s = Sim::new(ExitPolicy::default());
    s.open(30 * SOL);
    s.other(37 * SOL); // x1.52 arms the trail, below TP 2.0
    s.next_slot(1);
    assert!(sells(&s.tick()).is_empty());
    assert!(s.pos.trail_armed);
    // 1.52 * 0.75 = 1.14 -> v <= 32.04
    s.other(33 * SOL);
    s.next_slot(1);
    assert!(sells(&s.tick()).is_empty());
    s.other(32 * SOL);
    s.next_slot(1);
    let o = sells(&s.tick());
    assert_eq!(o[0].reason, ExitReason::Trailing);
    assert!(o[0].close_account);
}

#[test]
fn phantom_stop_loss_after_delay() {
    let mut s = Sim::new(ExitPolicy::default());
    s.open(40 * SOL);
    s.other(30 * SOL); // x0.5625 <= 0.6 but before after_ms
    s.next_slot(1);
    assert!(sells(&s.tick()).is_empty());
    s.ms += 10_000;
    s.next_slot(1);
    let o = sells(&s.tick());
    assert_eq!(o[0].reason, ExitReason::PhantomSl);
}

#[test]
fn no_activity_and_max_hold() {
    let mut s = Sim::new(ExitPolicy::default());
    s.open(31 * SOL);
    s.ms += 149_000;
    s.next_slot(1);
    assert!(sells(&s.tick()).is_empty());
    s.ms += 600;
    s.next_slot(1);
    assert_eq!(sells(&s.tick())[0].reason, ExitReason::NoActivity);

    let mut s = Sim::new(ExitPolicy::default());
    s.open(31 * SOL);
    let mut fired = None;
    for _ in 0..20 {
        s.ms += 100_000;
        s.next_slot(1);
        let held = s.ms - s.pos.opened_ms;
        let o = sells(&s.other(31 * SOL + s.slot)); // keep trading, flat price
        if let Some(o) = o.first() {
            fired = Some((o.reason, held));
            break;
        }
    }
    let (reason, held) = fired.expect("max hold fired");
    assert_eq!(reason, ExitReason::MaxHold);
    assert!((1_800_000..1_900_000).contains(&held));
}

#[test]
fn bundle_sell_dumps_immediately_and_supersedes_tp() {
    let mut s = Sim::new(ExitPolicy::default());
    s.open(30 * SOL);
    let held = s.pos.tokens;
    s.other(45 * SOL);
    s.next_slot(1);
    let tp = sells(&s.tick());
    assert_eq!(tp[0].reason, ExitReason::TakeProfit(0));
    // same slot: the dev dumps 2 SOL while the TP order is in flight
    let dev = s.dev;
    let a = s.trade(dev, 43 * SOL);
    assert!(matches!(
        a[0],
        ExitAction::DumpSignal {
            reason: ExitReason::BundleSell,
            ..
        }
    ));
    let o = sells(&a);
    // TP order re-sent at dump bounds, the rest on a new dump order
    assert_eq!(o.len(), 2);
    assert_eq!(o[0].order_id, tp[0].order_id);
    assert_eq!(o[0].kind, SellKind::Dump);
    assert_eq!(o[0].try_no, 2);
    assert_eq!(o[1].kind, SellKind::Dump);
    assert_eq!(o[1].reason, ExitReason::BundleSell);
    assert_eq!(o[0].tokens + o[1].tokens, held);
    assert!(!o[1].close_account);
    // dump bound is the floor quote x 0.98
    let floor = CurveState::fresh(s.dev)
        .quote_sell(o[1].tokens, FeeBps::DEFAULT)
        .unwrap()
        .net;
    assert_eq!(o[1].min_sol_output, floor * 98 / 100);
    assert_eq!(s.pos.inflight_tokens(), held);
}

#[test]
fn small_bundle_sells_and_strangers_do_not_dump() {
    let mut s = Sim::new(ExitPolicy::default());
    s.open(35 * SOL);
    let dev = s.dev;
    assert!(s.trade(dev, 35 * SOL - SOL / 20).is_empty()); // 0.05 SOL < 0.1 min
    assert!(s.other(30 * SOL).is_empty()); // a stranger's 5 SOL sell
    assert!(s.pos.dumped.is_none());
}

#[test]
fn dump_signal_before_open_then_late_fill_sells_at_once() {
    let mut s = Sim::new(ExitPolicy::default());
    let dev = s.dev;
    s.v = 40 * SOL;
    let a = s.trade(dev, 38 * SOL);
    assert_eq!(a.len(), 1);
    assert!(matches!(a[0], ExitAction::DumpSignal { .. }));
    assert_eq!(s.pos.state, PositionState::Pending);
    s.next_slot(1);
    let a = s.open(39 * SOL); // our variant landed late
    assert!(matches!(a[0], ExitAction::Opened { .. }));
    let o = sells(&a);
    assert_eq!(o.len(), 1);
    assert_eq!(o[0].kind, SellKind::Dump);
    assert_eq!(o[0].tokens, s.pos.tokens);
    // only one DumpSignal ever
    assert!(!a.iter().any(|x| matches!(x, ExitAction::DumpSignal { .. })));
}

#[test]
fn big_buy_window() {
    let p = ExitPolicy::default()
        .with_overrides(&ExitOverrides {
            big_buy_lamports: Some([5 * SOL, 20 * SOL]),
            ..Default::default()
        })
        .unwrap();
    let mut s = Sim::new(p);
    s.open(30 * SOL);
    assert!(s.other(34 * SOL).is_empty()); // 4 SOL: below the window
    assert!(s.other(60 * SOL).is_empty()); // 26 SOL: above the window
    let a = s.other(66 * SOL); // 6 SOL: inside, and price is far above TP
    assert!(matches!(
        a[0],
        ExitAction::DumpSignal {
            reason: ExitReason::BigBuy,
            ..
        }
    ));
    assert_eq!(sells(&a)[0].kind, SellKind::Dump);
    s.next_slot(1);
    // slot close: TP is suppressed, the position is exiting at dump bounds
    assert!(sells(&s.tick()).is_empty());
}

#[test]
fn consolidation_within_two_slots() {
    let mut s = Sim::new(ExitPolicy::default());
    s.open(31 * SOL);
    let sink = Pubkey::new_from_array([5; 32]);
    let x = |amount, slot| {
        ExitEvent::Transfer(TransferEvent {
            mint: Pubkey::new_from_array([1; 32]),
            from_owner: Pubkey::new_from_array([6; 32]),
            to_owner: sink,
            amount,
            slot,
        })
    };
    // 15% then (3 slots later) 10%: outside the window
    assert!(s.ev(x(150_000_000_000_000, 1000)).is_empty());
    s.next_slot(3);
    assert!(s.ev(x(100_000_000_000_000, 1003)).is_empty());
    // another 10% one slot later: 20% within 2 slots
    s.next_slot(1);
    let a = s.ev(x(100_000_000_000_000, 1004));
    assert!(matches!(
        a[0],
        ExitAction::DumpSignal {
            reason: ExitReason::Consolidation,
            ..
        }
    ));
}

#[test]
fn retry_schedule_escalation_and_exhaustion() {
    let mut s = Sim::new(ExitPolicy::default());
    s.open(30 * SOL);
    s.other(45 * SOL);
    s.next_slot(1);
    let first = sells(&s.tick())[0];
    assert_eq!((first.try_no, first.tip_lamports), (1, 50_000));
    s.next_slot(1);
    assert!(sells(&s.tick()).is_empty(), "retry only every 2 slots");
    s.next_slot(1);
    let r2 = sells(&s.tick())[0];
    assert_eq!(
        (r2.order_id, r2.try_no, r2.tip_lamports),
        (first.order_id, 2, 100_000)
    );
    assert_eq!(r2.kind, SellKind::Normal);
    assert_eq!(
        r2.min_sol_output, first.min_sol_output,
        "bound keeps V_trigger"
    );
    s.next_slot(2);
    let r3 = sells(&s.tick())[0];
    assert_eq!(
        (r3.try_no, r3.tip_lamports, r3.kind),
        (3, 200_000, SellKind::Normal)
    );
    s.next_slot(2);
    let r4 = sells(&s.tick())[0];
    assert_eq!(
        (r4.try_no, r4.kind),
        (4, SellKind::Dump),
        "escalates after 3 tries"
    );
    // keep failing: tips double until the 0.02 SOL spend cap, then the alert
    let mut alerts = Vec::new();
    let mut last_try = 4;
    for _ in 0..20 {
        s.next_slot(2);
        for a in s.tick() {
            match a {
                ExitAction::Sell(o) => last_try = o.try_no,
                ExitAction::Alert(al) => alerts.push(al),
                _ => {}
            }
        }
    }
    assert!(s.pos.sell_spend <= 20_000_000);
    assert!(alerts
        .iter()
        .any(|a| matches!(a, Alert::SellFailing { tries: 5, .. })));
    assert!(alerts
        .iter()
        .any(|a| matches!(a, Alert::SellExhausted { .. })));
    assert!(last_try <= 15);
    assert!(s.pos.pending[0].unwrap().exhausted);
}

#[test]
fn max_tries_caps_retries() {
    let p = ExitPolicy {
        max_sell_spend_lamports: u64::MAX / 4,
        retry_max_tries: 4,
        ..ExitPolicy::default()
    };
    let mut s = Sim::new(p);
    s.open(30 * SOL);
    s.ms += 200_000;
    s.next_slot(1);
    assert_eq!(sells(&s.tick())[0].reason, ExitReason::NoActivity);
    let mut tries = vec![];
    let mut exhausted = false;
    for _ in 0..10 {
        s.next_slot(2);
        for a in s.tick() {
            match a {
                ExitAction::Sell(o) => tries.push(o.try_no),
                ExitAction::Alert(Alert::SellExhausted { tries, .. }) => {
                    exhausted = true;
                    assert_eq!(tries, 4);
                }
                _ => {}
            }
        }
    }
    assert_eq!(tries, vec![2, 3, 4]);
    assert!(exhausted);
}

#[test]
fn sell_failed_retries_immediately() {
    let mut s = Sim::new(ExitPolicy::default());
    s.open(30 * SOL);
    s.ms += 200_000;
    s.next_slot(1);
    let o = sells(&s.tick())[0];
    let a = s.ev(ExitEvent::SellFailed {
        order_id: o.order_id,
    });
    assert_eq!(sells(&a)[0].try_no, 2);
}

#[test]
fn completion_moves_orders_to_pumpswap() {
    let mut s = Sim::new(ExitPolicy::default());
    s.open(30 * SOL);
    s.other(115 * SOL);
    s.next_slot(1);
    let tp = sells(&s.tick());
    assert_eq!(tp[0].venue, Venue::Curve);
    let a = s.ev(ExitEvent::Complete { slot: s.slot });
    assert!(matches!(
        a[0],
        ExitAction::VenueChanged {
            venue: Venue::PumpSwap
        }
    ));
    let o = sells(&a);
    assert_eq!(o[0].venue, Venue::PumpSwap);
    assert_eq!(o[0].order_id, tp[0].order_id);
    assert!(o[0].min_sol_output > 0);
    assert_eq!(s.pos.venue, Venue::PumpSwap);
    // pool events drive the price after migration
    s.ev(ExitEvent::PoolState {
        reserve_sol: 80 * SOL,
        reserve_token: 210_000_000_000_000,
        slot: s.slot,
    });
    assert!(s.pos.pool_known);
    assert!(s.pos.mult_bps() > 100_000);
}

#[test]
fn orphan_balance_resync_and_external_close() {
    let mut s = Sim::new(ExitPolicy::default());
    s.other(33 * SOL);
    let a = s.ev(ExitEvent::Balance {
        tokens: 5_000_000_000_000,
        slot: s.slot,
    });
    assert!(matches!(
        a[0],
        ExitAction::Opened {
            origin: Origin::Orphan,
            ..
        }
    ));
    assert_eq!(s.pos.v_fill, 33 * SOL);
    // a sell in flight, then the chain says we hold less: the order shrinks
    s.ms += 200_000;
    s.next_slot(1);
    let o = sells(&s.tick())[0];
    assert_eq!(o.tokens, 5_000_000_000_000);
    s.ev(ExitEvent::Balance {
        tokens: 3_000_000_000_000,
        slot: s.slot,
    });
    assert_eq!(s.pos.inflight_tokens(), 3_000_000_000_000);
    s.next_slot(2);
    assert_eq!(sells(&s.tick())[0].tokens, 3_000_000_000_000);
    let a = s.ev(ExitEvent::Balance {
        tokens: 0,
        slot: s.slot,
    });
    assert!(matches!(
        a[0],
        ExitAction::Closed {
            cause: CloseCause::BalanceZero,
            ..
        }
    ));
    assert!(
        s.ev(ExitEvent::Tick).is_empty(),
        "closed positions are inert"
    );
}

#[test]
fn unpriced_orphan_sells_blind_on_the_timers() {
    // A balance adopted on a mint that never trades again: no price is ever
    // seen (v_fill stays 0), so the price triggers cannot fire. The idle
    // timer still sells the whole balance with a zero bound.
    let mut s = Sim::new(ExitPolicy {
        no_activity_ms: 1_000,
        ..ExitPolicy::default()
    });
    let a = s.ev(ExitEvent::Balance {
        tokens: 705_990_620_346,
        slot: s.slot,
    });
    assert!(matches!(
        a[0],
        ExitAction::Opened {
            origin: Origin::Orphan,
            v_fill: 0,
            ..
        }
    ));
    assert_eq!(s.pos.v_fill, 0);
    // before the timer: nothing
    s.ms += 500;
    s.next_slot(1);
    assert!(sells(&s.tick()).is_empty());
    // after it: one order for everything, no price floor
    s.ms += 600;
    s.next_slot(1);
    let o = sells(&s.tick());
    assert_eq!(o.len(), 1);
    assert_eq!(o[0].tokens, 705_990_620_346);
    assert_eq!(o[0].min_sol_output, 0);
    assert_eq!(o[0].reason, ExitReason::NoActivity);
    assert!(o[0].close_account);
    assert_eq!(s.pos.state, PositionState::Exiting);
    // the fill closes it like any other sell
    let a = s.ev(ExitEvent::Balance {
        tokens: 0,
        slot: s.slot,
    });
    assert!(matches!(
        a[0],
        ExitAction::Closed {
            cause: CloseCause::BalanceZero,
            ..
        }
    ));
}

#[test]
fn unpriced_orphan_hard_exit_on_max_hold_slots() {
    let mut s = Sim::new(ExitPolicy {
        no_activity_ms: 0,
        max_hold_ms: 0,
        max_hold_slots: 5,
        ..ExitPolicy::default()
    });
    s.ev(ExitEvent::Balance {
        tokens: 1_000,
        slot: s.slot,
    });
    s.next_slot(4);
    assert!(sells(&s.tick()).is_empty());
    s.next_slot(1);
    let o = sells(&s.tick());
    assert_eq!(o.len(), 1);
    assert_eq!(o[0].reason, ExitReason::MaxHold);
    assert_eq!(o[0].min_sol_output, 0);
}

#[test]
fn other_mints_are_ignored() {
    let mut s = Sim::new(ExitPolicy::default());
    s.open(30 * SOL);
    let mut t = Trade {
        venue: Venue::Curve,
        mint: Pubkey::new_from_array([42; 32]),
        user: s.dev,
        is_buy: false,
        sol_amount: 10 * SOL,
        token_amount: 1,
        fee_lamports: 0,
        reserve_sol: 1,
        reserve_token: 1,
        slot: s.slot,
    };
    assert!(s.ev(ExitEvent::Trade(t)).is_empty());
    t.mint = s.mint;
    assert!(!s.ev(ExitEvent::Trade(t)).is_empty());
}

#[test]
fn position_columns_serialize() {
    let s = Sim::new(ExitPolicy::default());
    let b: serde_json::Value = serde_json::from_str(&s.pos.bundle_json()).unwrap();
    assert_eq!(b[0], s.dev.to_string());
    let p: ExitPolicy = serde_json::from_str(&s.pos.policy_json()).unwrap();
    assert_eq!(p, ExitPolicy::default());
    let full = serde_json::to_string(&s.pos).unwrap();
    let back: Position = serde_json::from_str(&full).unwrap();
    assert_eq!(back, s.pos);
}

#[test]
fn toml_exit_default_table_parses() {
    let text = r#"
tp = [[2.0, 0.5], [3.0, 1.0]]
tp_slip_bps = 1500
trail = { activate = 1.5, retrace_bps = 2500 }
phantom_sl = { after_ms = 10_000, bps = 4000 }
no_activity_ms = 150_000
max_hold_ms = 1_800_000
bundle_sell_min_lamports = 100_000_000
consolidation_pct = 20
big_buy_lamports = [0, 0]
sell_retry = { every_slots = 2, max_tries = 15, tip_mult = 2.0 }
"#;
    let c: ExitConfig = toml::from_str(text).unwrap();
    assert_eq!(c, ExitConfig::default());
    assert_eq!(c.to_policy(20_000_000).unwrap(), ExitPolicy::default());
    let o: ExitOverrides = toml::from_str("tp = [[1.8, 1.0]]\nmax_hold_ms = 600_000").unwrap();
    let p = ExitPolicy::default().with_overrides(&o).unwrap();
    assert_eq!((p.tp.len(), p.max_hold_ms), (1, 600_000));
    assert!(toml::from_str::<ExitOverrides>("bogus = 1").is_err());
}

// ------------------------------------------------- real fixtures (pump-codec)
fn fixture_trades(name: &str) -> (Vec<(u64, Trade)>, bool) {
    let path = format!(
        "{}/../pump-codec/fixtures/tx/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let r = &v["result"];
    let slot = r["slot"].as_u64().unwrap();
    let logs: Vec<String> = r["meta"]["logMessages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l.as_str().unwrap().to_string())
        .collect();
    let scan = scan_program_data_logs(&logs).unwrap();
    let mut out = Vec::new();
    let mut complete = false;
    for d in &scan.data {
        let Some(src) = EventSource::from_program(&d.program) else {
            continue;
        };
        match decode_event_payload(src, &d.data).unwrap() {
            PumpEvent::Trade(t) => out.push((slot, Trade::from_event(&t, slot))),
            PumpEvent::Complete(_) => complete = true,
            _ => {}
        }
    }
    (out, complete)
}

#[test]
fn real_trade_event_opens_and_completion_switches_venue() {
    // create + buy that completes the curve in one transaction
    let (trades, complete) = fixture_trades("create_buy_complete");
    assert!(complete && !trades.is_empty());
    let (slot, buy) = trades[0];
    let mut pos = Position::pending(
        buy.mint,
        buy.user,
        1,
        ExitPolicy::default(),
        BundleSet::default(),
        Pubkey::default(),
        FeeBps::DEFAULT,
        SUPPLY,
    );
    let now = Now { ms: 0, slot };
    let a = decide(&mut pos, &ExitEvent::Trade(buy), now);
    assert!(
        matches!(a[0], ExitAction::Opened { origin: Origin::Intent, tokens, .. } if tokens == buy.token_amount)
    );
    assert_eq!(pos.cost, buy.sol_amount + buy.fee_lamports);
    assert!(pos.cost > buy.sol_amount, "fees are part of the cost");
    let a = decide(&mut pos, &ExitEvent::Complete { slot }, now);
    assert!(matches!(
        a[0],
        ExitAction::VenueChanged {
            venue: Venue::PumpSwap
        }
    ));
}

#[test]
fn real_sell_by_bundle_wallet_is_a_dump() {
    let (trades, _) = fixture_trades("legacy_sell16");
    let (slot, sell) = trades[0];
    assert!(!sell.is_buy);
    let me = Pubkey::new_from_array([77; 32]);
    let mut pos = Position::pending(
        sell.mint,
        me,
        1,
        ExitPolicy {
            bundle_sell_min_lamports: sell.sol_amount,
            ..ExitPolicy::default()
        },
        BundleSet::new(&[sell.user]),
        Pubkey::default(),
        FeeBps::DEFAULT,
        SUPPLY,
    );
    let a = decide(
        &mut pos,
        &ExitEvent::Balance {
            tokens: 1_000_000_000_000,
            slot,
        },
        Now { ms: 0, slot },
    );
    assert!(matches!(a[0], ExitAction::Opened { .. }));
    let a = decide(&mut pos, &ExitEvent::Trade(sell), Now { ms: 1, slot });
    assert!(matches!(
        a[0],
        ExitAction::DumpSignal {
            reason: ExitReason::BundleSell,
            ..
        }
    ));
    let o = sells(&a)[0];
    assert_eq!(o.tokens, 1_000_000_000_000);
    assert!(o.min_sol_output > 0);
    let _ = INITIAL_VIRTUAL_TOKEN_RESERVES;
}
