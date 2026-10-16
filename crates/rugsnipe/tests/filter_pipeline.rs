//! The entry filter in the real pipeline (hot loop, cold thread, state.db,
//! rslog) and the outcome store across a SIGKILL:
//!
//! * `entry.filter_mode = "skip"` drops exactly the attempts scoring in the
//!   bottom tertile (a fill verdict and a failed one): no position, no fill
//!   row, no cost, outcome `filtered`; a top-tertile and an unscored intent
//!   trade. In `log` mode the same intents all trade, and every verdict
//!   carries its score and tertile (rslog `Filter`, state.db `filter`, the
//!   summary's tertile totals);
//! * the live outcome store is written to state.db, survives a SIGKILL of
//!   the process that wrote it, and is reloaded at the next start.

mod common;

use common::*;
use pump_codec::curve::CurveState;
use pump_codec::Pubkey;
use rugsnipe::filter::{Feat, OutcomeStore};
use rugsnipe::msgs::{HotIn, ShadowInfo};
use rugsnipe_core::config::{Config, FilterMode};
use rugsnipe_core::rslog::{Rec, ShadowOutcome};
use rugsnipe_core::IntentId;
use rugsnipe_feed::{FeedBuf, FeedMux, FeedStatus, Payload};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const S0: u64 = 1_000;
/// `tx.nonce_pool` of the test config (paper default).
const NONCE_POOL: u64 = 32;

fn report(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/reports/strategy")
        .join(name)
}

fn filter_cfg(dir: &Path, model: &Path, mode: FilterMode) -> Config {
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/paper-lead.example.toml"
    );
    let lead = rugsnipe::setup::load_config(Path::new(p)).unwrap();
    let mut cfg = paper_cfg(dir);
    cfg.exit = lead.exit.clone();
    cfg.paper = lead.paper.clone();
    cfg.entry = lead.entry.clone();
    cfg.entry.filter_model = model.display().to_string();
    cfg.entry.filter_mode = mode;
    assert_eq!(cfg.tx.nonce_pool as u64, NONCE_POOL);
    cfg
}

/// A feature vector of the exported check rows in tertile `t`, and its score.
fn row_in(t: u64) -> (Feat, f64) {
    let v: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(report("entry_filter_v1_check.json")).unwrap(),
    )
    .unwrap();
    let r = v["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["tertile"].as_u64() == Some(t))
        .unwrap();
    let mut f = Feat::default();
    for (k, x) in r["x"].as_array().unwrap().iter().enumerate() {
        f.x[k] = x.as_f64().unwrap();
    }
    (f, r["score"].as_f64().unwrap())
}

fn decide_all(app: &mut rugsnipe::app::App, events: Vec<rugsnipe_feed::FeedEvent>, n: u64) {
    let mut feed = FeedMux::new(vec![Box::new(VecFeed::new(events))]);
    let mut buf = FeedBuf::with_capacity(64, 1 << 16);
    let t0 = Instant::now();
    while app.hot.counters.intents < n {
        if app.hot.step(&mut feed, &mut buf) == FeedStatus::Ended && app.hot.counters.intents < n {
            panic!(
                "feed ended with {:?}, decisions {:?}",
                app.hot.counters,
                app.stats.decision_counts()
            );
        }
        assert!(t0.elapsed() < Duration::from_secs(10));
    }
}

fn info(c: &CurveState) -> ShadowInfo {
    ShadowInfo {
        create_slot: S0,
        create_index: Some(100),
        block_txs: Some(1_000),
        bundle_trades: 2,
        bundle_wallets: 1,
        our_index: Some(102),
        premium_bps: 110,
        post_bundle_v: c.virtual_sol,
        post_bundle_t: c.virtual_token,
        ..Default::default()
    }
}

