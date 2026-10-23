//! The lead policy's exit knobs (docs/04 sections 2 and 6): take-profit on
//! the realised value, triggers on the first trade that shows them, the
//! insider-sell set, the phantom stop measured from the post-bundle price
//! and the hard exit at landing slot + N.

use pump_codec::curve::{CurveState, FeeBps, K_INITIAL};
use pump_codec::Pubkey;
use rugsnipe_exit::*;

const SUPPLY: u64 = 1_000_000_000_000_000;

/// `[exit.default]` of `config/paper-lead.example.toml`.
const LEAD: &str = r#"
tp = [[1.15, 1.0]]
tp_basis = "value"
fire_on = "trade"
insider_sell = "bundle"
tp_slip_bps = 2500
trail = { activate = 0.0, retrace_bps = 0 }
phantom_sl = { after_ms = 1000, drop = 0.15 }
no_activity_ms = 1000
max_hold_ms = 0
max_hold_slots = 5
bundle_sell_min_lamports = 0
consolidation_pct = 0
big_buy_lamports = [0, 0]
sell_retry = { every_slots = 2, max_tries = 15, tip_mult = 2.0 }
"#;

fn lead() -> ExitPolicy {
    let c: ExitConfig = toml::from_str(LEAD).unwrap();
    c.to_policy(20_000_000).unwrap()
}

/// A curve position fed by hand: `v` is the historical virtual SOL (the
/// curve without us); trades by others move it, our tokens stay outstanding
/// (the paper counterfactual, `t' = t_hist - ours`).
struct Lead {
    pos: Position,
    mint: Pubkey,
    me: Pubkey,
    v: u64,
    slot: u64,
}

impl Lead {
    fn new(policy: ExitPolicy, bundle: &[Pubkey]) -> Lead {
        let mint = Pubkey::new_from_array([1; 32]);
        let me = Pubkey::new_from_array([2; 32]);
        let creator = Pubkey::new_from_array([3; 32]);
        Lead {
            pos: Position::pending(
                mint,
                me,
                7,
                policy,
                BundleSet::new(bundle),
                creator,
                FeeBps::DEFAULT,
                SUPPLY,
            ),
            mint,
            me,
            v: 40_000_000_000,
            slot: 100,
        }
    }

    fn t_hist(&self) -> u64 {
        (K_INITIAL / self.v as u128) as u64
    }

    /// The reserves with our tokens outstanding.
    fn with_us(&self, v_hist: u64) -> (u64, u64) {
        let t = (K_INITIAL / v_hist as u128) as u64 - self.pos.tokens;
        ((K_INITIAL / t as u128) as u64, t)
    }

    fn now(&self) -> Now {
        Now {
            ms: self.slot * 200,
            slot: self.slot,
        }
    }

    /// Our buy of `spend` lamports at the current (post-bundle) state.
    fn buy(&mut self, spend: u64) -> Vec<ExitAction> {
        let c = CurveState {
            virtual_sol: self.v,
            virtual_token: self.t_hist(),
            real_sol: 0,
            real_token: u64::MAX,
            complete: false,
            creator: self.pos.creator,
        };
        let q = c.quote_buy_exact_sol_in(spend, FeeBps::DEFAULT).unwrap();
        let t_after = self.t_hist() - q.tokens;
        let tr = Trade {
            venue: Venue::Curve,
            mint: self.mint,
            user: self.me,
            is_buy: true,
            sol_amount: q.sol_cost,
            token_amount: q.tokens,
            fee_lamports: q.protocol_fee + q.creator_fee,
            reserve_sol: (K_INITIAL / t_after as u128) as u64,
            reserve_token: t_after,
            slot: self.slot,
        };
        let now = self.now();
        decide(&mut self.pos, &ExitEvent::Trade(tr), now)
    }

    /// Someone else trades: the historical curve moves to `v_after`.
    fn other(&mut self, user: Pubkey, v_after: u64) -> Vec<ExitAction> {
        let is_buy = v_after >= self.v;
        let sol = self.v.abs_diff(v_after);
        let t0 = self.t_hist();
        self.v = v_after;
        let (rv, rt) = self.with_us(v_after);
        let tr = Trade {
            venue: Venue::Curve,
            mint: self.mint,
            user,
            is_buy,
            sol_amount: sol,
            token_amount: t0.abs_diff(self.t_hist()),
            fee_lamports: sol / 80,
            reserve_sol: rv,
            reserve_token: rt,
            slot: self.slot,
        };
        let now = self.now();
        decide(&mut self.pos, &ExitEvent::Trade(tr), now)
    }

