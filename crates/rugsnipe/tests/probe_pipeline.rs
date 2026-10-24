//! Probe mode end to end with mocks (feature `real-senders`): the process
//! assembled by `app::build` from systemd-style credentials (0400 probe
//! keypair and `api-keys`), a signed ArmSet with `probe_allowed` and
//! `probe_stage = "a"`, the nonce pool seeded from (mock) chain accounts,
//! a mock Helius sender and a mock JSON-RPC for the real fill lane.
//!
//! 1. A create on a live-class feed fires: both bid variants go to the
//!    sender (each re-targeted to a Helius tip account, signed with the
//!    probe key); the chain says it landed and **failed** (6042): no
//!    position, the attempt costs its `meta.fee` (daily loss), its position
//!    slot comes back.
//! 2. A second create fires; **no position opens on the send** nor on a
//!    third party's trade. Only when the lane finds our transaction landed
//!    with our own TradeEvent does the position open.
//! 3. The dev dumps: the exit engine's sell goes through the real sender on
//!    a recent blockhash (bid split by `BidPolicy::plan_sell`); the position
//!    closes only on our own sell TradeEvent.
//!
//! The `sends` / `fills` / `shadow` rows carry exactly what
//! `shadow_gate.py --probe` reads.
#![cfg(feature = "real-senders")]

mod common;

use common::*;
use pump_codec::curve::CurveState;
use pump_codec::Pubkey;
use rugsnipe::app::{self, AppOpts, ArmChoice, RealOpts};
use rugsnipe_core::config::ProbeStage;
use rugsnipe_core::Mode;
use rugsnipe_feed::{FeedBuf, FeedSrc, Payload};
use rugsnipe_state::chain::{ChainRead, TokenBalance};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
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

fn b58(s: &[u8]) -> String {
    bs58::encode(s).into_string()
}

/// The chain the lane reads: landed transactions by signature (slot, json),
/// the curve's signature list and block orders.
#[derive(Default)]
struct Chain {
    landed: HashMap<String, (u64, Value)>,
    sigs: Vec<Value>,
    blocks: HashMap<u64, Vec<String>>,
}

fn sig_of(tx: &[u8]) -> String {
    b58(&tx[1..65])
}

type Sent = Arc<Mutex<Vec<Vec<u8>>>>;