/// Four launches by `dev`: (features, verdict). Filled ones close on idle.
fn run_launches(
    app: &mut rugsnipe::app::App,
    dev: Pubkey,
    plan: &[(Option<Feat>, ShadowOutcome)],
) -> Vec<(Pubkey, u64)> {
    let mut creates = vec![];
    let mut out = vec![];
    let pool = NONCE_POOL;
    let mut used = std::collections::HashSet::new();
    for (k, _) in plan.iter().enumerate() {
        // distinct nonce slots (xxh3(mint) % pool), so every create decides
        let mint = loop {
            let m = Pubkey::new_unique();
            if used.insert(xxhash_rust::xxh3::xxh3_64(m.as_ref()) % pool) {
                break m;
            }
        };
        let (h, c) = create(mint, dev, S0, 10 + k as u64, 1_000_000_000);
        creates.push(ev(S0, Payload::Hit(h)));
        out.push((mint, c));
    }
    decide_all(app, creates, plan.len() as u64);
    let owner = app.owner;
    let mut ids = vec![];
    for ((feat, outcome), (mint, c)) in plan.iter().zip(out.iter_mut()) {
        let intent = IntentId::new(mint, &owner).0;
        let b = Pubkey::new_unique();
        let tok = c.tokens_for_sol(500_000_000);
        c.apply_buy(tok, 500_000_000);
        if let Some(f) = feat {
            app.hot.inject(HotIn::E1Feat {
                intent,
                feat: *f,
                op_lead_n: 0,
                bw_lead_n: 0,
            });
        }
        app.hot.inject(HotIn::E1Open {
            intent,
            mint: *mint,
            outcome: *outcome,
            info: info(c),
        });
        app.hot.inject(HotIn::E1Insider {
            intent,
            key: b,
            creator: false,
        });
        app.hot.inject(HotIn::E1End {
            intent,
            horizon: S0 + 12,
        });
        ids.push((*mint, intent));
    }
    ids
}

