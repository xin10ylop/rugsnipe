//! LogsWs against a local websocket server and a local HTTP JSON-RPC server
//! (no network): subscription handling, creates, watched mints, funding
//! lookups, the stale detector and reconnects.
#![cfg(feature = "ws")]

use futures_util::{SinkExt, StreamExt};
use rugsnipe_feed::ws::{LogsWs, LogsWsConfig};
use rugsnipe_feed::{Feed, FeedBuf, Payload};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

fn fixture(p: &str) -> String {
    std::fs::read_to_string(format!("{}/{p}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// Recorded create notifications (raw frames), re-addressed to `sub`.
fn create_frames(sub: u64) -> Vec<String> {
    fixture("fixtures/logs_frames.jsonl")
        .lines()
        .filter_map(|l| {
            let mut v: serde_json::Value = serde_json::from_str(l).unwrap();
            let f = v.get_mut("frame")?;
            if f["method"] != "logsNotification" {
                return None;
            }
            f["params"]["subscription"] = sub.into();
            Some(f.to_string())
        })
        .collect()
}

fn funder_notification(sub: u64, sig: &str) -> String {
    serde_json::json!({"jsonrpc":"2.0","method":"logsNotification","params":{"result":{"context":{"slot":1},
      "value":{"signature":sig,"err":null,"logs":["Program 11111111111111111111111111111111 invoke [1]","Program 11111111111111111111111111111111 success"]}},"subscription":sub}}).to_string()
}

#[test]
fn local_server_creates_funding_stale_and_reconnect() {
    let funding = fixture("fixtures/funding_get_transaction.json");
    let fv: serde_json::Value = serde_json::from_str(&funding).unwrap();
    let funder = fv["funder"].as_str().unwrap().to_string();
    let fsig = fv["result"]["transaction"]["signatures"][0]
        .as_str()
        .unwrap()
        .to_string();
    let rpc_body = format!(r#"{{"jsonrpc":"2.0","id":1,"result":{}}}"#, fv["result"]);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let connections = Arc::new(AtomicU64::new(0));
    let (ws_addr, rpc_addr) = rt.block_on(async {
        // JSON-RPC: answer every POST with the funding transaction
        let rpc = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let rpc_addr = rpc.local_addr().unwrap();
        let body = rpc_body.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = rpc.accept().await else { return };
                let body = body.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    while let Ok(n) = s.read(&mut buf).await {
                        if n == 0 {
                            break;
                        }
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        if s.write_all(resp.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                });
            }
        });
        // websocket: ack subscriptions, send creates and one funder notification, then go silent
        let ws = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ws_addr = ws.local_addr().unwrap();
        let conns = connections.clone();
        let funder = funder.clone();
        tokio::spawn(async move {
            loop {
                let Ok((s, _)) = ws.accept().await else { return };
                conns.fetch_add(1, Ordering::SeqCst);
                let funder = funder.clone();
                let fsig = fsig.clone();
                tokio::spawn(async move {
                    let mut ws = tokio_tungstenite::accept_async(s).await.unwrap();
                    let mut create_sub = 0;
                    let mut funder_sub = 0;
                    let mut acked = 0;
                    while acked < 2 {
                        let Some(Ok(Message::Text(t))) = ws.next().await else { return };
                        let v: serde_json::Value = serde_json::from_str(t.as_str()).unwrap();
                        let id = v["id"].as_u64().unwrap();
                        let mention = v["params"][0]["mentions"][0].as_str().unwrap().to_string();
                        let sub = 100 + id;
                        if mention == funder {
                            funder_sub = sub;
                        } else {
                            create_sub = sub;
                        }
                        ws.send(Message::Text(format!(r#"{{"jsonrpc":"2.0","result":{sub},"id":{id}}}"#).into()))
                            .await
                            .unwrap();
                        acked += 1;
                    }
                    for f in create_frames(create_sub) {
                        ws.send(Message::Text(f.into())).await.unwrap();
                    }
                    ws.send(Message::Text(funder_notification(funder_sub, &fsig).into()))
                        .await
                        .unwrap();
                    // dead silence (not even pongs: the socket is not read any more):
                    // the client's stale detector must reconnect
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    drop(ws);
                });
            }
        });
        (ws_addr, rpc_addr)
    });

    let cfg = LogsWsConfig {
        url: format!("ws://{ws_addr}"),
        stale_ms: 700,
        min_backoff_ms: 50,
        max_backoff_ms: 200,
        watch_funders: vec![funder.parse().unwrap()],
        rpc_url: Some(format!("http://{rpc_addr}/")),
        rpc_rps: 50.0,
        confirm_delay_ms: 10,
        ..Default::default()
    };
    let mut feed = LogsWs::spawn(cfg).unwrap();
    let expected_creates = create_frames(1)
        .iter()
        .filter(|f| f.contains("\"err\":null"))
        .count();
    let mut buf = FeedBuf::with_capacity(512, 0);
    let (mut hits, mut funds) = (0usize, 0usize);
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(15) {
        buf.clear();
        feed.poll(&mut buf);
        for ev in buf.events() {
            match ev.payload {
                Payload::Hit(_) => hits += 1,
                Payload::Funding(f) => {
                    funds += 1;
                    assert_eq!(f.funder.to_string(), funder);
                    assert!(f.lamports > 0);
                }
                _ => {}
            }
        }
        let h = feed.health().unwrap().snapshot();
        if h.stale_reconnects >= 1 && hits >= 2 * expected_creates && funds >= 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let h = feed.health().unwrap().snapshot();
    feed.shutdown();
    assert!(
        h.stale_reconnects >= 1,
        "stale detector did not fire: {h:?}"
    );
    assert!(connections.load(Ordering::SeqCst) >= 2, "no reconnect");
    assert!(
        hits >= 2 * expected_creates,
        "hits {hits}, expected {expected_creates} per connection"
    );
    assert!(funds >= 2, "funding events {funds}");
    assert_eq!(h.decode_miss, 0);
}

#[test]
fn quiet_but_alive_connection_is_not_stale() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let addr = rt.block_on(async {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((s, _)) = l.accept().await {
                tokio::spawn(async move {
                    let mut ws = tokio_tungstenite::accept_async(s).await.unwrap();
                    // ack the subscription, then send nothing; reading answers the pings
                    if let Some(Ok(Message::Text(t))) = ws.next().await {
                        let v: serde_json::Value = serde_json::from_str(t.as_str()).unwrap();
                        let id = v["id"].as_u64().unwrap();
                        let ack = format!(r#"{{"jsonrpc":"2.0","result":7,"id":{id}}}"#);
                        ws.send(Message::Text(ack.into())).await.unwrap();
                    }
                    while let Some(Ok(_)) = ws.next().await {}
                });
            }
        });
        addr
    });
    let mut feed = LogsWs::spawn(LogsWsConfig {
        url: format!("ws://{addr}"),
        stale_ms: 900,
        rpc_url: None,
        ..Default::default()
    })
    .unwrap();
    std::thread::sleep(Duration::from_millis(3500));
    let h = feed.health().unwrap().snapshot();
    feed.shutdown();
    assert!(h.connected, "{h:?}");
    assert_eq!(
        h.stale_reconnects, 0,
        "pongs keep a quiet socket fresh: {h:?}"
    );
    assert_eq!(h.reconnects, 0);
}
