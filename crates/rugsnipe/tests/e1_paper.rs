//! The E1 shadow fill model end to end (docs/04 sections 2 and 7): an E1
//! verdict and its replay tape go through the real hot loop (lead exits from
//! `config/paper-lead.example.toml`), the cold thread, state.db and rslog;
//! failed attempts carry their reason and cost into the session totals.

mod common;

use common::*;
use pump_codec::curve::{CurveState, FeeBps};
use pump_codec::Pubkey;
use rugsnipe::msgs::{HotIn, ShadowInfo};
use rugsnipe_core::config::Config;
use rugsnipe_core::rslog::{Rec, ShadowOutcome};
use rugsnipe_core::IntentId;
use rugsnipe_exit::{Trade, Venue};
use rugsnipe_feed::{FeedBuf, FeedMux, FeedStatus, Payload};
use std::time::{Duration, Instant};

const S0: u64 = 1_000;

fn lead_cfg(dir: &std::path::Path) -> Config {
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/paper-lead.example.toml"
    );
    let lead = rugsnipe::setup::load_config(std::path::Path::new(p)).unwrap();
    let mut cfg = paper_cfg(dir);
    cfg.exit = lead.exit.clone();
    cfg.paper = lead.paper.clone();
    cfg.entry = lead.entry.clone();
    cfg
}

/// Feed the creates, then stop once every intent exists.
fn decide_all(app: &mut rugsnipe::app::App, events: Vec<rugsnipe_feed::FeedEvent>, n: u64) {
    let mut feed = FeedMux::new(vec![Box::new(VecFeed::new(events))]);
    let mut buf = FeedBuf::with_capacity(64, 1 << 16);
    let t0 = Instant::now();
    while app.hot.counters.intents < n {
        if app.hot.step(&mut feed, &mut buf) == FeedStatus::Ended && app.hot.counters.intents < n {
            panic!("feed ended with {:?}", app.hot.counters);
        }
        assert!(t0.elapsed() < Duration::from_secs(10));
    }
}

fn state(c: &CurveState) -> (u64, u64) {
    (c.virtual_sol, c.virtual_token)
}

/// A historical trade (reserves without us).
fn hist(c: &mut CurveState, mint: Pubkey, user: Pubkey, slot: u64, sol: u64, sell: u64) -> Trade {
    let t = trade(c, mint, user, slot, slot * 100 + sol % 97, sol, sell);
    Trade {
        venue: Venue::Curve,
        mint,
        user,
        is_buy: t.is_buy,
        sol_amount: t.sol_amount,
        token_amount: t.token_amount,
        fee_lamports: t.fee,
        reserve_sol: t.virtual_sol_reserves,
        reserve_token: t.virtual_token_reserves,
        slot,
    }
}

fn info(c: &CurveState, premium_bps: u32) -> ShadowInfo {
    ShadowInfo {
        create_slot: S0,
        create_index: Some(100),
        block_txs: Some(1_000),
        bundle_trades: 2,
        bundle_wallets: 1,
        our_index: Some(102),
        tp_ahead: 0,
        tp_behind: 1,
        s0_failed: 0,
        premium_bps,
        post_bundle_v: c.virtual_sol,
        post_bundle_t: c.virtual_token,
        post_bundle_mcap: (c.virtual_sol as u128 * 1_000_000_000_000_000 / c.virtual_token as u128)
            as u64,
    }
}

