//! Kill triggers end to end in a probe process (feature `real-senders`;
//! `--probe-dry` assembly, so nothing can send): synthetic chain conditions
//! from a mock JSON-RPC
//!
//! * the wallet falls by 1 SOL between two `getBalance` reads with no
//!   intent in flight: `unexplained_drop`;
//! * pump's programdata head (deployed slot) changes between two reads:
//!   `program_upgrade`;
//!
//! latch the kill (buys stop), write `kv.kill_trigger:<name>` rows and the
//! persisted `kv.kill_latch`; a new process on the same `state.db` starts
//! killed (restored before any decision), and only the local unkill clears
//! it. The per-trigger evaluators (send errors over 50 sends, disk, clock,
//! drift, feed stale, the marked-to-market daily loss) have their own
//! synthetic-condition tests in `rugsnipe::triggers`.
#![cfg(feature = "real-senders")]

mod common;

use common::*;
use pump_codec::Pubkey;
use rugsnipe::app::{self, AppOpts, ArmChoice, RealOpts};
use rugsnipe_core::Mode;
use rugsnipe_feed::{FeedBuf, FeedSrc, Payload};
use rugsnipe_state::chain::{ChainRead, TokenBalance};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::*};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct MockChain {
    owner: Pubkey,
}

impl ChainRead for MockChain {
    fn slot(&self) -> Result<u64, String> {
        Ok(1_000)
    }
    fn token_balances(&self, _o: &Pubkey) -> Result<Vec<TokenBalance>, String> {
        Ok(vec![])
    }
    fn accounts(&self, keys: &[Pubkey]) -> Result<Vec<Option<Vec<u8>>>, String> {
        Ok(keys
            .iter()
            .enumerate()
            .map(|(i, _)| Some(nonce_account_data(&self.owner, [i as u8 + 1; 32])))
            .collect())
    }
    fn balance(&self, _k: &Pubkey) -> Result<u64, String> {
        Ok(2_000_000_000)
    }
}

fn programdata_b64(slot: u64) -> String {
    use base64::Engine as _;
    let mut b = vec![3, 0, 0, 0];
    b.extend_from_slice(&slot.to_le_bytes());
    b.push(1);
    b.extend_from_slice(&[7; 32]);
    base64::engine::general_purpose::STANDARD.encode(b)
}

fn tune(t: &mut rugsnipe::cold::TriggerCfg) {
    t.slot_check_ms = 200;
    t.upgrade_check_ms = 300;
}

fn probe_app(dir: &std::path::Path, rpc_url: String) -> (app::App, Pubkey) {
    probe_app_with(dir, rpc_url, None)
}

