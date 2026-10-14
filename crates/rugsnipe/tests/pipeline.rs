//! End-to-end paper pipeline through the real hot loop, cold thread, sender
//! thread, state.db and rslog (no network: an in-memory feed).

mod common;

use common::*;
use pump_codec::Pubkey;
use rugsnipe_core::rslog::Rec;
use rugsnipe_feed::{FeedBuf, FeedMux, FeedStatus, Payload};
use std::time::{Duration, Instant};

fn drive(
    app: &mut rugsnipe::app::App,
    feed: &mut dyn rugsnipe_feed::Feed,
    until: impl Fn(&rugsnipe::app::App) -> bool,
) {
    let mut buf = FeedBuf::with_capacity(64, 1 << 16);
    let t0 = Instant::now();
    loop {
        let st = app.hot.step(feed, &mut buf);
        if st == FeedStatus::Ended && until(app) {
            return;
        }
        if t0.elapsed() > Duration::from_secs(10) {
            panic!("timeout; counters {:?}", app.hot.counters);
        }
        if st != FeedStatus::Ready(1) {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

#[test]
fn paper_create_to_fill_to_dump_exit_is_recorded_everywhere() {
    let dir = tmpdir("pipeline");
    let dev = Pubkey::new_unique();
    let mut app = build(paper_cfg(&dir), armset(&[dev], &[]), &dir, 3, true);
    assert!(app.senders.real().is_empty(), "paper builds no real sender");
    let (a, b, c_, e) = (
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    );
    let (hit_a, mut curve_a) = create(a, dev, 1000, 1, 1_000_000_000);
    let (hit_b, _) = create(b, Pubkey::new_unique(), 1000, 2, 0);
    let (mut hit_c, _) = create(c_, dev, 1000, 3, 0);
    hit_c.mayhem = true;
    let (hit_e, _) = create(e, Pubkey::new_unique(), 1004, 5, 0);
    let (x, y, z) = (
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    );
    let t1 = trade(&mut curve_a, a, x, 1001, 10, 500_000_000, 0);
    let t2 = trade(&mut curve_a, a, y, 1002, 11, 200_000_000, 0);
    // the dev (bundle wallet) dumps ~0.3 SOL worth
    let dev_tokens = hit_a.intx_tokens_out / 3;
    let t3 = trade(&mut curve_a, a, dev, 1005, 12, 0, dev_tokens);
    let t4 = trade(&mut curve_a, a, z, 1006, 13, 10_000_000, 0);
    let events = vec![
        ev(1000, Payload::Hit(hit_a)),
        ev(1000, Payload::Hit(hit_b)),
        ev(1000, Payload::Hit(hit_c)),
        ev(1000, Payload::Hit(hit_b)), // duplicate copy -> Dup
        ev(1001, Payload::Trade(t1)),
        ev(1002, Payload::Trade(t2)),
        ev(1004, Payload::Hit(hit_e)),
        ev(1005, Payload::Trade(t3)),
        ev(1006, Payload::Trade(t4)),
    ];
    let mut feed = FeedMux::new(vec![Box::new(VecFeed::new(events))]);
    drive(&mut app, &mut feed, |a| a.hot.counters.closed >= 1);
    let c = app.hot.counters;
    assert_eq!(c.creates, 4, "{c:?}");
    assert_eq!(c.intents, 1);
    assert_eq!(c.fills, 1);
    assert_eq!(c.closed, 1);
    assert!(c.sells >= 1 && c.sell_fills >= 1, "{c:?}");
    let records = app.senders.paper.records();
    assert!(!records.is_empty());
    // the recorded primary variant is a valid signed transaction of the paper wallet
    let v = &records[0].variant;
    let bytes = v.bytes();
    assert_eq!(bytes[0], 1, "one signature (nonce authority = fee payer)");
    assert!(rugsnipe_tx::signer::verify(
        &app.owner,
        &bytes[65..],
        &v.sig
    ));
    let owner = app.owner;
    std::thread::sleep(Duration::from_millis(200));
    let summary = app.shutdown(vec![]);
    let dec: std::collections::HashMap<String, u64> = summary.decisions.iter().cloned().collect();
    assert_eq!(dec.get("paper"), Some(&1));
    assert_eq!(dec.get("no_match"), Some(&2));
    assert_eq!(dec.get("skip_class"), Some(&1));
    assert_eq!(summary.cold.fills, 1);
    assert_eq!(summary.cold.positions_closed, 1);
    // state.db
    let conn = rusqlite::Connection::open(dir.join("state.db")).unwrap();
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM intents WHERE state = 'paper' AND mode = 'paper'"
        ),
        1
    );
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM sends"),
        2,
        "primary + secondary variant"
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM fills WHERE err IS NULL AND tokens > 0"
        ),
        1
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM positions WHERE state = 'closed'"
        ),
        1
    );
    assert!(
        count(
            &conn,
            "SELECT COUNT(*) FROM exits WHERE reason = 'bundle_sell'"
        ) >= 1
    );
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM nonces"), 32);
    let wallet: String = conn
        .query_row("SELECT v FROM kv WHERE k = 'paper_wallet'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(wallet, owner.to_string());
    // rslog: every create has a CreateHit and a Decision record
    let recs = rslog_records(&dir.join("rslog"));
    let n = |f: &dyn Fn(&Rec) -> bool| recs.iter().filter(|r| f(r)).count();
    assert_eq!(n(&|r| matches!(r, Rec::CreateHit(_))), 4);
    assert_eq!(n(&|r| matches!(r, Rec::Decision { .. })), 4);
    assert_eq!(n(&|r| matches!(r, Rec::FeedSeen { .. })), 4);
    assert_eq!(n(&|r| matches!(r, Rec::Sent { .. })), 2);
    assert_eq!(n(&|r| matches!(r, Rec::Matched { shadow: 0, .. })), 1);
    assert!(n(&|r| matches!(r, Rec::ExitFired { .. })) >= 1);
    assert_eq!(
        n(&|r| matches!(r, Rec::PosEvent { kind: 1, .. })),
        1,
        "opened"
    );
    assert_eq!(
        n(&|r| matches!(r, Rec::PosEvent { kind: 4, .. })),
        1,
        "closed"
    );
    assert!(summary.rslog_dropped == 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn creator_arg_only_is_shadow_and_never_an_intent() {
    let dir = tmpdir("shadow");
    let creator = Pubkey::new_unique();
    let mut app = build(paper_cfg(&dir), armset(&[], &[creator]), &dir, 3, false);
    let (mut h, _) = create(Pubkey::new_unique(), Pubkey::new_unique(), 50, 1, 0);
    h.creator = creator; // creator argument only (a launcher-style create)
    let mut feed = VecFeed::new(vec![ev(50, Payload::Hit(h))]);
    drive(&mut app, &mut feed, |a| a.hot.counters.creates == 1);
    assert_eq!(app.hot.counters.intents, 0);
    let s = app.shutdown(vec![]);
    assert!(
        s.decisions.iter().any(|(k, v)| k == "shadow" && *v == 1),
        "{:?}",
        s.decisions
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unfilled_when_e2_is_above_v_max() {
    let dir = tmpdir("unfilled");
    let dev = Pubkey::new_unique();
    let mut app = build(paper_cfg(&dir), armset(&[dev], &[]), &dir, 2, false);
    let a = Pubkey::new_unique();
    let (h, mut c) = create(a, dev, 10, 1, 500_000_000);
    // a 15 SOL buy lands in slot+1: E2 is ~46 SOL > v_max 40 SOL
    let t = trade(&mut c, a, Pubkey::new_unique(), 11, 2, 15_000_000_000, 0);
    let (h2, _) = create(Pubkey::new_unique(), Pubkey::new_unique(), 14, 3, 0);
    let mut feed = VecFeed::new(vec![
        ev(10, Payload::Hit(h)),
        ev(11, Payload::Trade(t)),
        ev(14, Payload::Hit(h2)),
    ]);
    drive(&mut app, &mut feed, |a| a.hot.counters.unfilled == 1);
    assert_eq!(app.hot.counters.fills, 0);
    assert_eq!(app.hot.positions(), 0);
    app.shutdown(vec![]);
    let conn = rusqlite::Connection::open(dir.join("state.db")).unwrap();
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM fills WHERE err LIKE 'unfilled%'"
        ),
        1
    );
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM positions"), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn kill_file_stops_buys_latches_and_decisions_are_still_recorded() {
    let dir = tmpdir("kill");
    let dev = Pubkey::new_unique();
    let mut app = build(paper_cfg(&dir), armset(&[dev], &[]), &dir, 3, false);
    std::fs::write(dir.join("KILL"), b"").unwrap();
    let t0 = Instant::now();
    while !app.risk.is_killed() {
        assert!(t0.elapsed() < Duration::from_secs(3), "kill file not seen");
        std::thread::sleep(Duration::from_millis(20));
    }
    std::fs::remove_file(dir.join("KILL")).unwrap();
    std::thread::sleep(Duration::from_millis(250));
    assert!(
        app.risk.is_killed(),
        "the latch stays after the file is removed"
    );
    let (h, _) = create(Pubkey::new_unique(), dev, 10, 1, 0);
    let mut feed = VecFeed::new(vec![ev(10, Payload::Hit(h))]);
    drive(&mut app, &mut feed, |a| a.hot.counters.creates == 1);
    assert_eq!(app.hot.counters.intents, 0);
    assert_eq!(app.senders.paper.total(), 0, "nothing recorded as sent");
    let s = app.shutdown(vec![]);
    assert!(s.cold.killed);
    assert_eq!(s.cold.kill_reason.as_deref(), Some("KillFile"));
    assert!(s
        .decisions
        .iter()
        .any(|(k, v)| k == "skip_killed" && *v == 1));
    let conn = rusqlite::Connection::open(dir.join("state.db")).unwrap();
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM risk WHERE killed = 1"),
        1
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sigusr1_latches_the_kill() {
    let dir = tmpdir("usr1");
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let app = rugsnipe::app::build(rugsnipe::app::AppOpts {
        cfg: paper_cfg(&dir),
        armset: rugsnipe::app::ArmChoice::None,
        regime: None,
        paper_unsigned: false,
        state_path: None,
        rslog_dir: dir.join("rslog"),
        logsws: None,
        healths: vec![],
        e2: None,
        metrics: false,
        fill_timeout_slots: 3,
        lease: false,
        sigusr1: flag.clone(),
        paper_balance: 1,
        live_flag: false,
    })
    .unwrap();
    flag.store(true, std::sync::atomic::Ordering::Release);
    let t0 = Instant::now();
    while !app.risk.is_killed() {
        assert!(t0.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        app.risk.kill_reason(),
        Some(rugsnipe_core::risk::KillReason::Signal)
    );
    app.shutdown(vec![]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn real_modes_are_refused_by_the_library_and_the_binary() {
    let dir = tmpdir("refuse");
    for mode in [rugsnipe_core::Mode::Probe, rugsnipe_core::Mode::Live] {
        let mut cfg = paper_cfg(&dir);
        cfg.mode.mode = mode;
        let r = rugsnipe::app::build(rugsnipe::app::AppOpts {
            cfg,
            armset: rugsnipe::app::ArmChoice::None,
            regime: None,
            paper_unsigned: false,
            state_path: None,
            rslog_dir: dir.join("rslog"),
            logsws: None,
            healths: vec![],
            e2: None,
            metrics: false,
            fill_timeout_slots: 3,
            lease: false,
            sigusr1: Default::default(),
            paper_balance: 1,
            live_flag: true,
        });
        let e = r.expect_err("real mode refused");
        for want in ["cu_limit", "clock offset", "post-execution", "ArmSet"] {
            assert!(e.contains(want), "{mode}: {want:?} missing in\n{e}");
        }
    }
    // the binary refuses before opening any connection (paper example + --mode live)
    let cfg =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/paper.example.toml");
    for args in [vec!["--mode", "live", "--live"], vec!["--mode", "probe"]] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_rugsnipe-hot"))
            .arg("--config")
            .arg(&cfg)
            .args(&args)
            .arg("--state")
            .arg(dir.join("never.db"))
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("refused"), "{err}");
        assert!(err.contains("ArmSet"), "{err}");
        assert!(
            !dir.join("never.db").exists(),
            "refused before state or feeds were opened"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