#[test]
fn e1_fill_replays_the_tape_and_sells_one_slot_after_the_trigger() {
    let dir = tmpdir("e1-fill");
    let dev = Pubkey::new_unique();
    let mut app = build(lead_cfg(&dir), armset(&[dev], &[]), &dir, 5_000, false);
    let mint = Pubkey::new_unique();
    let (hit, mut curve) = create(mint, dev, S0, 1, 1_000_000_000);
    decide_all(&mut app, vec![ev(S0, Payload::Hit(hit))], 1);
    let intent = IntentId::new(&mint, &app.owner).0;
    // the bundle: one wallet outside the creator set buys 1 SOL behind the create
    let b1 = Pubkey::new_unique();
    hist(&mut curve, mint, b1, S0, 1_000_000_000, 0);
    let (v_ref, t_ref) = state(&curve);
    app.hot.inject(HotIn::E1Open {
        intent,
        mint,
        outcome: ShadowOutcome::Filled,
        info: info(&curve, 110),
    });
    app.hot.inject(HotIn::E1Insider {
        intent,
        key: b1,
        creator: false,
    });
    app.hot.inject(HotIn::E1Insider {
        intent,
        key: dev,
        creator: true,
    });
    assert_eq!(app.hot.counters.fills, 1);
    // our tokens: the quote at the post-bundle state / (1 + 1.1%)
    let st = CurveState {
        virtual_sol: v_ref,
        virtual_token: t_ref,
        real_sol: 0,
        real_token: u64::MAX,
        complete: false,
        creator: dev,
    };
    let q = st
        .quote_buy_exact_sol_in(100_000_000, FeeBps::DEFAULT)
        .unwrap();
    let ours = (q.tokens as u128 * 10_000 / 10_110) as u64;
    // the tape: a sniper behind us in slot 0, a 6 SOL buy in slot +1 (the
    // value crosses +15%: TP on that trade), the dev sells in slot +2 (dump
    // re-send; the TP variant still fills at the end of slot +2)
    let tape = vec![
        hist(&mut curve, mint, Pubkey::new_unique(), S0, 300_000_000, 0),
        hist(
            &mut curve,
            mint,
            Pubkey::new_unique(),
            S0 + 1,
            6_000_000_000,
            0,
        ),
        hist(&mut curve, mint, dev, S0 + 2, 0, 10_000_000_000_000),
    ];
    let (v_end, t_end) = state(&curve);
    for t in &tape {
        app.hot.inject(HotIn::E1Trade { intent, trade: *t });
    }
    app.hot.inject(HotIn::E1End {
        intent,
        horizon: S0 + 12,
    });
    assert_eq!(app.hot.counters.closed, 1, "{:?}", app.hot.counters);
    // the sell fills at the counterfactual end of slot +2: our tokens out of the curve
    let t_cf = t_end - ours;
    let v_cf = (v_end as u128 * t_end as u128 / t_cf as u128) as u64;
    let want = CurveState {
        virtual_sol: v_cf,
        virtual_token: t_cf,
        real_sol: 0,
        real_token: 0,
        complete: false,
        creator: dev,
    }
    .quote_sell(ours, FeeBps::DEFAULT)
    .unwrap()
    .net;
    std::thread::sleep(Duration::from_millis(200));
    let summary = app.shutdown(vec![]);
    let conn = rusqlite::Connection::open(dir.join("state.db")).unwrap();
    let (reason, slot, delta): (String, i64, i64) = conn
        .query_row("SELECT reason, slot, sol_delta FROM exits", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!(reason, "tp0");
    assert_eq!(slot as u64, S0 + 2, "TP at slot +1 fills at the end of +2");
    assert_eq!(delta as u64, want);
    let tokens: i64 = conn
        .query_row("SELECT tokens FROM fills", [], |r| r.get(0))
        .unwrap();
    assert_eq!(tokens as u64, ours);
    let (outcome, our_index, prem, mcap): (String, i64, i64, i64) = conn
        .query_row(
            "SELECT outcome, our_index, premium_bps, post_bundle_mcap FROM shadow",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!((outcome.as_str(), our_index, prem), ("filled", 102, 110));
    assert!(mcap > 30_000_000_000);
    let via: String = conn
        .query_row("SELECT via FROM intents", [], |r| r.get(0))
        .unwrap();
    assert_eq!(via, "signer");
    // session totals
    let p = &summary.cold.paper;
    let pnl = want as i64 - q.total as i64;
    assert!(pnl > 0, "a take-profit exit makes money: {pnl}");
    assert_eq!((p.attempts, p.fills, p.closed), (1, 1, 1));
    assert_eq!(p.landing_rate, 1.0);
    assert_eq!(p.mean_premium_bps_fills, 110.0);
    assert_eq!(p.realized_lamports, pnl);
    assert_eq!(p.net_lamports, pnl - 4_000_000);
    assert_eq!(p.exits_by_reason.get("tp0"), Some(&1));
    let recs = rslog_records(&dir.join("rslog"));
    assert!(recs.iter().any(|r| matches!(
        r,
        Rec::Shadow {
            outcome: 0,
            our_index: 102,
            ..
        }
    )));
    assert!(!recs.iter().any(|r| matches!(r, Rec::FailedBuy { .. })));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn e1_no_trades_exits_on_idle_and_steady_trading_hits_the_slot_cap() {
    let dir = tmpdir("e1-idle");
    let dev = Pubkey::new_unique();
    let mut app = build(lead_cfg(&dir), armset(&[dev], &[]), &dir, 5_000, false);
    let (m1, m2) = (Pubkey::new_unique(), Pubkey::new_unique());
    let (h1, mut c1) = create(m1, dev, S0, 1, 1_000_000_000);
    let (h2, mut c2) = create(m2, dev, S0, 2, 1_000_000_000);
    decide_all(
        &mut app,
        vec![ev(S0, Payload::Hit(h1)), ev(S0, Payload::Hit(h2))],
        2,
    );
    let owner = app.owner;
    for (m, c) in [(m1, &mut c1), (m2, &mut c2)] {
        hist(c, m, Pubkey::new_unique(), S0, 500_000_000, 0);
        app.hot.inject(HotIn::E1Open {
            intent: IntentId::new(&m, &owner).0,
            mint: m,
            outcome: ShadowOutcome::Filled,
            info: info(c, 110),
        });
    }
    // m1: nothing trades; m2: a small buy in every slot
    let i2 = IntentId::new(&m2, &owner).0;
    for s in S0 + 1..=S0 + 8 {
        let t = hist(&mut c2, m2, Pubkey::new_unique(), s, 20_000_000, 0);
        app.hot.inject(HotIn::E1Trade {
            intent: i2,
            trade: t,
        });
    }
    for m in [m1, m2] {
        app.hot.inject(HotIn::E1End {
            intent: IntentId::new(&m, &owner).0,
            horizon: S0 + 12,
        });
    }
    assert_eq!(app.hot.counters.closed, 2);
    std::thread::sleep(Duration::from_millis(200));
    let summary = app.shutdown(vec![]);
    let conn = rusqlite::Connection::open(dir.join("state.db")).unwrap();
    let slot_of = |m: Pubkey| -> (String, i64) {
        conn.query_row(
            "SELECT reason, slot FROM exits WHERE mint = ?1",
            [m.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    };
    // 1 s = 5 slots of 217 ms from the landing slot; the sell fills one slot later
    assert_eq!(slot_of(m1), ("no_activity".to_string(), (S0 + 6) as i64));
    assert_eq!(slot_of(m2), ("max_hold".to_string(), (S0 + 6) as i64));
    assert_eq!(summary.cold.paper.closed, 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn e1_failed_attempts_are_logged_with_reason_and_cost() {
    let dir = tmpdir("e1-failed");
    let dev = Pubkey::new_unique();
    let mut app = build(lead_cfg(&dir), armset(&[dev], &[]), &dir, 5_000, false);
    let mints: Vec<Pubkey> = (0..4).map(|_| Pubkey::new_unique()).collect();
    let mut curves = vec![];
    let mut events = vec![];
    for (k, m) in mints.iter().enumerate() {
        let (h, c) = create(*m, dev, S0, k as u64 + 1, 1_000_000_000);
        events.push(ev(S0, Payload::Hit(h)));
        curves.push(c);
    }
    decide_all(&mut app, events, 4);
    let owner = app.owner;
    let open = |app: &mut rugsnipe::app::App, m: Pubkey, o: ShadowOutcome, inf: ShadowInfo| {
        app.hot.inject(HotIn::E1Open {
            intent: IntentId::new(&m, &owner).0,
            mint: m,
            outcome: o,
            info: inf,
        })
    };
    // premium: a sniper inside the bundle run cost +3.4%
    open(
        &mut app,
        mints[0],
        ShadowOutcome::Premium,
        info(&curves[0], 340),
    );
    // slippage: the post-bundle curve is past the operator's v_max (40 SOL)
    let mut hot_curve = curves[1];
    let t = hot_curve.tokens_for_sol(15_000_000_000);
    hot_curve.apply_buy(t, 15_000_000_000);
    open(
        &mut app,
        mints[1],
        ShadowOutcome::Filled,
        info(&hot_curve, 110),
    );
    // class: not multi-bundle (a skip, no cost); unresolved: nothing recorded as a fill
    open(
        &mut app,
        mints[2],
        ShadowOutcome::Class,
        info(&curves[2], 110),
    );
    open(
        &mut app,
        mints[3],
        ShadowOutcome::Unresolved,
        ShadowInfo::default(),
    );
    assert_eq!(app.hot.counters.fills, 0);
    assert_eq!(app.hot.counters.e1_failed, 4);
    assert_eq!(app.hot.positions(), 0);
    std::thread::sleep(Duration::from_millis(200));
    let summary = app.shutdown(vec![]);
    let conn = rusqlite::Connection::open(dir.join("state.db")).unwrap();
    let mut rows: Vec<(String, i64, i64)> = conn
        .prepare("SELECT fail_reason, fail_cost, sol_delta FROM fills ORDER BY fail_reason")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    rows.sort();
    assert_eq!(
        rows,
        vec![
            ("class".to_string(), 0, 0),
            ("premium".to_string(), 3_000_000, -3_000_000),
            ("slippage".to_string(), 3_000_000, -3_000_000),
        ]
    );
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM shadow"), 4);
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM shadow WHERE outcome = 'unresolved'"
        ),
        1
    );
    let p = &summary.cold.paper;
    assert_eq!((p.attempts, p.fills, p.failed_attempts), (2, 0, 2));
    assert_eq!(p.in_class, 2);
    assert_eq!(p.outcomes.get("class"), Some(&1));
    assert_eq!(p.landing_rate, 0.0);
    assert_eq!(p.failed_cost_lamports, 6_000_000);
    assert_eq!(p.net_lamports, -6_000_000);
    // -3,000,000 per attempt on a 0.1 SOL stake
    assert!(
        (p.ev_per_attempt + 0.03).abs() < 1e-9,
        "{}",
        p.ev_per_attempt
    );
    assert_eq!(p.mean_premium_bps_attempts, 225.0);
    let recs = rslog_records(&dir.join("rslog"));
    let failed: Vec<(u8, u64)> = recs
        .iter()
        .filter_map(|r| match r {
            Rec::FailedBuy { reason, cost, .. } => Some((*reason, *cost)),
            _ => None,
        })
        .collect();
    assert_eq!(failed.len(), 3);
    assert!(failed.contains(&(ShadowOutcome::Premium as u8, 3_000_000)));
    assert!(failed.contains(&(ShadowOutcome::Slippage as u8, 3_000_000)));
    assert!(failed.contains(&(ShadowOutcome::Class as u8, 0)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn class_arm_takes_every_create_and_records_via_class() {
    let dir = tmpdir("e1-class-arm");
    let mut set = armset(&[], &[]);
    set.match_all = Some(1);
    let mut app = build(lead_cfg(&dir), set, &dir, 5_000, false);
    let (h, _) = create(Pubkey::new_unique(), Pubkey::new_unique(), S0, 1, 0);
    decide_all(&mut app, vec![ev(S0, Payload::Hit(h))], 1);
    std::thread::sleep(Duration::from_millis(200));
    let _ = app.shutdown(vec![]);
    let conn = rusqlite::Connection::open(dir.join("state.db")).unwrap();
    let via: String = conn
        .query_row("SELECT via FROM intents", [], |r| r.get(0))
        .unwrap();
    assert_eq!(via, "class");
    let recs = rslog_records(&dir.join("rslog"));
    assert!(recs.iter().any(|r| matches!(
        r,
        Rec::Matched {
            via: 4,
            shadow: 0,
            ..
        }
    )));
    let _ = std::fs::remove_dir_all(&dir);
}
