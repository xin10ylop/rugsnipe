//! Cross-feed dedup through FeedMux, and a model check of SigDedup's window.

use proptest::prelude::*;
use rugsnipe_feed::replay::Replay;
use rugsnipe_feed::*;
use std::collections::{HashSet, VecDeque};

fn frames() -> String {
    std::fs::read_to_string(format!(
        "{}/fixtures/logs_frames.jsonl",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// The same recording as seen by another feed, `delay_ns` later.
fn shifted(text: &str, src: &str, delay_ns: u64) -> String {
    text.lines()
        .map(|l| {
            let mut v: serde_json::Value = serde_json::from_str(l).unwrap();
            v["src"] = src.into();
            v["recv_ns"] = (v["recv_ns"].as_u64().unwrap() + delay_ns).into();
            v.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn second_feed_copies_become_dups_with_delay() {
    let text = frames();
    let a = Replay::from_text(&text, false);
    let n = a.events().len();
    let b = Replay::from_text(&shifted(&text, "relay", 3_000_000), false);
    let mut mux = FeedMux::new(vec![Box::new(a), Box::new(b)]);
    let mut buf = FeedBuf::with_capacity(4 * n, 0);
    while !matches!(mux.poll(&mut buf), FeedStatus::Ended | FeedStatus::Full) {}
    let (mut hits, mut dups) = (0, 0);
    for e in buf.events() {
        match e.payload {
            Payload::Hit(_) => {
                hits += 1;
                assert_eq!(e.src, FeedSrc::LogsWs);
            }
            Payload::Dup(d) => {
                dups += 1;
                assert_eq!(e.src, FeedSrc::Relay);
                assert_eq!(d.first_src, FeedSrc::LogsWs);
                assert_eq!(d.delay_ns, 3_000_000);
            }
            _ => {}
        }
    }
    assert_eq!(hits, n);
    assert_eq!(dups, n);
    assert_eq!(mux.dups[FeedSrc::Relay as usize], n as u64);
    assert_eq!(mux.firsts[FeedSrc::LogsWs as usize], n as u64);
    assert_eq!(
        mux.dup_delay_ns[FeedSrc::Relay as usize],
        3_000_000 * n as u64
    );
}

#[test]
fn fired_set_gives_feed_lead() {
    let text = frames();
    let early = Replay::from_text(&shifted(&text, "pumpportal", 0), false);
    let late = Replay::from_text(&shifted(&text, "logsws", 5), false);
    let fired = FiredSet::default();
    let mut lead = 0;
    for r in [&early, &late] {
        for e in r.events() {
            if let Some(m) = e.mint() {
                if let Fired::Already { first_src } = fired.first(&m, 0, e.src) {
                    assert_eq!(first_src, FeedSrc::PumpPortal);
                    lead += 1;
                }
            }
        }
    }
    assert_eq!(lead, early.events().len());
}

proptest! {
    /// SigDedup == a reference sliding window of the last 65,536 distinct keys.
    #[test]
    fn dedup_matches_reference_window(keys in prop::collection::vec(0u64..70_000, 1..3000)) {
        // spread small ints over the hash space but keep collisions in the low bits
        let h = |k: u64| xxhash_rust::xxh3::xxh3_64(&k.to_le_bytes()) & !0xFFFF | (k & 0xFFFF);
        let mut d = SigDedup::new();
        let mut set = HashSet::new();
        let mut order = VecDeque::new();
        for &k in &keys {
            let key = h(k);
            let was = set.contains(&key);
            let got = d.insert(key, FeedSrc::LogsWs, 0);
            prop_assert_eq!(was, got != Seen::New);
            if !was {
                set.insert(key);
                order.push_back(key);
                if order.len() > dedup::DEDUP_RING {
                    let old = order.pop_front().unwrap();
                    set.remove(&old);
                }
            }
        }
        for &k in &keys {
            prop_assert_eq!(d.contains(h(k)), set.contains(&h(k)));
        }
    }
}

#[test]
fn collector_relay_over_a_unix_socket_reconnects() {
    use rugsnipe_feed::relay::{CollectorRelay, RelayConfig};
    use std::io::Write;
    use std::os::unix::net::UnixListener;
    let dir = std::env::temp_dir().join(format!("rugsnipe-relay-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("pp.sock");
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    let lines = std::fs::read_to_string(format!(
        "{}/fixtures/relay_pp.txt",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let server = std::thread::spawn(move || {
        // two client sessions: the relay must reconnect after the first closes
        for _ in 0..2 {
            let (mut s, _) = listener.accept().unwrap();
            s.write_all(lines.as_bytes()).unwrap();
            drop(s);
        }
    });
    let mut cfg = RelayConfig::new(&path);
    cfg.max_backoff_ms = 100;
    let mut feed = CollectorRelay::spawn(cfg).unwrap();
    let mut buf = FeedBuf::with_capacity(64, 0);
    let mut hits = 0;
    let t0 = std::time::Instant::now();
    while hits < 6 && t0.elapsed().as_secs() < 10 {
        buf.clear();
        feed.poll(&mut buf);
        hits += buf
            .events()
            .iter()
            .filter(|e| e.src == FeedSrc::Relay && matches!(e.payload, Payload::Hit(_)))
            .count();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    server.join().unwrap();
    let h = feed.health().unwrap().snapshot();
    feed.shutdown();
    assert_eq!(hits, 6, "3 creates per session x 2 sessions");
    assert!(h.reconnects >= 1);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn collector_relay_keeps_a_partial_line_across_read_timeouts() {
    use rugsnipe_feed::relay::{CollectorRelay, RelayConfig};
    use std::io::Write;
    use std::os::unix::net::UnixListener;
    let dir = std::env::temp_dir().join(format!("rugsnipe-relay-split-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("pp.sock");
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    let lines = std::fs::read_to_string(format!(
        "{}/fixtures/relay_pp.txt",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let server = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        // every line arrives in two halves with a pause longer than the reader's 500 ms
        // read timeout in between: the first half must not be dropped
        for line in lines.lines() {
            let bytes = line.as_bytes();
            let mid = bytes.len() / 2;
            s.write_all(&bytes[..mid]).unwrap();
            s.flush().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(700));
            s.write_all(&bytes[mid..]).unwrap();
            s.write_all(b"\n").unwrap();
            s.flush().unwrap();
        }
        // keep the socket open briefly so the last line is consumed before EOF
        std::thread::sleep(std::time::Duration::from_millis(300));
        drop(s);
    });
    let mut cfg = RelayConfig::new(&path);
    cfg.max_backoff_ms = 100;
    let mut feed = CollectorRelay::spawn(cfg).unwrap();
    let mut buf = FeedBuf::with_capacity(64, 0);
    let mut hits = 0;
    let t0 = std::time::Instant::now();
    while hits < 3 && t0.elapsed().as_secs() < 15 {
        buf.clear();
        feed.poll(&mut buf);
        hits += buf
            .events()
            .iter()
            .filter(|e| e.src == FeedSrc::Relay && matches!(e.payload, Payload::Hit(_)))
            .count();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    server.join().unwrap();
    feed.shutdown();
    assert_eq!(hits, 3, "all 3 creates decoded although every line was split across a timeout");
    let _ = std::fs::remove_dir_all(dir);
}