    fn tick(&mut self, slot: u64) -> Vec<ExitAction> {
        self.slot = slot;
        let now = self.now();
        decide(&mut self.pos, &ExitEvent::Tick, now)
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

#[test]
fn lead_toml_parses_to_the_docs_04_policy() {
    let p = lead();
    assert_eq!(
        p.tp.levels(),
        &[TpLevel {
            mult_bps: 11_500,
            frac_bps: 10_000
        }]
    );
    assert_eq!(p.tp_basis, TpBasis::Value);
    assert_eq!(p.fire_on, FireOn::Trade);
    assert_eq!(p.insider_sell, InsiderSell::Bundle);
    assert_eq!(
        (p.phantom_after_ms, p.phantom_bps, p.phantom_from),
        (1000, 1500, PhantomFrom::Entry)
    );
    assert_eq!(
        (p.no_activity_ms, p.max_hold_ms, p.max_hold_slots),
        (1000, 0, 5)
    );
    assert_eq!(p.bundle_sell_min_lamports, 0);
    // round trip through the float form keeps every knob
    let back = ExitConfig::from_policy(&p).to_policy(p.max_sell_spend_lamports);
    assert_eq!(back.unwrap(), p);
    // the defaults are unchanged (docs/06 section 6)
    let d = ExitPolicy::default();
    assert_eq!(
        (
            d.tp_basis,
            d.fire_on,
            d.insider_sell,
            d.phantom_from,
            d.max_hold_slots
        ),
        (
            TpBasis::Price,
            FireOn::SlotClose,
            InsiderSell::Bundle,
            PhantomFrom::Fill,
            0
        )
    );
    // bps and drop together are refused
    let bad = LEAD.replace("drop = 0.15", "drop = 0.15, bps = 1500");
    let c: ExitConfig = toml::from_str(&bad).unwrap();
    assert!(c.to_policy(20_000_000).is_err());
    // an old policy JSON (no new fields) still deserializes with the defaults
    let mut js = serde_json::to_value(ExitPolicy::default()).unwrap();
    for k in [
        "tp_basis",
        "fire_on",
        "insider_sell",
        "phantom_from",
        "max_hold_slots",
    ] {
        js.as_object_mut().unwrap().remove(k);
    }
    let old: ExitPolicy = serde_json::from_value(js).unwrap();
    assert_eq!(old, ExitPolicy::default());
}

#[test]
fn value_take_profit_fires_on_the_trade_that_crosses_it() {
    let mut l = Lead::new(lead(), &[]);
    l.buy(500_000_000);
    assert_eq!(l.pos.state, PositionState::Open);
    assert_eq!(l.pos.cost, 500_000_000);
    // +10% in price is not +15% of value after two fees and our impact
    l.slot = 101;
    let v10 = (l.v as f64 * 1.10f64.sqrt()) as u64;
    assert!(sells(&l.other(Pubkey::new_unique(), v10)).is_empty());
    assert!(l.pos.mult_bps() >= 10_000);
    assert!(l.pos.value_bps().unwrap() < 11_500);
    // a buy that lifts the value above 1.15 x cost sells everything at once,
    // in the same slot (fire_on = "trade")
    let mut v = l.v;
    let order = loop {
        v += 200_000_000;
        let s = sells(&l.other(Pubkey::new_unique(), v));
        if let Some(o) = s.first() {
            break *o;
        }
    };
    assert_eq!(order.reason, ExitReason::TakeProfit(0));
    assert_eq!(order.trigger_slot, 101);
    assert_eq!(order.tokens, l.pos.tokens);
    let vb = l.pos.value_bps().unwrap();
    assert!((11_500..11_700).contains(&vb), "value {vb}");
    // price basis on the same path would have fired earlier
    let mut p = lead();
    p.tp_basis = TpBasis::Price;
    let mut l2 = Lead::new(p, &[]);
    l2.buy(500_000_000);
    l2.slot = 101;
    let mut v = l2.v;
    let first = loop {
        v += 200_000_000;
        if !sells(&l2.other(Pubkey::new_unique(), v)).is_empty() {
            break v;
        }
    };
    assert!(first < l.v, "price TP at {first}, value TP at {}", l.v);
}

#[test]
fn hard_exit_at_landing_slot_plus_five_and_idle_counts_from_the_landing() {
    // no trade at all: idle fires first (1 s = 5 slots at 200 ms here)
    let mut l = Lead::new(lead(), &[]);
    l.buy(500_000_000);
    assert!(sells(&l.tick(104)).is_empty());
    let s = sells(&l.tick(105));
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].reason, ExitReason::NoActivity);
    // with steady trading, the hard exit fires at landing slot + 5
    let mut l = Lead::new(lead(), &[]);
    l.buy(500_000_000);
    for slot in 101..=104 {
        l.slot = slot;
        assert!(sells(&l.other(Pubkey::new_unique(), l.v + 10_000_000)).is_empty());
    }
    l.slot = 105;
    let s = sells(&l.other(Pubkey::new_unique(), l.v + 10_000_000));
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!(s[0].reason, ExitReason::MaxHold);
}

