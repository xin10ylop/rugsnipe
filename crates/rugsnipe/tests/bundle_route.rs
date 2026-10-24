//! The revert-protected bundle copy end to end (feature `real-senders`): a
//! probe-mode process with a mock Helius sender and a mock Nozomi Send
//! Bundle endpoint fires one launch with the leader schedule unknown, so the
//! primary variant bids for a tip-ordered leader and the secondary for a
//! Harmonic one (`HotCfg::bid_plans_at`). Helius gets both plain copies;
//! the bundle endpoint gets exactly one request: the secondary as a
//! one-transaction bundle (`u16 BE len ‖ tx` on `/api/sendBundle?c=KEY`,
//! the key shared with `nozomi`) with the whole bid as its priority fee and
//! the 0.001 SOL Nozomi tip, on the intent's durable nonce, signed apart
//! from every plain copy. The ledger books what was sent.
#![cfg(feature = "real-senders")]

mod common;

use common::*;
use pump_codec::Pubkey;
use rugsnipe::app::{self, AppOpts, ArmChoice, RealOpts};
use rugsnipe_core::config::ProbeStage;
use rugsnipe_core::Mode;
use rugsnipe_feed::{FeedBuf, FeedSrc, Payload};
use rugsnipe_state::chain::{ChainRead, TokenBalance};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
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

/// `(request line, body)` of every request.
type Raw = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

/// An HTTP/1.1 keep-alive sink that answers `200` with an empty body (Nozomi
/// Send Bundle's "queued").
fn serve_raw() -> (String, Raw) {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let got: Raw = Arc::default();
    let g = got.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(s) = s else { continue };
            let g = g.clone();
            std::thread::spawn(move || {
                let mut w = s.try_clone().unwrap();
                let mut r = BufReader::new(s);
                loop {
                    let (mut first, mut len) = (String::new(), 0usize);
                    loop {
                        let mut line = String::new();
                        if r.read_line(&mut line).unwrap_or(0) == 0 {
                            return;
                        }
                        if line == "\r\n" {
                            break;
                        }
                        if first.is_empty() {
                            first = line.trim_end().to_string();
                        }
                        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                            len = v.trim().parse().unwrap_or(0);
                        }
                    }
                    let mut body = vec![0u8; len];
                    if r.read_exact(&mut body).is_err() {
                        return;
                    }
                    g.lock().unwrap().push((first, body));
                    if w.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                        .is_err()
                    {
                        return;
                    }
                }
            });
        }
    });
    (url, got)
}

