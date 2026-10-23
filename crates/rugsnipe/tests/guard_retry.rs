//! The guard after the hot box dies (docs/06 section 8, docs/08 section 9):
//! the real `rugsnipe-hot` is SIGKILLed with positions open; the guard
//! (probe sell path against a mock sender) takes the exits lease over, its
//! first sell is dropped (the balance stays), it re-sends on a fresh
//! blockhash after `retry_ms`, the second lands (balance 0) and the
//! position is closed in `state.db`. A guard holding the lease hands it
//! back once the hot box's heartbeat returns.

mod common;

use common::*;
use pump_codec::Pubkey;
use rugsnipe::guard::{Guard, GuardAction, GuardCfg};
use rugsnipe_state::lease::Lease;
use rugsnipe_state::rows::*;
use rugsnipe_state::{Ledger, StateStore};
#[cfg(feature = "real-senders")]
use rusqlite::Connection;

fn cfg(dry_run: bool, grace_s: u64) -> GuardCfg {
    GuardCfg {
        owner: "guard:test".into(),
        takeover_after_ms: 5_000,
        drop_from_peak_bps: 3_000,
        grace_s,
        dry_run,
        retry_ms: 3_000,
        max_sell_spend: 20_000_000,
    }
}

#[cfg(feature = "real-senders")]
fn q(c: &Connection, sql: &str) -> i64 {
    c.query_row(sql, [], |r| r.get(0)).unwrap_or(-1)
}