#[test]
fn a_balance_report_from_a_node_ahead_of_the_feed_does_not_freeze_the_hold_timers() {
    // 2026-10-22: the lane's balance report carries the RPC node's slot;
    // when that ran ahead of the feed the exit clock jumped past it and the
    // hold timers were not evaluated until the feed caught up (dead
    // launches closed at +11..+14 instead of +6)
    let mut l = Lead::new(lead(), &[]);
    l.buy(500_000_000);
    l.slot = 101;
    let now = l.now();
    let tokens = l.pos.tokens;
    assert!(sells(&decide(
        &mut l.pos,
        &ExitEvent::Balance { tokens, slot: 120 },
        now
    ))
    .is_empty());
    assert!(sells(&l.tick(104)).is_empty());
    let s = sells(&l.tick(105));
    assert_eq!(
        s.len(),
        1,
        "the idle exit fires at +5 whatever slot the balance report named"
    );
    assert_eq!(s[0].reason, ExitReason::NoActivity);
    // the trigger is the slot that just closed, not the report's 120
    assert_eq!(s[0].trigger_slot, 104);
}

#[test]
fn phantom_stop_measures_from_the_post_bundle_price_without_our_tokens() {
    let user = Pubkey::new_unique();
    // a sell that takes the historical price 14% below the post-bundle price
    let run = |p: ExitPolicy| -> Vec<SellOrder> {
        let mut l = Lead::new(p, &[]);
        l.buy(5_000_000_000);
        let v0 = l.v;
        l.slot = 101;
        let v_drop = (v0 as f64 * 0.86f64.sqrt()) as u64;
        let mut out = sells(&l.other(user, v_drop));
        // keep trading (no idle exit) until the stop is armed
        for s in 102..=106 {
            l.slot = s;
            out.extend(sells(&l.other(user, l.v - 1_000)));
        }
        out
    };
    let mut entry = lead();
    entry.no_activity_ms = 0;
    entry.max_hold_slots = 0;
    entry.tp = TpLadder::default();
    // -14% from the post-bundle price: the entry-referenced stop holds
    assert!(run(entry).is_empty());
    // ... while a fill-referenced stop at the same 15% fires: with our 5 SOL
    // still in the curve, the same sell moves the marked price further below
    // our fill (roughly twice our share of the token reserve)
    let mut fill = entry;
    fill.phantom_from = PhantomFrom::Fill;
    let s = run(fill);
    assert_eq!(s.first().map(|o| o.reason), Some(ExitReason::PhantomSl));
    // -16%: the entry-referenced stop fires once armed (after_ms = 1000)
    let mut l = Lead::new(entry, &[]);
    l.buy(2_000_000_000);
    let v0 = l.v;
    l.slot = 101;
    let v_drop = (v0 as f64 * 0.84f64.sqrt()) as u64;
    assert!(sells(&l.other(user, v_drop)).is_empty(), "not armed yet");
    let m = l.pos.ex_us_mult_bps().unwrap();
    assert!((8_350..8_450).contains(&m), "ex-us mult {m}");
    let s = sells(&l.tick(105));
    assert_eq!(s.first().map(|o| o.reason), Some(ExitReason::PhantomSl));
}

#[test]
fn insider_sell_off_disables_the_bundle_sell_trigger() {
    let insider = Pubkey::new_unique();
    let mut l = Lead::new(lead(), &[insider]);
    l.buy(500_000_000);
    l.slot = 101;
    let a = l.other(insider, l.v - 1);
    assert!(matches!(
        a[0],
        ExitAction::DumpSignal {
            reason: ExitReason::BundleSell,
            ..
        }
    ));
    let mut p = lead();
    p.insider_sell = InsiderSell::Off;
    let mut l = Lead::new(p, &[insider]);
    l.buy(500_000_000);
    l.slot = 101;
    assert!(l.other(insider, l.v - 1).is_empty());
    assert!(InsiderSell::Bundle.creator_set() && InsiderSell::Bundle.bundle_wallets());
    assert!(!InsiderSell::BundleOnly.creator_set() && InsiderSell::BundleOnly.bundle_wallets());
    assert!(InsiderSell::Creator.creator_set() && !InsiderSell::Creator.bundle_wallets());
}

/// `[exit.default]` of the operator wave (docs/09): every LEAD rule off,
/// sell everything at create + 6.
const WAVE: &str = r#"
tp = []
tp_slip_bps = 2500
trail = { activate = 0.0, retrace_bps = 0 }
phantom_sl = { after_ms = 0, drop = 0.0 }
no_activity_ms = 0
max_hold_ms = 0
max_hold_slots = 0
bundle_sell_min_lamports = 0
insider_sell = "off"
consolidation_pct = 0
big_buy_lamports = [0, 0]
sell_retry = { every_slots = 2, max_tries = 15, tip_mult = 2.0 }
hold_to_create_slots = 6
"#;

