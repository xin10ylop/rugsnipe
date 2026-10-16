//! The gRPC post-execution feed inside the real `rugsnipe-hot` binary
//! (paper mode, enabled explicitly in the config; feature `grpc`, on by
//! default) against a local Yellowstone mock server, next to the logs path
//! (collector relay frames of the same transactions):
//!
//! * the creates arrive as wire events with their meta trailer and are
//!   decoded on the hot thread (v0 with a lookup table, v1);
//! * each launch is decided once although two feeds deliver it (the second
//!   copy is `already_fired`);
//! * the first-seen benchmark pairs the copies per signature and reports
//!   which feed was first and the other's lag (`cold.first_seen`);
//! * the creation slot's bundle run is assembled from the stream
//!   (`cold.feed_bundle`, `feed_bundle` rows for the decided launches).
//!
//! Loopback only (no external network).

mod common;

use common::*;
use rugsnipe_feed::grpc::mock::{fixture_update, slot_update, MockServer};
use std::sync::atomic::{AtomicBool, Ordering::*};
use std::sync::Arc;
use std::time::{Duration, Instant};

const CREATES: [&str; 3] = [
    "fresh_v0alt_create_v2_buy_v3",
    "v1_create_v2_buy",
    "v0_create_v2_buy_v3",
];

#[test]
fn paper_binary_runs_the_grpc_feed_and_compares_first_seen() {
    let dir = tmpdir("grpc-feed");
    let srv = MockServer::start(None);
    let slot = 456_000_000u64;
    let fx: Vec<serde_json::Value> = CREATES.iter().map(|n| codec_fixture(n)).collect();
    // the creates' signers, decoded as the hot thread will
    let mut users = vec![];
    for f in &fx {
        let (tx, meta) = fixture_wire(f, 0);
        let view = pump_codec::tx::TxView::parse(&tx).unwrap();
        let m = rugsnipe_feed::wiremeta::WireMeta::parse(&meta).unwrap();
        let mut loaded = vec![pump_codec::Pubkey::default(); 256];
        let n = m.loaded_into(&mut loaded);
        let h = pump_codec::create_hit(
            &view,
            (n > 0).then_some(&loaded[..n]),
            &[],
            pump_codec::curve::FeeBps::DEFAULT,
        )
        .unwrap()
        .unwrap();
        users.push(h.user);
    }
    std::fs::write(
        dir.join("armset.json"),
        serde_json::to_vec(&armset(&users, &[])).unwrap(),
    )
    .unwrap();
    // the logs path's copies through the relay
    let lines: String = fx
        .iter()
        .map(|f| format!("1\t{}\n", fixture_logs_frame(f, slot)))
        .collect();
    let stop = Arc::new(AtomicBool::new(false));
    let sock = dir.join("pp.sock");
    let relay = serve_relay(&sock, lines, stop.clone());
    let cfg = paper_toml(&dir, &sock, [9; 6]).replace(
        "pumpportal = { enabled = false }",
        &format!(
            "pumpportal = {{ enabled = false }}\ngrpc = {{ enabled = true, url = \"{}\", x_token = \"\", stale_ms = 2000, bundle_run = true }}",
            srv.url()
        ),
    );
    assert!(cfg.contains("grpc = {"));
    std::fs::write(dir.join("paper.toml"), cfg).unwrap();
    let child = hot_cmd(
        &dir.join("paper.toml"),
        &dir.join("armset.json"),
        &dir.join("state.db"),
        5,
    )
    .arg("--summary")
    .arg(dir.join("summary.json"))
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::piped())
    .spawn()
    .unwrap();
    // the stream: the three creates in one slot, then slots
    let t0 = Instant::now();
    while !srv.geyser.connected() {
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "the binary never subscribed"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(300));
    for (i, f) in fx.iter().enumerate() {
        assert!(srv
            .geyser
            .push(fixture_update(f, slot, 10 * i as u64, &["creates", "pump"]).unwrap()));
    }
    let pusher = {
        let g = srv.geyser.clone();
        let stop = stop.clone();
        std::thread::spawn(move || {
            let mut s = slot + 1;
            while !stop.load(Acquire) {
                g.push(slot_update(s));
                s += 1;
                std::thread::sleep(Duration::from_millis(100));
            }
        })
    };
    let out = child.wait_with_output().unwrap();
    stop.store(true, Release);
    let _ = relay.join();
    let _ = pusher.join();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    let s: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("summary.json")).unwrap()).unwrap();
    // the subscription the binary sent
    let req = &srv.geyser.requests()[0];
    assert!(req.transactions.contains_key("creates") && req.transactions.contains_key("pump"));
    // one decision per launch although two feeds delivered it
    assert_eq!(s["counters"]["intents"], 3, "{}", s["decisions"]);
    assert_eq!(decision(&s, "already_fired"), 3, "{}", s["decisions"]);
    // the gRPC copies were wire events decoded on the hot thread
    let g = s["feeds"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["src"] == "grpc")
        .expect("the grpc feed's health")
        .clone();
    assert_eq!(g["creates"], 3, "{g}");
    assert_eq!(s["cold"]["grpc"]["wire_events"], 3, "{}", s["cold"]["grpc"]);
    // the first-seen benchmark paired every create
    let fs = &s["cold"]["first_seen"];
    assert_eq!(fs["paired"], 3, "{fs}");
    assert_eq!(fs["seen_by"]["grpc"], 3);
    assert_eq!(fs["seen_by"]["relay"], 3);
    let lags = fs["lag"].as_object().unwrap();
    assert!(
        lags.contains_key("grpc_behind_relay") || lags.contains_key("relay_behind_grpc"),
        "{fs}"
    );
    // the bundle runs of the creation slot (closed by the next slot)
    let fb = &s["cold"]["feed_bundle"];
    assert_eq!(fb["runs"], 3, "{fb}");
    assert_eq!(fb["with_intent"], 3, "{fb}");
    let c = rusqlite::Connection::open(dir.join("state.db")).unwrap();
    assert_eq!(
        count(
            &c,
            "SELECT COUNT(*) FROM feed_bundle WHERE intent_id IS NOT NULL"
        ),
        3
    );
    assert_eq!(
        count(
            &c,
            "SELECT COUNT(*) FROM feed_bundle WHERE features IS NOT NULL"
        ),
        3,
        "the dev buy of each create is a bundle"
    );
    // paper with the gRPC feed stays paper: nothing real was built
    assert_eq!(s["mode"], "paper");
    drop(c);
    let _ = std::fs::remove_dir_all(&dir);
}