#[test]
fn guard_hands_the_exits_lease_back_when_the_hot_heartbeat_returns() {
    let dir = tmpdir("guard-handback");
    let s = StateStore::open(dir.join("state.db")).unwrap();
    let l = Ledger::new(s.writer());
    let c = s.reader().unwrap();
    let m = Pubkey::new_unique();
    l.position(PositionRow {
        mint: m,
        intent_id: 5,
        opened_slot: 1,
        tokens: 1_000,
        cost: 100_000_000,
        v_fill: 32_000_000_000,
        peak_v: 40_000_000_000,
        bundle_json: "[]".into(),
        policy_json: "{}".into(),
        origin: Origin::Intent,
        state: PosState::Open,
        sell_spend: 0,
        closed_slot: None,
    });
    l.flush();
    let hc = s.reader().unwrap();
    let mut hot = Lease::new("exits", "hot:1");
    hot.acquire(&hc, 10_000, 5_000).unwrap();
    rugsnipe_state::lease::beat(&hc, rugsnipe_state::lease::HOT_HEARTBEAT, "hot:1", 10_000)
        .unwrap();
    let mut g = Guard::new(cfg(true, 600), None);
    let none = |_: &Pubkey| None;
    // the hot box stops: the guard takes over
    let acts = g.step(&c, 16_000, &none).unwrap();
    assert!(
        matches!(&acts[0], GuardAction::TookOver { epoch: 2, .. }),
        "{acts:?}"
    );
    // the stale heartbeat of the dead process does not count
    let acts = g.step(&c, 17_000, &none).unwrap();
    assert!(acts.is_empty(), "{acts:?}");
    assert!(g.holds_lease());
    // a restarted hot box beats again (it cannot take the fresh lease itself)
    let mut hot2 = Lease::new("exits", "hot:2");
    assert!(matches!(
        hot2.acquire(&hc, 18_000, 5_000).unwrap(),
        rugsnipe_state::lease::Acquire::Busy(_)
    ));
    rugsnipe_state::lease::beat(&hc, rugsnipe_state::lease::HOT_HEARTBEAT, "hot:2", 18_000)
        .unwrap();
    let acts = g.step(&c, 18_500, &none).unwrap();
    assert_eq!(
        acts,
        vec![GuardAction::ReleasedToHot {
            owner: "hot:2".into()
        }]
    );
    assert!(!g.holds_lease());
    // the hot box's next renew takes it at once
    assert!(matches!(
        hot2.acquire(&hc, 19_000, 5_000).unwrap(),
        rugsnipe_state::lease::Acquire::Held { epoch: 3, .. }
    ));
    let acts = g.step(&c, 19_500, &none).unwrap();
    assert!(
        matches!(&acts[..], [GuardAction::HotAlive { .. }]),
        "{acts:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(feature = "real-senders")]
#[test]
fn hot_sigkilled_guard_retries_a_dropped_sell_until_the_position_closes() {
    use std::os::unix::process::ExitStatusExt;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering::*};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    // --- a real hot process with open positions, SIGKILLed
    let dir = tmpdir("guard-retry");
    let (lines, users) = fixture_relay();
    let armset_path = dir.join("armset.json");
    std::fs::write(
        &armset_path,
        serde_json::to_vec(&armset(&users, &[])).unwrap(),
    )
    .unwrap();
    let state = dir.join("state.db");
    let stop = Arc::new(AtomicBool::new(false));
    let sock = dir.join("pp.sock");
    let relay = serve_relay(&sock, lines, stop.clone());
    let cfg1 = dir.join("paper.toml");
    std::fs::write(&cfg1, paper_toml(&dir, &sock, [9; 6])).unwrap();
    let mut child = hot_cmd(&cfg1, &armset_path, &state, 120)
        .arg("--fill-timeout-slots")
        .arg("2")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let t0 = Instant::now();
    loop {
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "no open position appeared"
        );
        if let Ok(c) = Connection::open(&state) {
            let _ = c.busy_timeout(Duration::from_millis(500));
            let open = q(
                &c,
                "SELECT COUNT(*) FROM positions WHERE state = 'open' AND tokens > 0",
            );
            let leased = q(
                &c,
                &format!(
                    "SELECT COUNT(*) FROM lease WHERE name = 'exits' AND owner LIKE '%:{pid}'"
                ),
            );
            if open >= 1 && leased == 1 {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    child.kill().unwrap();
    assert_eq!(child.wait().unwrap().signal(), Some(9));
    stop.store(true, Release);
    let _ = relay.join();
    let c = rugsnipe_state::store::open_conn(&state).unwrap();
    let open: Vec<(String, i64)> = {
        let mut st = c
            .prepare("SELECT mint, tokens FROM positions WHERE state = 'open' AND tokens > 0")
            .unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    assert!(!open.is_empty());
    // the guard sells one of them (the rest are closed here: the mock
    // sender's rate limit would only spread the test over more steps)
    c.execute(
        "UPDATE positions SET state = 'closed' WHERE mint != ?1",
        [&open[0].0],
    )
    .unwrap();
    let open = [open[0].clone()];
    // --- the chain: one balance per open mint; the first sell of the first
    // mint is dropped, every later send lands (its balance goes to 0)
    let balances: Arc<Mutex<std::collections::HashMap<Pubkey, u64>>> = Arc::new(Mutex::new(
        open.iter()
            .map(|(m, t)| (m.parse().unwrap(), *t as u64))
            .collect(),
    ));
    let first: Pubkey = open[0].0.parse().unwrap();
    let sends = Arc::new(AtomicU64::new(0));
    let hashes: Arc<Mutex<Vec<[u8; 32]>>> = Arc::default();
    let (b2, s2, h2) = (balances.clone(), sends.clone(), hashes.clone());
    let (url, _) = serve_rpc(move |_m, p| {
        use base64::Engine as _;
        let tx = base64::engine::general_purpose::STANDARD
            .decode(p[0].as_str().unwrap())
            .unwrap();
        let view = pump_codec::tx::TxView::parse(&tx).unwrap();
        h2.lock().unwrap().push(*view.recent_blockhash());
        let n = s2.fetch_add(1, AcqRel);
        let mut b = b2.lock().unwrap();
        // which mint: the one whose key the transaction names
        let keys = view.static_account_keys();
        let hit: Vec<Pubkey> = b.keys().filter(|m| keys.contains(m)).copied().collect();
        for m in hit {
            if !(m == first && n == 0) {
                b.insert(m, 0); // landed
            }
        }
        Ok(serde_json::json!("ok"))
    });
    let mut cfgt = rugsnipe_core::config::Config::default();
    rugsnipe::setup::paper_caps(&mut cfgt.risk);
    cfgt.mode.mode = rugsnipe_core::Mode::Probe;
    cfgt.tx.cu_limit = 88_000;
    cfgt.tx.lads_bytes = 12_124_160;
    cfgt.feeds.grpc.enabled = true;
    cfgt.feeds.grpc.url = "https://grpc.invalid".into();
    cfgt.secrets.probe_keypair = "probe-keypair".into();
    cfgt.secrets.armset_pubkey = Pubkey::new_unique().to_string();
    cfgt.secrets.cold_wallet = Pubkey::new_unique().to_string();
    cfgt.senders.rpcsend.enabled = false;
    cfgt.senders.helius.enabled = true;
    cfgt.senders.helius.url = format!("{url}fast");
    cfgt.senders.helius.max_rps = 50;
    let mut env = rugsnipe_core::config::ValidateEnv {
        clock_offset_ms: Some(0.1),
        post_exec_feed_healthy: true,
        armset: rugsnipe_core::config::ArmSetGate::Verified {
            gate_passed: false,
            probe_allowed: true,
        },
        ..Default::default()
    };
    env.credential_modes
        .insert("probe-keypair".into(), Some(0o400));
    let senders =
        Arc::new(rugsnipe_tx::senders::SenderSet::build(&cfgt, &env, &Default::default()).unwrap());
    let signer = Arc::new(rugsnipe::setup::throwaway_signer().unwrap());
    let pool = rugsnipe::setup::paper_nonce_pool(signer.pubkey(), 1);
    let t = rugsnipe::setup::templates(&cfgt, signer.pubkey(), &pool).unwrap();
    let hn = Arc::new(AtomicU8::new(0));
    let bal = balances.clone();
    let mut g = Guard::new(
        cfg(false, 0),
        Some(rugsnipe::guard::PaperSell {
            template: t.sell_final,
            signer: signer.clone(),
            paper: Arc::new(rugsnipe_tx::senders::paper::Paper::new(16)),
            tip_account: t.tip_account,
            bid: rugsnipe_core::bid::BidPolicy::from_cfg(&cfgt.bid),
            real: Some(rugsnipe::guard::GuardReal {
                senders,
                // a fresh blockhash per call
                blockhash: Arc::new(move || Some([hn.fetch_add(1, AcqRel) + 1; 32])),
                token_balance: Some(Arc::new(move |m| bal.lock().unwrap().get(m).copied())),
                proceeds: None,
                find_sale: None,
            }),
        }),
    );
    let none = |_: &Pubkey| None;
    // the dead process's heartbeat is > takeover_after_ms old
    let now = rugsnipe::now_ms() + 6_000;
    let acts = g.step(&c, now, &none).unwrap();
    assert!(matches!(&acts[0], GuardAction::TookOver { .. }), "{acts:?}");
    let sent: Vec<_> = acts
        .iter()
        .filter(|a| {
            matches!(
                a,
                GuardAction::SentReal {
                    try_no: 1,
                    accepted: 1,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(sent.len(), open.len(), "{acts:?}");
    // the first sell was dropped: the balance stays, no retry before retry_ms
    let acts = g.step(&c, now + 500, &none).unwrap();
    assert!(acts.is_empty(), "{acts:?}");
    assert_eq!(g.pending_sells(), 1);
    assert_eq!(
        q(&c, "SELECT COUNT(*) FROM positions WHERE state = 'open'"),
        1,
        "not closed on a send"
    );
    // retry_ms later: re-sent on a fresh blockhash; it lands
    let acts = g.step(&c, now + 3_100, &none).unwrap();
    assert!(
        matches!(&acts[..], [GuardAction::SentReal { mint, try_no: 2, .. }] if *mint == first),
        "{acts:?}"
    );
    let hs = hashes.lock().unwrap().clone();
    assert_eq!(hs.len(), open.len() + 1);
    assert_ne!(hs[0], hs[hs.len() - 1], "a fresh blockhash per try");
    let acts = g.step(&c, now + 3_600, &none).unwrap();
    assert_eq!(
        acts,
        vec![GuardAction::Closed {
            mint: first,
            tries: 2
        }]
    );
    assert_eq!(
        q(
            &c,
            "SELECT COUNT(*) FROM positions WHERE state IN ('open','exiting')"
        ),
        0
    );
    assert_eq!(
        q(
            &c,
            &format!("SELECT try_no FROM exits WHERE mint = '{first}' AND reason = 'guard_grace'")
        ),
        2
    );
    // nothing open: the lease is released
    assert_eq!(
        g.step(&c, now + 4_000, &none).unwrap(),
        vec![GuardAction::Released]
    );
    let _ = std::fs::remove_dir_all(&dir);
}