fn wave() -> ExitPolicy {
    let c: ExitConfig = toml::from_str(WAVE).unwrap();
    c.to_policy(20_000_000).unwrap()
}

#[test]
fn hold_to_create_sells_everything_at_create_plus_h_and_not_before() {
    // created at 99, landed at 100 (create + 1): the hold counts from the
    // create, so the sell goes out at slot 105 = 99 + 6, whatever happens
    // before it (a +60% pump, an insider dump, an idle stretch: all off)
    let dev = Pubkey::new_unique();
    let mut l = Lead::new(wave(), &[dev]);
    l.pos = l.pos.with_create_slot(99);
    assert_eq!(l.pos.hold_to_slot(), Some(105));
    let opened = l.buy(500_000_000);
    assert!(sells(&opened).is_empty(), "{opened:?}");
    let tokens = l.pos.tokens;
    for slot in 101..=104 {
        l.slot = slot;
        let v = if slot == 102 {
            l.v * 13 / 10
        } else {
            l.v * 9 / 10
        };
        assert!(sells(&l.other(dev, v)).is_empty(), "slot {slot}");
        assert!(sells(&l.tick(slot)).is_empty(), "slot {slot}");
    }
    let s = sells(&l.tick(105));
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!(s[0].reason, ExitReason::HoldTo);
    assert_eq!(s[0].tokens, tokens, "100% of the balance");
    assert_eq!(s[0].kind, SellKind::Dump, "dump bounds: sells at any price");
    assert!(!l.pos.dump_signalled, "a time exit is not a dump signal");
    assert_eq!(ExitReason::HoldTo.label(), "hold_to");
    // retries keep the reason (the order is re-sent, never doubled)
    let r = sells(&l.tick(107));
    assert_eq!(r.len(), 1);
    assert_eq!((r[0].reason, r[0].try_no), (ExitReason::HoldTo, 2));

    // per position: H = 10 from the operator table, the default stays 6
    let mut p10 = wave();
    p10.hold_to_create_slot = Some(10);
    let mut l = Lead::new(p10, &[]);
    l.pos = l.pos.with_create_slot(100);
    l.buy(500_000_000);
    for slot in 101..=109 {
        assert!(sells(&l.tick(slot)).is_empty(), "slot {slot}");
    }
    assert_eq!(sells(&l.tick(110))[0].reason, ExitReason::HoldTo);

    // the feed skipped slots: the first slot seen at or past create + H sells
    let mut l = Lead::new(wave(), &[]);
    l.pos = l.pos.with_create_slot(100);
    l.buy(500_000_000);
    assert!(sells(&l.tick(103)).is_empty());
    assert_eq!(sells(&l.tick(108))[0].reason, ExitReason::HoldTo);

    // a landing at or past the hold (H = 1, landed at create + 1) sells at once
    let mut p1 = wave();
    p1.hold_to_create_slot = Some(1);
    let mut l = Lead::new(p1, &[]);
    l.pos = l.pos.with_create_slot(99);
    let s = sells(&l.buy(500_000_000));
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!(s[0].reason, ExitReason::HoldTo);

    // unknown create slot: counted from the landing
    let mut l = Lead::new(wave(), &[]);
    l.buy(500_000_000);
    assert_eq!(l.pos.hold_to_slot(), Some(106));
    assert!(sells(&l.tick(105)).is_empty());
    assert_eq!(sells(&l.tick(106))[0].reason, ExitReason::HoldTo);

    // off (the shipped default): the same position never sells on time
    let mut off = wave();
    off.hold_to_create_slot = None;
    let mut l = Lead::new(off, &[]);
    l.pos = l.pos.with_create_slot(99);
    l.buy(500_000_000);
    assert!(sells(&l.tick(500)).is_empty());
}

#[test]
fn hold_to_create_coexists_with_the_lead_rules() {
    // the LEAD exits stay on next to the hold: whichever comes first sells
    let mut p = lead();
    p.hold_to_create_slot = Some(3);
    let mut l = Lead::new(p, &[]);
    l.pos = l.pos.with_create_slot(100);
    l.buy(500_000_000);
    // steady trading (no idle exit), max_hold_slots 5 would be 105: the hold at 103 wins
    for slot in 101..=102 {
        l.slot = slot;
        assert!(sells(&l.other(Pubkey::new_unique(), l.v + 10_000_000)).is_empty());
    }
    l.slot = 103;
    let s = sells(&l.other(Pubkey::new_unique(), l.v + 10_000_000));
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!(s[0].reason, ExitReason::HoldTo);
}