#[test]
fn skip_mode_drops_the_bottom_tertile_and_log_mode_only_records() {
    let (low, s_low) = row_in(0);
    let (high, s_high) = row_in(2);
    let plan = [
        (Some(low), ShadowOutcome::Filled),  // bottom tertile, would fill
        (Some(high), ShadowOutcome::Filled), // top tertile
        (None, ShadowOutcome::Filled),       // no features: unscored, passes
        (Some(low), ShadowOutcome::Premium), // bottom tertile, would fail
    ];
    for mode in [FilterMode::Skip, FilterMode::Log] {
        let dir = tmpdir(&format!("filter-{mode:?}"));
        let dev = Pubkey::new_unique();
        let cfg = filter_cfg(&dir, &report("entry_filter_v1.json"), mode);
        let mut app = build(cfg, armset(&[dev], &[]), &dir, 5_000, false);
        let ids = run_launches(&mut app, dev, &plan);
        let c = app.hot.counters;
        let skip = mode == FilterMode::Skip;
        assert_eq!(c.filtered, if skip { 2 } else { 0 }, "{c:?}");
        assert_eq!(c.fills, if skip { 2 } else { 3 }, "{c:?}");
        assert_eq!(c.closed, c.fills, "filled positions close on idle: {c:?}");
        std::thread::sleep(Duration::from_millis(200));
        let summary = app.shutdown(vec![]);
        let conn = rusqlite::Connection::open(dir.join("state.db")).unwrap();
        let outcome = |k: usize| -> String {
            conn.query_row(
                "SELECT outcome FROM shadow WHERE intent_id = ?1",
                [ids[k].1 as i64],
                |r| r.get(0),
            )
            .unwrap()
        };
        let want: [&str; 4] = if skip {
            ["filtered", "filled", "filled", "filtered"]
        } else {
            ["filled", "filled", "filled", "premium"]
        };
        for (k, w) in want.iter().enumerate() {
            assert_eq!(outcome(k), *w, "{mode:?} launch {k}");
        }
        // a filtered intent was never sent: no fill row, no failed cost
        assert_eq!(
            count(&conn, "SELECT COUNT(*) FROM fills"),
            if skip { 2 } else { 4 }
        );
        assert_eq!(
            count(
                &conn,
                "SELECT COALESCE(SUM(cost), 0) FROM shadow WHERE outcome = 'filtered'"
            ),
            0
        );
        // every verdict is scored (or marked unscored) in state.db
        let rows: Vec<(i64, Option<f64>, Option<i64>, i64)> = {
            let mut st = conn
                .prepare("SELECT intent_id, score, tertile, skipped FROM filter")
                .unwrap();
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .unwrap()
                .map(|r| r.unwrap())
                .collect()
        };
        assert_eq!(rows.len(), 4);
        let row = |k: usize| *rows.iter().find(|r| r.0 == ids[k].1 as i64).unwrap();
        assert!((row(0).1.unwrap() - s_low).abs() < 1e-12);
        assert_eq!((row(0).2, row(0).3), (Some(0), skip as i64));
        assert!((row(1).1.unwrap() - s_high).abs() < 1e-12);
        assert_eq!((row(1).2, row(1).3), (Some(2), 0));
        assert_eq!((row(2).1, row(2).2, row(2).3), (None, None, 0));
        assert_eq!((row(3).2, row(3).3), (Some(0), skip as i64));
        // rslog: one Filter record per verdict
        let recs = rslog_records(&dir.join("rslog"));
        let filt: Vec<(u8, u8)> = recs
            .iter()
            .filter_map(|r| match r {
                Rec::Filter {
                    tertile, skipped, ..
                } => Some((*tertile, *skipped)),
                _ => None,
            })
            .collect();
        assert_eq!(filt.len(), 4);
        assert_eq!(
            filt.iter().filter(|(_, s)| *s == 1).count(),
            if skip { 2 } else { 0 }
        );
        assert!(filt.contains(&(255, 0)), "the unscored intent");
        // the summary: totals by tertile
        let f = &summary.cold.paper.filter;
        assert_eq!(f.model, "entry_filter_v1");
        assert_eq!(
            (f.scored, f.unscored, f.skipped),
            (3, 1, if skip { 2 } else { 0 })
        );
        let low_t = &f.tertiles["low"];
        assert_eq!(low_t.intents, 2);
        if skip {
            assert_eq!(
                (low_t.filtered, low_t.attempts, low_t.failed_cost_lamports),
                (2, 0, 0)
            );
        } else {
            assert_eq!(
                (
                    low_t.attempts,
                    low_t.fills,
                    low_t.failed_attempts,
                    low_t.closed
                ),
                (2, 1, 1, 1)
            );
            assert!(low_t.failed_cost_lamports > 0);
        }
        assert_eq!(
            (f.tertiles["high"].fills, f.tertiles["high"].closed),
            (1, 1)
        );
        assert_eq!(f.tertiles["unscored"].closed, 1);
        assert_eq!(
            summary
                .cold
                .paper
                .outcomes
                .get("filtered")
                .copied()
                .unwrap_or(0),
            if skip { 2 } else { 0 }
        );
        // the outcome store: one row per launch (flushed at shutdown), LEAD results for the closed ones
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM outcomes"), 4);
        assert_eq!(
            count(&conn, "SELECT COUNT(*) FROM outcomes WHERE lead = 1"),
            c.closed as i64
        );
        assert_eq!(f.store_rows, 4);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn skip_mode_without_a_model_is_refused() {
    let dir = tmpdir("filter-nomodel");
    let mut cfg = paper_cfg(&dir);
    cfg.entry.filter_mode = FilterMode::Skip;
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
        fill_timeout_slots: 10,
        lease: false,
        sigusr1: Default::default(),
        paper_balance: 1,
        live_flag: false,
        real: None,
    });
    assert!(r.err().unwrap().contains("filter_model"));
    let _ = std::fs::remove_dir_all(&dir);
}

const CHILD: &str = "RUGSNIPE_FILTER_STORE_CHILD";

/// The model with outcomes usable at once (the 10 s delay off), so the
/// child writes its rows within a second.
fn model_no_delay(dir: &Path) -> PathBuf {
    let mut v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(report("entry_filter_v1.json")).unwrap())
            .unwrap();
    v["constants"]["op_outcome_delay_s"] = 0.into();
    let p = dir.join("entry_filter_v1.json");
    std::fs::write(&p, v.to_string()).unwrap();
    p
}

