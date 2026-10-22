//! Fixed-buffer HTTP senders against a local mock server (feature `senders`).
#![cfg(feature = "senders")]

use pump_codec::constants::JITO_TIP_ACCOUNTS;
use rugsnipe_tx::senders::http::{HttpConn, HttpTarget};
use rugsnipe_tx::senders::providers::*;
use rugsnipe_tx::senders::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;

/// Serve `responses` in order on one keep-alive connection; send each raw
/// request (head + body) back over the channel.
fn mock(responses: Vec<(u16, &'static str)>) -> (String, mpsc::Receiver<(String, Vec<u8>)>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut it = responses.into_iter();
        'outer: for stream in l.incoming() {
            let mut s = stream.unwrap();
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            loop {
                let mut head = String::new();
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 {
                        continue 'outer;
                    }
                    head.push_str(&line);
                    if line == "\r\n" {
                        break;
                    }
                }
                let len = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                let mut body = vec![0u8; len];
                r.read_exact(&mut body).unwrap();
                tx.send((head, body)).unwrap();
                let Some((status, resp)) = it.next() else {
                    return;
                };
                let out = format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{resp}", resp.len());
                s.write_all(out.as_bytes()).unwrap();
            }
        }
    });
    (format!("http://{addr}"), rx)
}

fn variant(tip_to: pump_codec::Pubkey, tip: u64) -> Variant {
    Variant::from_bytes(
        &[7u8; 200],
        [1; 64],
        9,
        0,
        VariantKind::Buy,
        0,
        tip,
        Some(tip_to),
    )
}

#[test]
fn jsonrpc_sender_format_keepalive_and_rpc_error() {
    let (url, rx) = mock(vec![
        (200, r#"{"jsonrpc":"2.0","result":"sig","id":1}"#),
        (
            200,
            r#"{"jsonrpc":"2.0","error":{"code":-32002,"message":"x"},"id":1}"#,
        ),
    ]);
    let t = HttpTarget::parse(&format!("{url}/fast"))
        .unwrap()
        .with_query("swqos_only=true");
    let s = HttpSender::new(
        SenderId::Helius,
        Wire::JsonRpc,
        t,
        &HELIUS_TIP_ACCOUNTS,
        5_000,
        vec![],
        0,
    );
    s.warm().unwrap();
    let r = s.send(&variant(HELIUS_TIP_ACCOUNTS[3], 5_000));
    assert!(matches!(r, SendResult::Accepted { .. }), "{r:?}");
    let (head, body) = rx.recv().unwrap();
    assert!(
        head.starts_with("POST /fast?swqos_only=true HTTP/1.1\r\n"),
        "{head}"
    );
    assert!(head.contains("Content-Type: application/json"));
    let j: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(j["method"], "sendTransaction");
    assert_eq!(j["params"][1]["skipPreflight"], true);
    assert_eq!(j["params"][1]["maxRetries"], 0);
    use base64::Engine as _;
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(j["params"][0].as_str().unwrap())
            .unwrap(),
        vec![7u8; 200]
    );
    // same connection, JSON-RPC error -> failure
    let r = s.send(&variant(HELIUS_TIP_ACCOUNTS[0], 6_000));
    assert!(matches!(r, SendResult::Failed(SendError::Rpc(_))), "{r:?}");
    rx.recv().unwrap();
    // wrong tip account: refused without a request
    let r = s.send(&variant(JITO_TIP_ACCOUNTS[0], 1_000_000));
    assert_eq!(r, SendResult::Failed(SendError::WrongTipAccount));
    assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
}

#[test]
fn nozomi_batch_body_and_breaker() {
    let (url, rx) = mock(vec![
        (200, "ok"),
        (500, "boom"),
        (500, "boom"),
        (500, "boom"),
        (500, "boom"),
        (500, "boom"),
    ]);
    let t = HttpTarget::parse(&url)
        .unwrap()
        .with_path("/api/sendBatch?c=KEY");
    let s = HttpSender::new(
        SenderId::Nozomi,
        Wire::NozomiBatch,
        t,
        &NOZOMI_TIP_ACCOUNTS,
        1_000_000,
        vec![],
        0,
    );
    let a = variant(NOZOMI_TIP_ACCOUNTS[0], 1_000_000);
    let b = Variant::from_bytes(
        &[8u8; 100],
        [2; 64],
        9,
        1,
        VariantKind::Buy,
        0,
        1_000_000,
        Some(NOZOMI_TIP_ACCOUNTS[5]),
    );
    assert!(s.send_many(&[&a, &b]).is_ok());
    let (head, body) = rx.recv().unwrap();
    assert!(head.starts_with("POST /api/sendBatch?c=KEY HTTP/1.1"));
    assert!(head.contains("application/octet-stream"));
    assert_eq!(&body[..2], &200u16.to_be_bytes());
    assert_eq!(&body[202..204], &100u16.to_be_bytes());
    assert_eq!(body.len(), 2 + 200 + 2 + 100);
    for _ in 0..5 {
        assert_eq!(s.send(&a), SendResult::Failed(SendError::Http(500)));
        rx.recv().unwrap();
    }
    assert_eq!(s.send(&a), SendResult::Failed(SendError::BreakerOpen));
    assert!(s.breaker().is_open(now_ms()));
}

