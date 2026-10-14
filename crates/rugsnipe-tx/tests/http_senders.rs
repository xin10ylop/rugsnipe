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