#[test]
fn the_harmonic_variant_goes_out_once_as_a_bundle_bidding_the_whole_bid_as_priority() {
    let dir = tmpdir("bundle-route");
    let creds = dir.join("creds");
    std::fs::create_dir_all(&creds).unwrap();
    write_mode(&creds, "probe-keypair", &keypair_json(23), 0o400);
    write_mode(
        &creds,
        "api-keys",
        "HELIUS_KEY = \"hk-secret\"\nNOZOMI_KEY = \"nk-secret\"\n",
        0o400,
    );
    let owner =
        rugsnipe_tx::signer::HotSigner::from_seed(&zeroize::Zeroizing::new([23; 32])).pubkey();
    let signing = rugsnipe_tx::signer::HotSigner::from_seed(&zeroize::Zeroizing::new([98; 32]));

    // plain copies: a mock Helius (JSON-RPC sendTransaction)
    let plain: Arc<Mutex<Vec<Vec<u8>>>> = Arc::default();
    let p2 = plain.clone();
    let (helius_url, _) = serve_rpc(move |method, params| {
        assert_eq!(method, "sendTransaction");
        use base64::Engine as _;
        let tx = base64::engine::general_purpose::STANDARD
            .decode(params[0].as_str().unwrap())
            .unwrap();
        p2.lock().unwrap().push(tx);
        Ok(json!("accepted"))
    });
    // the bundle copy: a mock Nozomi Send Bundle
    let (bundle_url, bundles) = serve_raw();
    // the lane's RPC: nothing has landed
    let (rpc_url, _) = serve_rpc(|method, params| {
        Ok(match method {
            "getSignatureStatuses" => {
                let n = params[0].as_array().map_or(0, |a| a.len());
                json!({"context": {"slot": 1_002}, "value": vec![Value::Null; n]})
            }
            "getMultipleAccounts" => {
                let n = params[0].as_array().map_or(0, |a| a.len());
                json!({"context": {"slot": 1_002}, "value": vec![Value::Null; n]})
            }
            "getBalance" => json!({"context": {"slot": 1_002}, "value": 2_000_000_000u64}),
            "getLatestBlockhash" => json!({"context": {"slot": 1_002},
                "value": {"blockhash": bs58::encode([5u8; 32]).into_string()}}),
            "getSignaturesForAddress" => json!([]),
            "getTransaction" => Value::Null,
            "getBlock" => json!({"signatures": []}),
            other => return Err(format!("unexpected {other}")),
        })
    });

    let mut cfg = paper_cfg(&dir);
    cfg.mode.mode = Mode::Probe;
    cfg.probe.stage = ProbeStage::A;
    cfg.tx.cu_limit = 88_000;
    cfg.tx.lads_bytes = 12_124_160;
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
    // the plain Nozomi sender stays off; its key name is shared
    cfg.senders.nozomi.key = "NOZOMI_KEY".into();
    cfg.senders.nozomi_bundle.enabled = true;
    cfg.senders.nozomi_bundle.url = bundle_url;
    cfg.bid.lamports = 6_000_000;
    let dev = Pubkey::new_unique();
    let mut set = armset(&[dev], &[]);
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
    .expect("probe builds with the bundle leg on the shared key");
    let labels: Vec<&str> = app.senders.real().iter().map(|s| s.label()).collect();
    assert_eq!(labels, ["nozomi_bundle", "helius"]);

    let mint = Pubkey::new_unique();
    let (hit, _) = create(mint, dev, 1_001, 1, 1_000_000_000);
    let mut feed = VecFeed::new(vec![rugsnipe_feed::FeedEvent {
        src: FeedSrc::Grpc,
        ..common::ev(1_001, Payload::Hit(hit))
    }]);
    feed.end_when_empty = false;
    let mut buf = FeedBuf::with_capacity(64, 1 << 16);
    let t0 = Instant::now();
    while plain.lock().unwrap().len() < 2 || bundles.lock().unwrap().is_empty() {
        assert!(
            t0.elapsed() < Duration::from_secs(15),
            "{:?}",
            app.hot.counters
        );
        app.hot.step(&mut feed, &mut buf);
        std::thread::sleep(Duration::from_millis(2));
    }
    for _ in 0..50 {
        app.hot.step(&mut feed, &mut buf);
        std::thread::sleep(Duration::from_millis(2));
    }
    let plain = plain.lock().unwrap().clone();
    let bundles = bundles.lock().unwrap().clone();
    assert_eq!(plain.len(), 2, "both plain variants to Helius");
    assert_eq!(bundles.len(), 1, "one bundle: the Harmonic variant only");

    // the request: Send Bundle with the shared key, one length-prefixed member
    let (line, body) = &bundles[0];
    assert!(
        line.starts_with("POST /api/sendBundle?c=nk-secret "),
        "{line}"
    );
    let n = u16::from_be_bytes([body[0], body[1]]) as usize;
    assert_eq!(n + 2, body.len(), "exactly one member");
    let tx = &body[2..];
    let view = pump_codec::tx::TxView::parse(tx).unwrap();
    let keys = view.static_account_keys();
    assert_eq!(keys[0], owner);
    assert!(
        keys.iter()
            .any(|k| rugsnipe_tx::senders::providers::NOZOMI_TIP_ACCOUNTS.contains(k)),
        "the tip goes to a Nozomi account"
    );
    assert!(rugsnipe_tx::signer::verify(
        &owner,
        &tx[65..],
        tx[1..65].try_into().unwrap()
    ));
    // on the intent's durable nonce, like the plain copies, with its own signature
    for p in &plain {
        let pv = pump_codec::tx::TxView::parse(p).unwrap();
        assert_eq!(pv.recent_blockhash(), view.recent_blockhash());
        assert_ne!(&p[1..65], &tx[1..65]);
    }

    let _ = app.shutdown(vec![]);
    let conn = rusqlite::Connection::open(dir.join("state.db")).unwrap();
    let rows: Vec<(String, i64, i64, i64, i64)> = conn
        .prepare("SELECT sender, variant, prio, tip, bid FROM sends WHERE kind = 'buy' ORDER BY sender, variant")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(rows.len(), 3, "{rows:?}");
    let b = rows.iter().find(|r| r.0 == "nozomi_bundle").unwrap();
    assert_eq!(
        (b.1, b.2, b.3, b.4),
        (1, 6_000_000, 1_000_000, 6_000_000),
        "the secondary, the whole bid as priority, the Nozomi minimum tip"
    );
    // the plain copies keep their plans: primary tip-ordered (bid as tip),
    // secondary priority-ordered and clamped by the failed-attempt cap
    let h: Vec<_> = rows.iter().filter(|r| r.0 == "helius").collect();
    assert_eq!((h[0].1, h[0].3), (0, 6_000_000));
    assert_eq!(h[1].1, 1);
    assert!(
        h[1].2 < 6_000_000,
        "the plain secondary is clamped: {:?}",
        h[1]
    );
    let dump: String = conn
        .query_row("SELECT coalesce(group_concat(v), '') FROM kv", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(!dump.contains("nk-secret"), "keys never reach the ledger");
    let _ = std::fs::remove_dir_all(&dir);
}