fn probe_app_with(
    dir: &std::path::Path,
    rpc_url: String,
    post_exec: Option<rugsnipe::cold::PostExecFeed>,
) -> (app::App, Pubkey) {
    let creds = dir.join("creds");
    std::fs::create_dir_all(&creds).unwrap();
    write_mode(&creds, "probe-keypair", &keypair_json(41), 0o400);
    write_mode(&creds, "api-keys", "HELIUS_KEY = \"hk\"\n", 0o400);
    let owner =
        rugsnipe_tx::signer::HotSigner::from_seed(&zeroize::Zeroizing::new([41; 32])).pubkey();
    let signing = rugsnipe_tx::signer::HotSigner::from_seed(&zeroize::Zeroizing::new([97; 32]));
    let mut cfg = paper_cfg(dir);
    cfg.mode.mode = Mode::Probe;
    cfg.tx.cu_limit = 88_000;
    cfg.tx.lads_bytes = 12_124_160;
    cfg.feeds.grpc.enabled = true;
    cfg.feeds.grpc.url = "https://grpc.invalid".into();
    cfg.feeds.grpc.stale_ms = 300;
    cfg.tx.nonce_pool = 4;
    cfg.secrets.probe_keypair = "probe-keypair".into();
    cfg.secrets.api_keys = "api-keys".into();
    cfg.secrets.armset_pubkey = signing.pubkey().to_string();
    cfg.secrets.cold_wallet = Pubkey::new_unique().to_string();
    cfg.senders.rpcsend.enabled = false;
    cfg.senders.helius.enabled = true;
    cfg.senders.helius.url = "http://127.0.0.1:9/fast".into();
    cfg.senders.helius.key = "HELIUS_KEY".into();
    let dev = Pubkey::new_unique();
    let mut set = armset(&[dev], &[]);
    set.probe_allowed = true;
    set.operators[0].size_lamports = 20_000_000;
    let armset_path = signed_armset(dir, &set, &signing);
    let app = app::build(AppOpts {
        cfg,
        armset: ArmChoice::File(armset_path),
        regime: None,
        paper_unsigned: false,
        state_path: Some(dir.join("state.db")),
        rslog_dir: dir.join("rslog"),
        logsws: None,
        healths: vec![],
        e2: None,
        metrics: false,
        fill_timeout_slots: 50,
        lease: true,
        sigusr1: Arc::new(AtomicBool::new(false)),
        paper_balance: 0,
        live_flag: false,
        real: Some(RealOpts {
            creds_dir: creds,
            clock_offset_ms: Some(0.4),
            post_exec_feed_healthy: false,
            dry: true,
            rpc_url,
            chain: Some(Box::new(MockChain { owner })),
        }),
        post_exec,
        leader_rpc: None,
        alerts: None,
        trigger_tuning: Some(tune),
    })
    .expect("a dry probe builds");
    (app, dev)
}

