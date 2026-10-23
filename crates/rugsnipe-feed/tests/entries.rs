//! The shredstream proxy's entry stream as the second pre-execution feed
//! (`feeds.entries`, source `shredstream`): a create in an entry reaches the
//! hot ring as a wire frame before execution; pump transactions beyond the
//! run complete it.

use rugsnipe_feed::buf::FeedBuf;
use rugsnipe_feed::event::Payload;
use rugsnipe_feed::grpc::mock::{fixture_entries, MockServer};
use rugsnipe_feed::grpc::{for_each_entry_tx, Grpc, GrpcConfig, ShredEntry};
use rugsnipe_feed::wiremeta::WireMeta;
use rugsnipe_feed::{Feed, FeedSrc};
use serde_json::Value;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

fn fixture(name: &str) -> Value {
    let p = format!(
        "{}/../pump-codec/fixtures/tx/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap()
}

fn wait(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(
            t0.elapsed() < Duration::from_secs(secs),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn the_entry_walker_yields_each_transaction_exactly() {
    let a = fixture("legacy_create_spl");
    let b = fixture("legacy_buy18");
    let bytes = fixture_entries(&[&a, &b]).unwrap();
    let mut seen: Vec<Vec<u8>> = Vec::new();
    let n = for_each_entry_tx(&bytes, |raw| seen.push(raw.to_vec())).unwrap();
    assert_eq!(n, 2);
    let raw = |v: &Value| {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD
            .decode(v["result"]["transaction"][0].as_str().unwrap())
            .unwrap()
    };
    assert_eq!(seen[0], raw(&a));
    assert_eq!(seen[1], raw(&b));
    // v0 transactions with lookup tables (the bulk of mainnet) and a v1
    // envelope are sized correctly without any loaded addresses
    let c = fixture("fresh_v0alt_inner_sell_v2");
    let d = fixture("fresh_v1_inner_sell_v3");
    let e = fixture("fresh_v0alt_create_v2_buy_v3");
    let bytes2 = fixture_entries(&[&c, &a, &d, &e, &b]).unwrap();
    let mut seen2: Vec<Vec<u8>> = Vec::new();
    let n2 = for_each_entry_tx(&bytes2, |raw| seen2.push(raw.to_vec())).unwrap();
    assert_eq!(n2, 5);
    assert_eq!(seen2[0], raw(&c));
    assert_eq!(seen2[2], raw(&d));
    assert_eq!(seen2[3], raw(&e));
    assert_eq!(seen2[4], raw(&b));
    // a truncated buffer delivers what it can and reports the error
    let mut seen2 = 0;
    let r = for_each_entry_tx(&bytes[..bytes.len() - 10], |_| seen2 += 1);
    assert!(r.is_err());
    assert_eq!(seen2, 1);
}

#[test]
fn a_create_in_an_entry_reaches_the_hot_ring_before_execution() {
    let srv = MockServer::start(None);
    let mut g = Grpc::spawn(GrpcConfig {
        src: FeedSrc::ShredStream,
        url: srv.url(),
        x_token: None,
        stale_ms: 5_000,
        bundle_run: true,
        entries: true,
        ..Default::default()
    })
    .unwrap();
    let h = g.handle();
    wait("the entries subscription", 10, || {
        srv.shredstream.entries_connected()
    });
    assert!(
        !srv.geyser.connected(),
        "entries mode never opens the post-execution subscription"
    );

    let slot = 460_500_000u64;
    // a legacy create with its dev buy (no lookup tables: all keys static)
    let create = fixture("legacy_create_spl");
    assert!(srv.shredstream.push_entry(ShredEntry {
        slot,
        entries: fixture_entries(&[&create]).unwrap(),
    }));
    let mut buf = FeedBuf::with_capacity(256, 1 << 20);
    let mut index = None;
    let mut got = false;
    wait("the create's wire event", 10, || {
        buf.clear();
        g.poll(&mut buf);
        for e in buf.events() {
            assert_eq!(e.src, FeedSrc::ShredStream);
            assert_eq!(e.slot, slot);
            if let Payload::Wire(w) = e.payload {
                let m = WireMeta::parse(buf.wire_meta(w)).expect("trailer");
                assert!(!m.failed(), "pre-execution: never failed");
                assert!(!m.has_inner(), "pre-execution: no inner instructions");
                index = m.tx_index;
                got = true;
            }
        }
        got
    });
    assert_eq!(index, Some(0), "the first pump transaction of the slot");
    assert_eq!(h.health().creates.load(Relaxed), 1);
    assert_eq!(h.stats().wire_events.load(Relaxed), 1);
    assert_eq!(h.stats().slot_updates.load(Relaxed), 1, "one entry message");

    // six more pump transactions of the slot in one entry: the sixth is
    // beyond the run (create + 5) and completes it -> RunDone for the mint
    // (lookup-table and v1 transactions among them, as on mainnet: handled
    // without loaded addresses, never breaking the walk)
    let others = [
        "legacy_buy18",
        "fresh_v0alt_inner_sell_v2",
        "legacy_buy19",
        "fresh_v1_inner_sell_v3",
        "legacy_buy_exact_sol_in",
        "fresh_v0alt_create_v2_buy_v3",
        "legacy_sell16",
        "worked_buy",
        "worked_sell",
    ];
    let fx: Vec<Value> = others.iter().map(|n| fixture(n)).collect();
    let refs: Vec<&Value> = fx.iter().collect();
    assert!(srv.shredstream.push_entry(ShredEntry {
        slot,
        entries: fixture_entries(&refs).unwrap(),
    }));
    let mut done = None;
    wait("the run-done signal", 10, || {
        buf.clear();
        g.poll(&mut buf);
        for e in buf.events() {
            if let Payload::RunDone(r) = e.payload {
                done = Some(r);
            }
        }
        done.is_some()
    });
    let done = done.unwrap();
    assert_eq!(done.slot, slot);
    assert!(done.index > 5, "{done:?}");
    // 10 pump transactions, one of which reaches the pump program through a
    // lookup table only (a router CPI): invisible here without loaded
    // addresses, carried by the other feeds
    assert_eq!(h.stats().transactions.load(Relaxed), 9, "pump transactions");

    // a dropped stream reconnects (the session ends, the proxy is re-subscribed)
    let before = srv.shredstream.entries_connections();
    srv.shredstream.drop_stream();
    wait("the resubscription", 10, || {
        srv.shredstream.entries_connections() > before
    });
    drop(g);
}
