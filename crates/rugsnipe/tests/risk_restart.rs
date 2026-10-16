//! The daily caps survive a restart (docs/08 sections 9 and 10): a probe
//! start reads today's spend back from `state.db` (failed attempts'
//! `fail_cost`, closed positions, real buys) and latches `daily_loss` when
//! the day is already at the cap; three starts without a clean shutdown in
//! an hour latch `crash_loop`.

mod common;

#[allow(unused_imports)]
use common::*;
#[cfg(feature = "real-senders")]
use pump_codec::Pubkey;
#[cfg(feature = "real-senders")]
use rugsnipe::app::{self, AppOpts, ArmChoice, RealOpts};
#[cfg(feature = "real-senders")]
use rugsnipe_core::risk::KillReason;
use rugsnipe_core::Mode;
#[cfg(feature = "real-senders")]
use rugsnipe_state::chain::{ChainRead, TokenBalance};
#[cfg(feature = "real-senders")]
use std::sync::atomic::AtomicBool;
#[cfg(feature = "real-senders")]
use std::sync::Arc;

#[cfg(feature = "real-senders")]
struct MockChain {
    owner: Pubkey,
}

#[cfg(feature = "real-senders")]
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

#[cfg(feature = "real-senders")]
fn probe_app(dir: &std::path::Path) -> (app::App, u64) {
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
    cfg.tx.nonce_pool = 4;
    cfg.secrets.probe_keypair = "probe-keypair".into();
    cfg.secrets.api_keys = "api-keys".into();
    cfg.secrets.armset_pubkey = signing.pubkey().to_string();
    cfg.secrets.cold_wallet = Pubkey::new_unique().to_string();
    cfg.senders.rpcsend.enabled = false;
    cfg.senders.helius.enabled = true;
    cfg.senders.helius.url = "http://127.0.0.1:9/fast".into();
    cfg.senders.helius.key = "HELIUS_KEY".into();
    let cap = cfg.effective_risk().max_daily_loss_lamports;
    let mut set = armset(&[Pubkey::new_unique()], &[]);
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
            rpc_url: "http://127.0.0.1:9".into(),
            chain: Some(Box::new(MockChain { owner })),
        }),
        post_exec: None,
        leader_rpc: None,
        alerts: None,
        trigger_tuning: None,
    })
    .expect("a dry probe builds");
    (app, cap)
}

#[cfg(feature = "real-senders")]
#[test]
fn todays_failed_cost_at_the_cap_latches_daily_loss_at_start() {
    let dir = tmpdir("risk-restart");
    // a first start creates state.db; nothing spent yet
    let (a, cap) = probe_app(&dir);
    assert!(!a.risk.is_killed());
    let _ = a.shutdown(vec![]);
    // an earlier run of today: failed attempts whose fail_cost adds up to the cap
    let c = rugsnipe_state::store::open_conn(&dir.join("state.db")).unwrap();
    let now_ms = rugsnipe::now_ms();
    for i in 0..2u64 {
        c.execute(
            "INSERT INTO intents(intent_id, mint, op_id, mode, state) VALUES(?1, ?2, 1, 'probe', 'failed')",
            rusqlite::params![900 + i as i64, format!("mint{i}")],
        )
        .unwrap();
        c.execute(
            "INSERT INTO sends(sig, intent_id, sent_unix_ms, kind) VALUES(?1, ?2, ?3, 'buy')",
            rusqlite::params![format!("s{i}"), 900 + i as i64, now_ms as i64 - 1_000],
        )
        .unwrap();
        c.execute(
            "INSERT INTO fills(sig, intent_id, fail_cost) VALUES(?1, ?2, ?3)",
            rusqlite::params![format!("f{i}"), 900 + i as i64, (cap / 2 + i) as i64],
        )
        .unwrap();
    }
    drop(c);
    let (a, _) = probe_app(&dir);
    assert_eq!(
        a.risk.kill_reason(),
        Some(KillReason::DailyLoss),
        "the restart comes back with today's loss"
    );
    assert!(a.risk.realized_today(rugsnipe::now_unix()) <= -(cap as i64));
    // the cold thread reports it on its next tick
    let t0 = std::time::Instant::now();
    while !a.report.lock().unwrap().killed && t0.elapsed() < std::time::Duration::from_secs(5) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let s = a.shutdown(vec![]);
    assert!(s.cold.killed);
    let c = rugsnipe_state::store::open_conn(&dir.join("state.db")).unwrap();
    let latch = rugsnipe_state::killkv::latched(&c)
        .unwrap()
        .expect("persisted");
    assert_eq!(latch.reason, "daily_loss");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(feature = "real-senders")]
#[test]
fn three_unclean_starts_in_an_hour_latch_the_kill() {
    let dir = tmpdir("crash-loop");
    let (a, _) = probe_app(&dir);
    assert!(!a.risk.is_killed());
    let _ = a.shutdown(vec![]);
    // two crashes already (no clean shutdown since), and the last run crashed too
    let c = rugsnipe_state::store::open_conn(&dir.join("state.db")).unwrap();
    let now = rugsnipe::now_unix();
    c.execute_batch(&format!(
        "INSERT OR REPLACE INTO kv(k, v) VALUES('unclean_starts', '{},{}'), ('run_open', '{}');",
        now - 1_200,
        now - 600,
        now - 300
    ))
    .unwrap();
    drop(c);
    let (a, _) = probe_app(&dir);
    assert_eq!(a.risk.kill_reason(), Some(KillReason::CrashLoop));
    let _ = a.shutdown(vec![]);
    let c = rugsnipe_state::store::open_conn(&dir.join("state.db")).unwrap();
    assert_eq!(
        rugsnipe_state::killkv::latched(&c).unwrap().unwrap().reason,
        "crash_loop"
    );
    assert!(
        rugsnipe_state::store::kv_get(&c, "run_open")
            .unwrap()
            .is_none(),
        "clean shutdown"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_real_mode_needs_its_kill_file_under_run_rugsnipe() {
    let mut cfg = rugsnipe_core::config::Config::default();
    rugsnipe::setup::paper_caps(&mut cfg.risk);
    cfg.mode.mode = Mode::Probe;
    let field = |c: &rugsnipe_core::config::Config| {
        c.validate(&Default::default())
            .refusals
            .iter()
            .any(|r| r.to_string().contains("risk.kill_file"))
    };
    assert!(!field(&cfg), "the default /run/rugsnipe/KILL is accepted");
    for bad in [
        "data/raw/paper/KILL",
        "KILL",
        "/etc/rugsnipe/KILL",
        "/run/rugsnipe/",
        "/run/rugsnipe/../etc/x",
    ] {
        cfg.risk.kill_file = bad.into();
        assert!(field(&cfg), "{bad} must be refused in probe");
    }
    // paper keeps relative kill files (the examples use them)
    cfg.mode.mode = Mode::Paper;
    cfg.risk.kill_file = "data/raw/paper/KILL".into();
    assert!(!field(&cfg));
}