#[test]
fn jito_rate_limit_and_nextblock_body() {
    let (url, rx) = mock(vec![(200, r#"{"result":"x"}"#)]);
    let t = HttpTarget::parse(&url)
        .unwrap()
        .with_path("/api/v1/transactions?bundleOnly=true");
    let s = HttpSender::new(
        SenderId::Jito,
        Wire::JsonRpc,
        t,
        &JITO_TIP_ACCOUNTS,
        1_000,
        vec![],
        1,
    );
    assert!(s.send(&variant(JITO_TIP_ACCOUNTS[2], 1_000)).is_ok());
    let (head, body) = rx.recv().unwrap();
    assert!(head.starts_with("POST /api/v1/transactions?bundleOnly=true "));
    let j: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(j["params"][1], serde_json::json!({"encoding":"base64"}));
    assert_eq!(
        s.send(&variant(JITO_TIP_ACCOUNTS[2], 1_000)),
        SendResult::Failed(SendError::RateLimited)
    );
    let mut out = vec![];
    jito_bundle_body(&mut out, &[&[1, 2, 3], &[4]]);
    let j: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(j["method"], "sendBundle");
    assert_eq!(j["params"][0], serde_json::json!(["AQID", "BA=="]));

    let (url, rx) = mock(vec![(200, "{}")]);
    let t = HttpTarget::parse(&url).unwrap().with_path("/api/v2/submit");
    let s = HttpSender::new(
        SenderId::NextBlock,
        Wire::NextBlock,
        t,
        &NEXTBLOCK_TIP_ACCOUNTS,
        1_000_000,
        vec![("Authorization".into(), "tok".into())],
        0,
    );
    assert!(s
        .send(&variant(NEXTBLOCK_TIP_ACCOUNTS[0], 1_000_000))
        .is_ok());
    let (head, body) = rx.recv().unwrap();
    assert!(head.contains("Authorization: tok\r\n"));
    let j: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(j["frontRunningProtection"], false);
    assert!(j["transaction"]["content"].is_string());
}

#[test]
fn reconnects_after_server_closes() {
    // first connection serves one request then closes; the client reconnects
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf).unwrap();
            s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
        }
    });
    let mut c = HttpConn::new(
        HttpTarget::parse(&format!("http://{addr}/x")).unwrap(),
        Duration::from_secs(2),
    );
    assert_eq!(c.post("/x", "text/plain", &[], b"a").unwrap(), 200);
    assert_eq!(c.last_body(), b"ok");
    assert!(!c.is_connected(), "Connection: close honoured");
    assert_eq!(c.post("/x", "text/plain", &[], b"b").unwrap(), 200);
}

#[test]
fn build_from_config_uses_api_keys() {
    let mut keys = ApiKeys::default();
    keys.insert("NOZOMI_KEY", "k1");
    let c = rugsnipe_core::config::HttpSenderCfg {
        enabled: true,
        key: "NOZOMI_KEY".into(),
        ..Default::default()
    };
    let s = build("nozomi", &c, &keys).unwrap();
    assert_eq!(s.id(), SenderId::Nozomi);
    assert_eq!(s.min_tip(), 1_000_000);
    assert_eq!(s.tip_accounts().len(), 17);
    assert!(!format!("{s:?}").contains("k1"), "keys never printed");
    let missing = rugsnipe_core::config::HttpSenderCfg {
        enabled: true,
        key: "NOPE".into(),
        ..Default::default()
    };
    assert!(build("nozomi", &missing, &keys).is_err());
    let https = rugsnipe_core::config::HttpSenderCfg {
        enabled: true,
        url: "https://x".into(),
        ..Default::default()
    };
    assert!(
        build("helius", &https, &keys).is_err(),
        "TLS endpoints are not for the fixed-buffer client"
    );
}