fn store_plan() -> Vec<(Option<Feat>, ShadowOutcome)> {
    let (low, _) = row_in(0);
    let (high, _) = row_in(2);
    vec![
        (Some(high), ShadowOutcome::Filled),
        (Some(low), ShadowOutcome::Filled),
        (Some(low), ShadowOutcome::Premium),
        (None, ShadowOutcome::NotInSlot0),
    ]
}

/// The child half of `outcome_store_survives_sigkill`: run four launches,
/// then idle until killed (the cold thread keeps flushing outcomes).
#[test]
#[ignore = "child process of outcome_store_survives_sigkill"]
fn outcome_store_child() {
    let Ok(dir) = std::env::var(CHILD) else {
        return;
    };
    let dir = PathBuf::from(dir);
    let dev: Pubkey = std::fs::read_to_string(dir.join("dev"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let cfg = filter_cfg(&dir, &dir.join("entry_filter_v1.json"), FilterMode::Log);
    let mut app = build(cfg, armset(&[dev], &[]), &dir, 5_000, false);
    run_launches(&mut app, dev, &store_plan());
    std::fs::write(dir.join("ready"), format!("{}", app.hot.counters.closed)).unwrap();
    loop {
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn outcome_store_survives_sigkill() {
    let dir = tmpdir("filter-store");
    // not from `new_unique`: the child process's own counter would hand the
    // same key out again (as a mint)
    let dev = Pubkey::new_from_array([0xab; 32]);
    std::fs::write(dir.join("dev"), dev.to_string()).unwrap();
    let model = model_no_delay(&dir);
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "outcome_store_child", "--ignored", "--nocapture"])
        .env(CHILD, &dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // wait for the four outcome rows to be committed by the live process
    let t0 = Instant::now();
    loop {
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "no outcome rows in state.db"
        );
        if let Some(st) = child.try_wait().unwrap() {
            let mut err = String::new();
            std::io::Read::read_to_string(child.stderr.as_mut().unwrap(), &mut err).unwrap();
            panic!("child exited early: {st:?}\n{err}");
        }
        let n = rusqlite::Connection::open_with_flags(
            dir.join("state.db"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .ok()
        .and_then(|c| {
            c.query_row("SELECT COUNT(*) FROM outcomes", [], |r| r.get::<_, i64>(0))
                .ok()
        })
        .unwrap_or(0);
        if n >= 4 {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    child.kill().unwrap(); // SIGKILL
    let st = child.wait().unwrap();
    assert_eq!(st.signal(), Some(9));
    // what the dead process left: four launches of `dev`, two LEAD results
    let conn = rusqlite::Connection::open(dir.join("state.db")).unwrap();
    let rows = rugsnipe_state::store::load_outcomes(&conn).unwrap();
    assert_eq!(rows.len(), 4);
    let store = OutcomeStore::from_rows(&rows);
    let s = store.signer(&dev);
    assert_eq!((s.n_launch, s.n_lead), (4, 2), "{s:?}");
    let outcomes: Vec<&str> = rows.iter().map(|r| r.outcome.as_str()).collect();
    assert_eq!(
        outcomes.iter().filter(|o| **o == "filled").count(),
        2,
        "{outcomes:?}"
    );
    assert!(outcomes.contains(&"premium") && outcomes.contains(&"not_in_slot0"));
    // the restart reloads exactly that store before it decides anything
    let cfg = filter_cfg(&dir, &model, FilterMode::Log);
    let app = build(cfg, armset(&[dev], &[]), &dir, 5_000, false);
    std::thread::sleep(Duration::from_millis(100));
    let summary = app.shutdown(vec![]);
    let f = &summary.cold.paper.filter;
    assert_eq!(
        (f.store_rows_loaded, f.store_rows, f.store_signers),
        (4, 4, 1)
    );
    let reloaded = OutcomeStore::from_rows(
        &rugsnipe_state::store::load_outcomes(
            &rusqlite::Connection::open(dir.join("state.db")).unwrap(),
        )
        .unwrap(),
    );
    assert_eq!(reloaded, store);
    let _ = std::fs::remove_dir_all(&dir);
}