fn step_until(
    app: &mut app::App,
    feed: &mut VecFeed,
    buf: &mut FeedBuf,
    what: &str,
    log: &Arc<Mutex<Vec<String>>>,
    f: &dyn Fn(&app::App) -> bool,
) {
    let t0 = Instant::now();
    while !f(app) {
        assert!(
            t0.elapsed() < Duration::from_secs(15),
            "{what}: {:?} {:?}",
            app.hot.counters,
            log.lock()
                .unwrap()
                .iter()
                .rev()
                .take(10)
                .collect::<Vec<_>>()
        );
        app.hot.step(feed, buf);
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn n_sent(s: &Sent) -> usize {
    s.lock().unwrap().len()
}

#[test]
fn probe_attempts_open_only_on_our_own_trade_event_and_close_on_our_sell() {
    let dir = tmpdir("probe-pipeline");
    let creds = dir.join("creds");
    std::fs::create_dir_all(&creds).unwrap();
    write_mode(&creds, "probe-keypair", &keypair_json(21), 0o400);
    write_mode(&creds, "api-keys", "HELIUS_KEY = \"hk-secret\"\n", 0o400);
    let owner =
        rugsnipe_tx::signer::HotSigner::from_seed(&zeroize::Zeroizing::new([21; 32])).pubkey();
    let signing = rugsnipe_tx::signer::HotSigner::from_seed(&zeroize::Zeroizing::new([99; 32]));

    // the sender: a mock Helius (JSON-RPC sendTransaction); keep every tx
    let sent: Sent = Arc::default();
    let s2 = sent.clone();
    let (helius_url, _) = serve_rpc(move |method, params| {
        assert_eq!(method, "sendTransaction");
        use base64::Engine as _;
        let tx = base64::engine::general_purpose::STANDARD
            .decode(params[0].as_str().unwrap())
            .unwrap();
        s2.lock().unwrap().push(tx);
        Ok(json!("accepted"))
    });
    let chain: Arc<Mutex<Chain>> = Arc::default();
    let c2 = chain.clone();
    let (rpc_url, rpc_log) = serve_rpc(move |method, params| {
        let c = c2.lock().unwrap();
        Ok(match method {
            "getSignatureStatuses" => {
                let v: Vec<Value> = params[0]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| match c.landed.get(s.as_str().unwrap()) {
                        Some((slot, tx)) => json!({"slot": slot, "confirmations": 0,
                            "err": tx["meta"]["err"], "confirmationStatus": "confirmed"}),
                        None => Value::Null,
                    })
                    .collect();
                json!({"context": {"slot": 1_003}, "value": v})
            }
            "getTransaction" => c
                .landed
                .get(params[0].as_str().unwrap())
                .map(|x| x.1.clone())
                .unwrap_or(Value::Null),
            "getSignaturesForAddress" => Value::Array(c.sigs.clone()),
            "getBlock" => json!({"signatures":
                c.blocks.get(&params[0].as_u64().unwrap()).cloned().unwrap_or_default()}),
            "getMultipleAccounts" => {
                // nonce accounts and token accounts unknown: the lane waits
                let v: Vec<Value> = params[0]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|_| Value::Null)
                    .collect();
                json!({"context": {"slot": 1_003}, "value": v})
            }
            "getBalance" => json!({"context": {"slot": 1_003}, "value": 2_000_000_000u64}),
            "getLatestBlockhash" => {
                json!({"context": {"slot": 1_002}, "value": {"blockhash": b58(&[5; 32])}})
            }
            other => return Err(format!("unexpected {other}")),
        })
    });

    // config: probe stage a (1 open position), every cap, Helius at the mock
    let mut cfg = paper_cfg(&dir);
    cfg.mode.mode = Mode::Probe;
    cfg.probe.stage = ProbeStage::A;
    cfg.tx.cu_limit = 88_000;
    cfg.tx.lads_bytes = 12_124_160;
    // the post-execution feed is configured (RealOpts carries its health)
    cfg.feeds.grpc.enabled = true;
    cfg.feeds.grpc.url = "https://grpc.invalid".into();
    cfg.tx.nonce_pool = 8;
    cfg.secrets.probe_keypair = "probe-keypair".into();
    cfg.secrets.hot_keypair = "hot-keypair".into();
    cfg.secrets.api_keys = "api-keys".into();
    cfg.secrets.armset_pubkey = signing.pubkey().to_string();
    cfg.secrets.cold_wallet = Pubkey::new_unique().to_string();
    cfg.senders.rpcsend.enabled = false;
    cfg.senders.helius.enabled = true;
    cfg.senders.helius.url = format!("{helius_url}fast");
    cfg.senders.helius.key = "HELIUS_KEY".into();
    cfg.senders.helius.max_rps = 100;
    let (dev_a, dev_b) = (Pubkey::new_unique(), Pubkey::new_unique());
    let mut set = armset(&[dev_a, dev_b], &[]);
    set.probe_allowed = true;
    set.probe_stage = Some(ProbeStage::A);
    set.operators[0].size_lamports = 20_000_000;
    let armset_path = signed_armset(&dir, &set, &signing);

    let mut app = app::build(AppOpts {
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
            creds_dir: creds.clone(),
            clock_offset_ms: Some(0.4),
            post_exec_feed_healthy: true,
            dry: false,
            rpc_url,
            chain: Some(Box::new(MockChain { owner })),
        }),
        post_exec: None,
        leader_rpc: None,
        alerts: None,
        trigger_tuning: None,
    })
    .expect("probe builds with every gate passing");
    assert_eq!(app.owner, owner, "the probe wallet from the credential");
    assert_eq!(app.senders.real().len(), 1);
    assert_eq!(
        app.risk.caps().max_probe_lamports,
        20_000_000,
        "stage a narrows the cap"
    );
    assert_eq!(app.risk.caps().max_open_positions, 1);
    assert_eq!(app.nonces.free_count(), 8, "nonce pool seeded from chain");

    let ev = |slot, payload| rugsnipe_feed::FeedEvent {
        src: FeedSrc::Grpc,
        ..common::ev(slot, payload)
    };
    // two mints on two different nonces
    let mint_a = Pubkey::new_unique();
    let mint_b = loop {
        let m = Pubkey::new_unique();
        if app.nonces.index_for(&m) != app.nonces.index_for(&mint_a) {
            break m;
        }
    };
    let (hit_a, _) = create(mint_a, dev_a, 1_001, 1, 1_000_000_000);
    let (hit_b, mut curve_b) = create(mint_b, dev_b, 1_001, 2, 1_000_000_000);
    let mut feed = VecFeed::new(vec![ev(1_001, Payload::Hit(hit_a))]);
    feed.end_when_empty = false;
    let mut buf = FeedBuf::with_capacity(64, 1 << 16);

    // ---- 1. attempt A lands and fails (slippage): its meta.fee is the cost
    step_until(&mut app, &mut feed, &mut buf, "A sent", &rpc_log, &|_| {
        n_sent(&sent) >= 2
    });
    let a_txs: Vec<Vec<u8>> = sent.lock().unwrap().clone();
    assert_eq!(app.risk.open_positions(), 1);
    {
        let fail = tx_json(
            1_001,
            &[owner],
            &[],
            json!({"InstructionError": [3, {"Custom": 6042}]}),
            1_005_000,
        );
        chain
            .lock()
            .unwrap()
            .landed
            .insert(sig_of(&a_txs[1]), (1_001, fail));
    }
    step_until(&mut app, &mut feed, &mut buf, "A failed", &rpc_log, &|a| {
        a.hot.counters.real_failed == 1
    });
    assert_eq!(app.hot.counters.fills, 0);
    assert_eq!(
        app.risk.open_positions(),
        0,
        "the failed attempt's slot came back"
    );
    assert_eq!(
        app.risk.realized_today(rugsnipe::now_unix()),
        -1_005_000,
        "meta.fee counts toward the daily loss"
    );

    // ---- 2. attempt B: no position on the send or a third party's buy
    let other = trade(
        &mut curve_b.clone(),
        mint_b,
        Pubkey::new_unique(),
        1_002,
        50,
        300_000_000,
        0,
    );
    feed.events.push(ev(1_001, Payload::Hit(hit_b)));
    feed.events.push(ev(1_002, Payload::Trade(other)));
    step_until(&mut app, &mut feed, &mut buf, "B sent", &rpc_log, &|_| {
        n_sent(&sent) >= 4
    });
    for _ in 0..50 {
        app.hot.step(&mut feed, &mut buf);
        std::thread::sleep(Duration::from_millis(2));
    }
    let c = app.hot.counters;
    assert_eq!(c.intents, 2, "{c:?}");
    assert_eq!(
        c.fills, 0,
        "no position on a send or a third party's trade: {c:?}"
    );
    assert!(app
        .hot
        .position_states()
        .iter()
        .all(|(_, _, filled)| !filled));
    let b_txs: Vec<Vec<u8>> = sent.lock().unwrap()[2..4].to_vec();
    let sigs: HashSet<String> = sent.lock().unwrap().iter().map(|t| sig_of(t)).collect();
    assert_eq!(sigs.len(), 4, "one signature per variant");
    for tx in &b_txs {
        let view = pump_codec::tx::TxView::parse(tx).unwrap();
        let keys = view.static_account_keys();
        assert_eq!(keys[0], owner);
        assert!(keys
            .iter()
            .any(|k| rugsnipe_tx::senders::providers::HELIUS_TIP_ACCOUNTS.contains(k)));
        assert!(!keys
            .iter()
            .any(|k| pump_codec::constants::JITO_TIP_ACCOUNTS.contains(k)));
        assert!(rugsnipe_tx::signer::verify(
            &owner,
            &tx[65..],
            tx[1..65].try_into().unwrap()
        ));
    }
    // the chain: the create, then our primary at index 3 with our TradeEvent
    let create_b = b58(&hit_b.sig.0);
    let mut c0 = CurveState::fresh(dev_b);
    let tok = c0.tokens_for_sol(1_000_000_000);
    c0.apply_buy(tok, 1_000_000_000);
    let create_log = trade_log_line(&mint_b, &dev_b, true, 1_000_000_000, tok, &c0);
    let our_tok = curve_b.tokens_for_sol(20_000_000);
    curve_b.apply_buy(our_tok, 20_000_000);
    let our_log = trade_log_line(&mint_b, &owner, true, 20_000_000, our_tok, &curve_b);
    let b_sig = sig_of(&b_txs[0]);
    {
        let mut c = chain.lock().unwrap();
        c.landed.insert(
            create_b.clone(),
            (
                1_001,
                tx_json(1_001, &[dev_b, mint_b], &[create_log], Value::Null, 5_000),
            ),
        );
        c.landed.insert(
            b_sig.clone(),
            (
                1_001,
                tx_json(1_001, &[owner], &[our_log], Value::Null, 15_000),
            ),
        );
        c.sigs = vec![
            json!({"signature": b_sig, "slot": 1_001, "err": null}),
            json!({"signature": create_b, "slot": 1_001, "err": null}),
        ];
        c.blocks.insert(
            1_001,
            vec!["v0".into(), create_b.clone(), "v2".into(), b_sig.clone()],
        );
    }
    step_until(&mut app, &mut feed, &mut buf, "B open", &rpc_log, &|a| {
        a.hot.counters.fills == 1
    });
    assert_eq!(app.hot.counters.own_trades, 1);
    assert!(app
        .hot
        .position_states()
        .iter()
        .any(|(m, _, filled)| *m == mint_b && *filled));

    // ---- 3. the dev dumps: our sell through the real sender, close on our sell event
    let dump = trade(&mut curve_b.clone(), mint_b, dev_b, 1_003, 60, 0, tok / 2);
    feed.events.push(ev(1_003, Payload::Trade(dump)));
    step_until(
        &mut app,
        &mut feed,
        &mut buf,
        "sell sent",
        &rpc_log,
        &|_| n_sent(&sent) >= 5,
    );
    let sell_tx = sent.lock().unwrap()[4].clone();
    let view = pump_codec::tx::TxView::parse(&sell_tx).unwrap();
    // a final sell (the whole balance, account closed) rides the position's
    // sell nonce, so at most one of its copies executes (91573e9)
    assert_eq!(
        view.recent_blockhash(),
        &[9u8; 32],
        "a real final sell lives on the position's sell nonce"
    );
    assert_eq!(app.hot.counters.closed, 0, "a send never closes a position");
    let mut cs = curve_b;
    let out = cs.sell_sol_out(our_tok);
    cs.apply_sell(our_tok, out);
    let sell_log = trade_log_line(&mint_b, &owner, false, out, our_tok, &cs);
    chain.lock().unwrap().landed.insert(
        sig_of(&sell_tx),
        (
            1_004,
            tx_json(1_004, &[owner], &[sell_log], Value::Null, 15_000),
        ),
    );
    step_until(&mut app, &mut feed, &mut buf, "closed", &rpc_log, &|a| {
        a.hot.counters.closed == 1
    });
    assert_eq!(app.risk.open_positions(), 0);
    for _ in 0..100 {
        app.hot.step(&mut feed, &mut buf);
        std::thread::sleep(Duration::from_millis(5));
    }
    let summary = app.shutdown(vec![]);
    let p = &summary.cold.probe;
    assert_eq!(
        (p.attempts, p.landed, p.landed_failed, p.dropped),
        (2, 1, 1, 0),
        "{p:#?}"
    );
    assert_eq!(p.by_sender["helius"].landed, 1);
    assert_eq!(p.by_sender["helius"].landed_failed, 1);
    assert_eq!(p.by_variant["0"].landed, 1);
    assert_eq!(p.by_variant["1"].landed_failed, 1);
    assert_eq!(p.failed_cost_lamports, 1_005_000);
    assert_eq!(p.sells_landed, 1);
    assert_eq!(p.fail_reasons["slippage"], 1);

    // the rows shadow_gate.py --probe reads
    let conn = rusqlite::Connection::open(dir.join("state.db")).unwrap();
    let n = |sql: &str| count(&conn, sql);
    assert_eq!(
        n("SELECT COUNT(*) FROM intents WHERE mode = 'probe' AND state = 'landed'"),
        1
    );
    assert_eq!(
        n("SELECT COUNT(*) FROM intents WHERE mode = 'probe' AND state = 'failed'"),
        1
    );
    assert_eq!(
        n("SELECT COUNT(*) FROM sends WHERE sender = 'helius' AND kind = 'buy' AND result = 'accepted' AND bid > 0 AND sent_unix_ms > 0"),
        4
    );
    assert_eq!(n("SELECT COUNT(*) FROM sends WHERE kind = 'sell'"), 1);
    let (variant, sender, fee, tokens, fail): (i64, String, i64, i64, Option<String>) = conn
        .query_row(
            "SELECT variant, sender, fee, tokens, fail_reason FROM fills WHERE sig = ?1",
            [&b_sig],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!(
        (variant, sender.as_str(), fee, tokens as u64, fail),
        (0, "helius", 15_000, our_tok, None)
    );
    let (fail_cost, reason): (i64, String) = conn
        .query_row(
            "SELECT fail_cost, fail_reason FROM fills WHERE sig = ?1",
            [&sig_of(&a_txs[1])],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((fail_cost, reason.as_str()), (1_005_000, "slippage"));
    // every fill's signature is a sends row: the verdict's per-variant split
    assert_eq!(
        n("SELECT COUNT(*) FROM fills f JOIN sends v ON v.sig = f.sig"),
        2
    );
    let shadow: Vec<(String, String, i64)> = conn
        .prepare("SELECT model, outcome, cost FROM shadow ORDER BY outcome")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(
        shadow,
        vec![
            ("probe".to_string(), "filled".to_string(), 0),
            ("probe".to_string(), "slippage".to_string(), 1_005_000)
        ]
    );
    assert_eq!(
        n("SELECT COUNT(*) FROM positions WHERE state = 'closed'"),
        1
    );
    assert_eq!(
        n("SELECT COUNT(*) FROM exits WHERE tokens > 0 AND sol_delta > 0"),
        1
    );
    // keys never reach the ledger
    let dump: String = conn
        .query_row("SELECT group_concat(v) FROM kv", [], |r| r.get(0))
        .unwrap();
    assert!(!dump.contains("hk-secret"));
    if let Some(keep) = std::env::var_os("RUGSNIPE_KEEP_PROBE_DB") {
        std::fs::copy(dir.join("state.db"), keep).unwrap();
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `--probe-dry`: the same assembly with the post-execution feed waived;
/// every variant is signed with the real probe key and re-targeted per
/// sender, then recorded: no request reaches the sender, no transaction is
/// sent by the lane, and the self-test yields a signed buy and a burn.
#[test]
fn probe_dry_signs_with_the_real_key_and_sends_nothing() {
    let dir = tmpdir("probe-dry");
    let creds = dir.join("creds");
    std::fs::create_dir_all(&creds).unwrap();
    write_mode(&creds, "probe-keypair", &keypair_json(31), 0o400);
    write_mode(&creds, "api-keys", "HELIUS_KEY = \"hk\"\n", 0o400);
    let owner =
        rugsnipe_tx::signer::HotSigner::from_seed(&zeroize::Zeroizing::new([31; 32])).pubkey();
    let signing = rugsnipe_tx::signer::HotSigner::from_seed(&zeroize::Zeroizing::new([98; 32]));
    let requests: Sent = Arc::default();
    let r2 = requests.clone();
    let (helius_url, _) = serve_rpc(move |_m, _p| {
        r2.lock().unwrap().push(vec![]);
        Ok(json!("x"))
    });
    let (rpc_url, rpc_log) = serve_rpc(|method, params| {
        Ok(match method {
            "getSignatureStatuses" => {
                json!({"value": params[0].as_array().unwrap().iter().map(|_| Value::Null).collect::<Vec<_>>()})
            }
            "getMultipleAccounts" => {
                json!({"value": params[0].as_array().unwrap().iter().map(|_| Value::Null).collect::<Vec<_>>()})
            }
            "getBalance" => json!({"value": 0u64}),
            "getLatestBlockhash" => {
                json!({"context": {"slot": 1}, "value": {"blockhash": b58(&[1; 32])}})
            }
            _ => Value::Null,
        })
    });
    let mut cfg = paper_cfg(&dir);
    cfg.mode.mode = Mode::Probe;
    cfg.tx.cu_limit = 88_000;
    cfg.tx.lads_bytes = 12_124_160;
    // the post-execution feed is configured (RealOpts carries its health)
    cfg.feeds.grpc.enabled = true;
    cfg.feeds.grpc.url = "https://grpc.invalid".into();
    cfg.tx.nonce_pool = 4;
    cfg.secrets.probe_keypair = "probe-keypair".into();
    cfg.secrets.api_keys = "api-keys".into();
    cfg.secrets.armset_pubkey = signing.pubkey().to_string();
    cfg.secrets.cold_wallet = Pubkey::new_unique().to_string();
    cfg.senders.rpcsend.enabled = false;
    cfg.senders.helius.enabled = true;
    cfg.senders.helius.url = format!("{helius_url}fast");
    cfg.senders.helius.key = "HELIUS_KEY".into();
    let dev = Pubkey::new_unique();
    let mut set = armset(&[dev], &[]);
    set.probe_allowed = true;
    set.operators[0].size_lamports = 20_000_000;
    let armset_path = signed_armset(&dir, &set, &signing);
    let mut app = app::build(AppOpts {
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
            post_exec_feed_healthy: false, // waived for a dry run
            dry: true,
            rpc_url,
            chain: Some(Box::new(MockChain { owner })),
        }),
        post_exec: None,
        leader_rpc: None,
        alerts: None,
        trigger_tuning: None,
    })
    .expect("a dry probe builds without a post-execution feed");
    assert!(app.senders.dry);
    assert!(app.senders.real().is_empty(), "nothing can send");
    assert_eq!(app.senders.targets().len(), 1);
    // the self-test: a signed buy per sender and the nonce burn
    let (vs, burn) = app.dry_selftest();
    assert_eq!(vs.len(), 1);
    let (label, v) = &vs[0];
    assert_eq!(*label, "helius");
    assert!(rugsnipe_tx::signer::verify(
        &owner,
        &v.bytes()[65..],
        &v.sig
    ));
    assert!(v.tip_lamports >= 1_000_000, "the Helius floor");
    let view = pump_codec::tx::TxView::parse(v.bytes()).unwrap();
    assert_eq!(view.static_account_keys()[0], owner);
    let burn = burn.expect("a burn of the first nonce");
    assert_eq!(&burn[69..101], owner.as_ref());
    // a live-class create fires: the variants are signed and recorded only
    let (hit, _) = create(Pubkey::new_unique(), dev, 1_001, 1, 1_000_000_000);
    let mut feed = VecFeed::new(vec![rugsnipe_feed::FeedEvent {
        src: FeedSrc::Grpc,
        ..common::ev(1_001, Payload::Hit(hit))
    }]);
    feed.end_when_empty = false;
    let mut buf = FeedBuf::with_capacity(64, 1 << 16);
    for _ in 0..300 {
        app.hot.step(&mut feed, &mut buf);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(app.hot.counters.intents, 1);
    let summary = app.shutdown(vec![]);
    assert!(
        summary.paper_variants_recorded >= 3,
        "plan copies + per-sender variants"
    );
    assert!(
        requests.lock().unwrap().is_empty(),
        "no request reached the sender"
    );
    assert!(!rpc_log
        .lock()
        .unwrap()
        .iter()
        .any(|m| m == "sendTransaction"));
    let conn = rusqlite::Connection::open(dir.join("state.db")).unwrap();
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM sends WHERE sender = 'helius' AND result = 'recorded'"
        ),
        2
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM sends WHERE result = 'accepted'"
        ),
        0
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Recovery in probe: a token balance found on chain at start (an orphan:
/// no position row) is adopted with an exit owner, takes a position slot,
/// its balance is polled, and a zero balance closes it.
#[test]
fn recovery_adopts_chain_balances_and_closes_them_on_a_zero_balance() {
    struct Orphan {
        owner: Pubkey,
        mint: Pubkey,
    }
    impl ChainRead for Orphan {
        fn slot(&self) -> Result<u64, String> {
            Ok(2_000)
        }
        fn token_balances(&self, _o: &Pubkey) -> Result<Vec<TokenBalance>, String> {
            Ok(vec![TokenBalance {
                mint: self.mint,
                account: pump_codec::pda::ata_token_2022(&self.owner, &self.mint),
                amount: 5_000_000,
            }])
        }
        fn accounts(&self, keys: &[Pubkey]) -> Result<Vec<Option<Vec<u8>>>, String> {
            Ok(keys
                .iter()
                .map(|_| Some(nonce_account_data(&self.owner, [9; 32])))
                .collect())
        }
        fn balance(&self, _k: &Pubkey) -> Result<u64, String> {
            Ok(1_500_000_000)
        }
    }
    let dir = tmpdir("probe-adopt");
    let creds = dir.join("creds");
    std::fs::create_dir_all(&creds).unwrap();
    write_mode(&creds, "probe-keypair", &keypair_json(41), 0o400);
    let owner =
        rugsnipe_tx::signer::HotSigner::from_seed(&zeroize::Zeroizing::new([41; 32])).pubkey();
    let signing = rugsnipe_tx::signer::HotSigner::from_seed(&zeroize::Zeroizing::new([97; 32]));
    let mint = Pubkey::new_unique();
    let (helius_url, _) = serve_rpc(|_m, _p| Ok(json!("x")));
    let (rpc_url, _) = serve_rpc(|method, params| {
        Ok(match method {
            // the token account is gone: zero tokens
            "getMultipleAccounts" | "getSignatureStatuses" => {
                json!({"context": {"slot": 2_001}, "value": params[0].as_array().unwrap().iter().map(|_| Value::Null).collect::<Vec<_>>()})
            }
            "getBalance" => json!({"value": 1_500_000_000u64}),
            "getLatestBlockhash" => {
                json!({"context": {"slot": 2_001}, "value": {"blockhash": b58(&[2; 32])}})
            }
            _ => Value::Null,
        })
    });
    let mut cfg = paper_cfg(&dir);
    cfg.mode.mode = Mode::Probe;
    cfg.tx.cu_limit = 88_000;
    cfg.tx.lads_bytes = 12_124_160;
    // the post-execution feed is configured (RealOpts carries its health)
    cfg.feeds.grpc.enabled = true;
    cfg.feeds.grpc.url = "https://grpc.invalid".into();
    cfg.tx.nonce_pool = 2;
    cfg.secrets.probe_keypair = "probe-keypair".into();
    cfg.secrets.armset_pubkey = signing.pubkey().to_string();
    cfg.secrets.cold_wallet = Pubkey::new_unique().to_string();
    cfg.senders.rpcsend.enabled = false;
    cfg.senders.helius.enabled = true;
    cfg.senders.helius.url = format!("{helius_url}fast");
    let mut set = armset(&[Pubkey::new_unique()], &[]);
    set.probe_allowed = true;
    set.operators[0].size_lamports = 20_000_000;
    let armset_path = signed_armset(&dir, &set, &signing);
    let mut app = app::build(AppOpts {
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
            post_exec_feed_healthy: true,
            dry: false,
            rpc_url,
            chain: Some(Box::new(Orphan { owner, mint })),
        }),
        post_exec: None,
        leader_rpc: None,
        alerts: None,
        trigger_tuning: None,
    })
    .unwrap();
    let rec = app.recovery.clone().unwrap();
    assert_eq!(rec.orphans.len(), 1);
    assert!(rec.reconciled);
    assert_eq!(app.hot.positions(), 1, "the orphan got an exit owner");
    assert_eq!(app.risk.open_positions(), 1, "and a position slot");
    assert!(app
        .hot
        .position_states()
        .iter()
        .any(|(m, _, filled)| *m == mint && *filled));
    let mut feed = VecFeed::new(vec![]);
    feed.end_when_empty = false;
    let mut buf = FeedBuf::with_capacity(8, 1 << 12);
    let t0 = Instant::now();
    while app.hot.counters.closed == 0 {
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "{:?}",
            app.hot.counters
        );
        app.hot.step(&mut feed, &mut buf);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(app.hot.positions(), 0, "a zero balance closed it");
    assert_eq!(app.risk.open_positions(), 0);
    app.shutdown(vec![]);
    let _ = std::fs::remove_dir_all(&dir);
}