#[test]
fn blockrazor_fast_mode_body_and_apikey_header() {
    let (url, rx) = mock(vec![
        (200, r#"{"signature":"x","error":""}"#),
        (
            200,
            r#"{"signature":"","error":"rpc error: code = Unknown desc = illegal transaction"}"#,
        ),
        (200, r#"{"signature":"y"}"#),
        (200, r#"{"signature":"z"}"#),
    ]);
    let mut keys = ApiKeys::default();
    keys.insert("BLOCKRAZOR_KEY", "br-token");
    let c = rugsnipe_core::config::HttpSenderCfg {
        enabled: true,
        url: format!("{url}/sendTransaction"),
        key: "BLOCKRAZOR_KEY".into(),
        ..Default::default()
    };
    let s = build("blockrazor", &c, &keys).unwrap();
    assert_eq!((s.id(), s.label()), (SenderId::BlockRazor, "blockrazor"));
    assert_eq!(s.min_tip(), 100_000, "0.0001 SOL minimum");
    assert_eq!(s.tip_accounts(), &BLOCKRAZOR_TIP_ACCOUNTS);
    assert!(!format!("{s:?}").contains("br-token"));
    let r = s.send(&variant(BLOCKRAZOR_TIP_ACCOUNTS[4], 100_000));
    assert!(matches!(r, SendResult::Accepted { .. }), "{r:?}");
    let (head, body) = rx.recv().unwrap();
    assert!(
        head.starts_with("POST /sendTransaction HTTP/1.1\r\n"),
        "{head}"
    );
    assert!(head.contains("apikey: br-token\r\n"), "{head}");
    assert!(head.contains("Content-Type: application/json"));
    let j: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(j["mode"], "fast");
    assert_eq!(j["revertProtection"], false);
    use base64::Engine as _;
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(j["transaction"].as_str().unwrap())
            .unwrap(),
        vec![7u8; 200]
    );
    // an error body is a failure; a tip below 0.0001 SOL never leaves the box
    std::thread::sleep(Duration::from_millis(350));
    assert!(matches!(
        s.send(&variant(BLOCKRAZOR_TIP_ACCOUNTS[0], 100_000)),
        SendResult::Failed(SendError::Rpc(_))
    ));
    rx.recv().unwrap();
    // 3 TPS (burst 3): two more go out, the next is refused before the network
    for _ in 0..2 {
        assert!(s
            .send(&variant(BLOCKRAZOR_TIP_ACCOUNTS[1], 100_000))
            .is_ok());
        rx.recv().unwrap();
    }
    assert_eq!(
        s.send(&variant(BLOCKRAZOR_TIP_ACCOUNTS[1], 100_000)),
        SendResult::Failed(SendError::RateLimited),
        "3 TPS"
    );
    assert_eq!(
        s.send(&variant(BLOCKRAZOR_TIP_ACCOUNTS[0], 99_999)),
        SendResult::Failed(SendError::TipTooLow {
            min: 100_000,
            got: 99_999
        })
    );
    assert!(rx.recv_timeout(Duration::from_millis(150)).is_err());
    // the key is required
    assert!(build("blockrazor", &c, &ApiKeys::default()).is_err());
}

#[test]
fn helius_key_in_the_query_and_the_swqos_leg() {
    let (url, rx) = mock(vec![(200, r#"{"result":"a"}"#)]);
    let (url2, rx2) = mock(vec![(200, r#"{"result":"b"}"#)]);
    let mut keys = ApiKeys::default();
    keys.insert("HELIUS_KEY", "hk");
    let mut c = rugsnipe_core::config::HttpSenderCfg {
        enabled: true,
        url: format!("{url}/fast"),
        key: "HELIUS_KEY".into(),
        min_tip: 1_000, // below the provider minimum: raised to it
        ..Default::default()
    };
    let max = build("helius", &c, &keys).unwrap();
    assert_eq!((max.id(), max.min_tip()), (SenderId::Helius, 1_000_000));
    c.swqos_only = true;
    c.url = format!("{url2}/fast");
    let swqos = build("helius", &c, &keys).unwrap();
    assert_eq!(
        (swqos.id(), swqos.label(), swqos.min_tip()),
        (SenderId::HeliusSwqos, "helius_swqos", 5_000)
    );
    assert!(max
        .send(&variant(HELIUS_TIP_ACCOUNTS[0], 1_000_000))
        .is_ok());
    let (head, _) = rx.recv().unwrap();
    assert!(head.starts_with("POST /fast?api-key=hk HTTP/1.1"), "{head}");
    assert!(swqos.send(&variant(HELIUS_TIP_ACCOUNTS[1], 5_000)).is_ok());
    let (head, _) = rx2.recv().unwrap();
    assert!(
        head.starts_with("POST /fast?swqos_only=true&api-key=hk HTTP/1.1"),
        "{head}"
    );
    assert!(!format!("{max:?}").contains("hk"));
}

#[test]
fn jito_regions_get_their_own_sender_label_and_token_bucket() {
    let (fra, rx_fra) = mock(vec![(200, r#"{"result":"a"}"#), (200, r#"{"result":"b"}"#)]);
    let (ams, rx_ams) = mock(vec![(200, r#"{"result":"c"}"#)]);
    let mut cfg = rugsnipe_core::config::Config::default();
    cfg.senders.jito = rugsnipe_core::config::HttpSenderCfg {
        enabled: true,
        regions: vec!["frankfurt".into(), "amsterdam".into()],
        max_rps: 1,
        ..Default::default()
    };
    // the region builds the block-engine host unless a url is given
    let mut one = cfg.senders.jito.clone();
    one.region = "frankfurt".into();
    one.url = fra.clone();
    let f = build("jito", &one, &ApiKeys::default()).unwrap();
    one.region = "amsterdam".into();
    one.url = ams.clone();
    let a = build("jito", &one, &ApiKeys::default()).unwrap();
    assert_eq!((f.label(), a.label()), ("jito_frankfurt", "jito_amsterdam"));
    assert_eq!(
        f.buy_variants(),
        1,
        "Jito takes the pay-on-success primary only"
    );
    let v = variant(JITO_TIP_ACCOUNTS[3], 1_000);
    assert!(f.send(&v).is_ok());
    assert!(a.send(&v).is_ok(), "the other region has its own 1 req/s");
    assert_eq!(f.send(&v), SendResult::Failed(SendError::RateLimited));
    let (head, _) = rx_fra.recv().unwrap();
    assert!(
        head.starts_with("POST /api/v1/transactions?bundleOnly=true "),
        "{head}"
    );
    rx_ams.recv().unwrap();
    std::thread::sleep(Duration::from_millis(1_050));
    assert!(f.send(&v).is_ok(), "the bucket refills after a second");
    rx_fra.recv().unwrap();
    assert_eq!(
        f.send(&variant(JITO_TIP_ACCOUNTS[3], 999)),
        SendResult::Failed(SendError::TipTooLow {
            min: 1_000,
            got: 999
        })
    );
    assert!(cfg.senders.real_enabled().contains(&"jito"));
}

#[test]
fn nozomi_never_takes_a_tip_below_its_silent_drop_floor() {
    let mut keys = ApiKeys::default();
    keys.insert("NOZOMI_KEY", "nk");
    let c = rugsnipe_core::config::HttpSenderCfg {
        enabled: true,
        key: "NOZOMI_KEY".into(),
        min_tip: 10_000,
        ..Default::default()
    };
    let s = build("nozomi", &c, &keys).unwrap();
    assert_eq!(s.min_tip(), 1_000_000);
    assert_eq!(
        s.send(&variant(NOZOMI_TIP_ACCOUNTS[0], 999_999)),
        SendResult::Failed(SendError::TipTooLow {
            min: 1_000_000,
            got: 999_999
        })
    );
    // tip planning raises a low plan tip to the floor and rotates accounts
    let (acct, tip) = tip_plan(s.as_ref(), 2, 1, 7, 10).unwrap();
    assert_eq!(tip, 1_000_000);
    assert!(NOZOMI_TIP_ACCOUNTS.contains(&acct));
    let (_, tip) = tip_plan(s.as_ref(), 2, 0, 7, 3_400_000).unwrap();
    assert_eq!(tip, 3_400_000, "a bid above the floor is kept");
}

#[test]
fn token_bucket_rate_and_burst() {
    let b = TokenBucket::new(2.0, 1.0);
    let t0 = std::time::Instant::now();
    assert!(b.try_take_at(t0));
    assert!(!b.try_take_at(t0));
    assert!(!b.try_take_at(t0 + Duration::from_millis(400)));
    assert!(b.try_take_at(t0 + Duration::from_millis(510)));
    // never more than the burst is saved
    assert!(b.try_take_at(t0 + Duration::from_secs(10)));
    assert!(!b.try_take_at(t0 + Duration::from_secs(10)));
    assert!(TokenBucket::new(0.0, 1.0).try_take(), "0 = unlimited");
}