#[test]
fn chain_triggers_latch_persist_and_survive_a_restart() {
    let dir = tmpdir("kill-triggers");
    let balance_reads = Arc::new(AtomicU64::new(0));
    let pd_reads = Arc::new(AtomicU64::new(0));
    let (b, p) = (balance_reads.clone(), pd_reads.clone());
    let (rpc_url, _log) = serve_rpc(move |method, params| {
        Ok(match method {
            "getSignatureStatuses" | "getMultipleAccounts" => {
                json!({"value": params[0].as_array().unwrap().iter().map(|_| Value::Null).collect::<Vec<_>>()})
            }
            // 2 SOL, then 1 SOL: nothing was sent
            "getBalance" => {
                let n = b.fetch_add(1, AcqRel);
                json!({"value": if n == 0 { 2_000_000_000u64 } else { 1_000_000_000 }})
            }
            "getLatestBlockhash" => {
                json!({"context": {"slot": 1}, "value": {"blockhash": bs58::encode([1u8; 32]).into_string()}})
            }
            "getSlot" => json!(1_000),
            // pump redeployed between the first and the second read
            "getAccountInfo" => {
                let n = p.fetch_add(1, AcqRel);
                let slot = if n == 0 { 456_494_698u64 } else { 457_000_000 };
                json!({"value": {"data": [programdata_b64(slot), "base64"]}})
            }
            _ => Value::Null,
        })
    });
    let (mut app, dev) = probe_app(&dir, rpc_url.clone());
    assert!(!app.risk.is_killed());
    let mut feed = VecFeed::new(vec![]);
    feed.end_when_empty = false;
    let mut buf = FeedBuf::with_capacity(64, 1 << 16);
    let t0 = Instant::now();
    loop {
        app.hot.step(&mut feed, &mut buf);
        let r = app.report.lock().unwrap().clone();
        if r.kill_triggers.contains_key("unexplained_drop")
            && r.kill_triggers.contains_key("program_upgrade")
        {
            break;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "triggers: {:?} checks {:?}",
            r.kill_triggers,
            r.checks
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(app.risk.is_killed());
    // a create now is skipped by the latch (buys stop)
    let (hit, _) = create(Pubkey::new_unique(), dev, 1_001, 1, 1_000_000_000);
    let mut f2 = VecFeed::new(vec![rugsnipe_feed::FeedEvent {
        src: FeedSrc::Grpc,
        ..common::ev(1_001, Payload::Hit(hit))
    }]);
    for _ in 0..20 {
        app.hot.step(&mut f2, &mut buf);
    }
    assert_eq!(app.hot.counters.intents, 0);
    let summary = app.shutdown(vec![]);
    assert!(summary.cold.killed);
    assert!(summary.cold.kill_triggers["unexplained_drop"]
        .detail
        .contains("1000000000"));
    assert!(summary.cold.kill_triggers["program_upgrade"]
        .detail
        .contains("457000000"));
    let c = rugsnipe_state::store::open_conn(&dir.join("state.db")).unwrap();
    let latch = rugsnipe_state::killkv::latched(&c)
        .unwrap()
        .expect("persisted latch");
    assert!(
        ["unexplained_drop", "program_upgrade"].contains(&latch.reason.as_str()),
        "{latch:?}"
    );
    let trig: Vec<String> = rugsnipe_state::killkv::triggers(&c)
        .unwrap()
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert!(trig.contains(&"unexplained_drop".to_string()), "{trig:?}");
    assert!(trig.contains(&"program_upgrade".to_string()), "{trig:?}");
    assert_eq!(
        rugsnipe_state::store::kv_get(&c, "pump_programdata")
            .unwrap()
            .as_deref(),
        Some(format!("456494698:{}", Pubkey::new_from_array([7; 32])).as_str()),
        "the baseline an upgrade is measured against"
    );
    drop(c);
    // a restart on the same state.db is killed before any decision
    let (app, _) = probe_app(&dir, rpc_url.clone());
    assert!(app.risk.is_killed(), "restored from state.db");
    let s = app.shutdown(vec![]);
    assert!(s.cold.kill_restored.is_some());
    // the local unkill clears it
    let c = rugsnipe_state::store::open_conn(&dir.join("state.db")).unwrap();
    assert!(rugsnipe_state::killkv::unkill(&c, 1).unwrap().is_some());
    drop(c);
    let (app, _) = probe_app(&dir, rpc_url);
    assert!(!app.risk.is_killed());
    app.shutdown(vec![]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The post-execution feed in a probe process: while it delivers, a free
/// feed's copy of a create is recorded only (`free_feed_defers`) and the
/// gRPC copy decides; when its slot trails `getSlot` by more than 8 slots
/// (`drift`) or it stops delivering for `stale_ms` (`feed_stale`), buys
/// pause (never the kill latch, no trigger rows), and the pause lifts on
/// its own once the feed delivers and keeps up again for twice `stale_ms`.
#[test]
fn a_stale_or_drifting_post_exec_feed_pauses_buys_until_it_is_back() {
    use rugsnipe_feed::FeedHealth;
    let dir = tmpdir("kill-feed-stale");
    let (rpc_url, _log) = serve_rpc(move |method, params| {
        Ok(match method {
            "getSignatureStatuses" | "getMultipleAccounts" => {
                json!({"value": params[0].as_array().unwrap().iter().map(|_| Value::Null).collect::<Vec<_>>()})
            }
            "getBalance" => json!({"value": 2_000_000_000u64}),
            "getLatestBlockhash" => {
                json!({"context": {"slot": 1}, "value": {"blockhash": bs58::encode([1u8; 32]).into_string()}})
            }
            "getSlot" => json!(1_000),
            "getAccountInfo" => {
                json!({"value": {"data": [programdata_b64(456_494_698), "base64"]}})
            }
            _ => Value::Null,
        })
    });
    let health = Arc::new(FeedHealth::new(FeedSrc::Grpc, 300));
    health.connected.store(true, Relaxed);
    health.frame();
    let feeding = Arc::new(AtomicBool::new(true));
    let pump = {
        let (h, f) = (health.clone(), feeding.clone());
        std::thread::spawn(move || {
            while f.load(Acquire) {
                h.frame();
                std::thread::sleep(Duration::from_millis(30));
            }
        })
    };
    let feed = rugsnipe::cold::PostExecFeed {
        health: health.clone(),
        watch: Box::new(|_, _| {}),
        stats: Box::new(|| json!({})),
        bundles: None,
        follow: None,
    };
    let (mut app, dev) = probe_app_with(&dir, rpc_url, Some(feed));
    let mut buf = FeedBuf::with_capacity(64, 1 << 16);
    // the logs path sees one create first, the gRPC feed another
    let (free, _) = create(Pubkey::new_unique(), dev, 1_001, 1, 1_000_000_000);
    let (live, _) = create(Pubkey::new_unique(), dev, 1_001, 2, 1_000_000_000);
    let mut f = VecFeed::new(vec![
        rugsnipe_feed::FeedEvent {
            src: FeedSrc::LogsWs,
            ..common::ev(1_001, Payload::Hit(free))
        },
        rugsnipe_feed::FeedEvent {
            src: FeedSrc::Grpc,
            ..common::ev(1_001, Payload::Hit(live))
        },
    ]);
    f.end_when_empty = false;
    for _ in 0..100 {
        app.hot.step(&mut f, &mut buf);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        !app.risk.is_killed(),
        "{:?}",
        app.report.lock().unwrap().kill_triggers
    );
    assert_eq!(app.hot.counters.intents, 1, "only the gRPC copy decides");
    let d = app.stats.decision_counts();
    assert_eq!(
        d.iter()
            .find(|(k, _)| *k == "free_feed_defers")
            .map(|x| x.1),
        Some(1),
        "{d:?}"
    );
    // the feed falls behind, then stops: buys pause, no kill
    health.last_slot.store(900, Relaxed);
    feeding.store(false, Release);
    let _ = pump.join();
    let t0 = Instant::now();
    loop {
        app.hot.step(&mut f, &mut buf);
        let r = app.report.lock().unwrap().clone();
        if r.feed_paused.is_some() && app.shared.feed_paused.load(Relaxed) {
            break;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "{:?} {:?}",
            r.feed_paused,
            r.checks
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    // both checks hold for a while: still paused, never killed
    let t1 = Instant::now();
    while t1.elapsed() < Duration::from_millis(1_500) {
        app.hot.step(&mut f, &mut buf);
        std::thread::sleep(Duration::from_millis(5));
    }
    {
        let r = app.report.lock().unwrap().clone();
        assert!(!app.risk.is_killed(), "{:?}", r.kill_triggers);
        assert!(r.kill_triggers.is_empty(), "{:?}", r.kill_triggers);
        assert!(app.shared.feed_paused.load(Relaxed));
        assert_eq!(r.feed_pauses, 1, "one pause, not one per check");
    }
    // the feed is back and keeps up: the pause lifts on its own
    health.last_slot.store(1_000, Relaxed);
    feeding.store(true, Release);
    let pump = {
        let (h, f) = (health.clone(), feeding.clone());
        std::thread::spawn(move || {
            while f.load(Acquire) {
                h.frame();
                std::thread::sleep(Duration::from_millis(30));
            }
        })
    };
    let t2 = Instant::now();
    loop {
        app.hot.step(&mut f, &mut buf);
        let r = app.report.lock().unwrap().clone();
        if r.feed_paused.is_none() && !app.shared.feed_paused.load(Relaxed) {
            break;
        }
        assert!(
            t2.elapsed() < Duration::from_secs(20),
            "{:?} {:?}",
            r.feed_paused,
            r.checks
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        t2.elapsed() >= Duration::from_millis(600),
        "lifts only after twice stale_ms fresh"
    );
    feeding.store(false, Release);
    let _ = pump.join();
    assert!(!app.risk.is_killed());
    let s = app.shutdown(vec![]);
    assert!(
        s.cold.kill_triggers.is_empty(),
        "{:?}",
        s.cold.kill_triggers
    );
    let c = rugsnipe_state::store::open_conn(&dir.join("state.db")).unwrap();
    assert!(rugsnipe_state::killkv::latched(&c).unwrap().is_none());
    assert!(rugsnipe_state::killkv::triggers(&c).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
