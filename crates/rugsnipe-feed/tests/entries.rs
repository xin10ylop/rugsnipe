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

fn raw_of(v: &Value) -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(v["result"]["transaction"][0].as_str().unwrap())
        .unwrap()
}

/// One entry batch (`Vec<Entry>`, one entry) of these wire transactions.
fn batch(txs: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&1u64.to_le_bytes());
    out.extend_from_slice(&7u64.to_le_bytes());
    out.extend_from_slice(&[0u8; 32]);
    out.extend_from_slice(&(txs.len() as u64).to_le_bytes());
    for t in txs {
        out.extend_from_slice(t);
    }
    out
}

/// The mint of a create transaction, as the adapter decodes it.
fn mint_of(raw: &[u8]) -> pump_codec::Pubkey {
    match rugsnipe_feed::grpc::classify_create(raw, &[]) {
        rugsnipe_feed::grpc::CreateCheck::Hit(m, _) => m,
        other => panic!("not a create hit without loaded addresses: {other:?}"),
    }
}

/// A top-level pump buy rewritten to buy `mint` (its static mint key
/// replaced; signatures are not checked before execution).
fn buy_of(fixture_name: &str, mint: pump_codec::Pubkey) -> Vec<u8> {
    let mut raw = raw_of(&fixture(fixture_name));
    let view = pump_codec::tx::TxView::parse(&raw).unwrap();
    let buys = pump_codec::create_hit::pump_buys(&view, None, &[]).unwrap();
    let old = buys[0].mint.expect("a top-level buy with a static mint");
    let at = raw
        .windows(32)
        .position(|w| w == old.as_ref())
        .expect("the mint key");
    raw[at..at + 32].copy_from_slice(mint.as_ref());
    raw
}

#[test]
fn mixed_v0_v1_creates_and_buys_in_entries_reach_the_hot_ring() {
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
    // v0 without lookups, v0 with the create's constant accounts (token
    // program, global, event authority...) in a lookup table, v1
    let c_v0 = raw_of(&fixture("v0_create_v2_buy_v3"));
    let c_alt = raw_of(&fixture("fresh_v0alt_create_v2_buy_v3"));
    let c_v1 = raw_of(&fixture("v1_create_v2_buy_b"));
    let (m_v0, m_alt, m_v1) = (mint_of(&c_v0), mint_of(&c_alt), mint_of(&c_v1));
    // buys of those mints in the same slot: a legacy and a v1 transaction
    let b_legacy = buy_of("legacy_buy18", m_v1);
    let b_v1 = buy_of("v1_buy_exact_quote_in_v3", m_v0);
    let b_alt = buy_of("legacy_buy_exact_sol_in", m_alt);
    let other = raw_of(&fixture("fresh_v0alt_inner_sell_v2"));
    let slot = 460_576_752u64;
    // message 1: the creates; then the next slot starts (interleaved, as a
    // shred feed delivers); message 3: two completed data sets of slot `slot`
    // in one message (an Alpenglow marker between them): the buys
    let mut m3 = batch(&[&other, &b_legacy]);
    m3.extend_from_slice(&0u64.to_le_bytes());
    m3.extend_from_slice(&[1, 0, 1, 41, 0, 1]);
    m3.extend_from_slice(&(slot - 1).to_le_bytes());
    m3.extend_from_slice(&[0xab; 32]);
    m3.extend_from_slice(&batch(&[&b_v1, &b_alt]));
    let msgs = [
        (slot, batch(&[&c_v0, &c_alt, &c_v1])),
        (slot + 1, batch(&[&raw_of(&fixture("legacy_sell16"))])),
        (slot, m3),
    ];
    for (s, entries) in msgs {
        let before = h.stats().slot_updates.load(Relaxed);
        assert!(srv.shredstream.push_entry(ShredEntry { slot: s, entries }));
        wait("the entry message", 10, || {
            h.stats().slot_updates.load(Relaxed) > before
        });
    }
    let mut buf = FeedBuf::with_capacity(256, 1 << 20);
    let (mut wires, mut trades) = (Vec::new(), Vec::new());
    wait("three creates and three buys", 10, || {
        buf.clear();
        g.poll(&mut buf);
        for e in buf.events() {
            match e.payload {
                Payload::Wire(w) => {
                    wires.push(WireMeta::parse(buf.wire_meta(w)).unwrap().tx_index)
                }
                Payload::Trade(t) => trades.push(t),
                _ => {}
            }
        }
        wires.len() >= 3 && trades.len() >= 3
    });
    assert_eq!(h.stats().convert_errors.load(Relaxed), 0, "every message walked");
    assert_eq!(h.health().decode_miss.load(Relaxed), 0, "no create left unresolved");
    assert_eq!(h.health().creates.load(Relaxed), 3);
    assert_eq!(h.stats().wire_events.load(Relaxed), 3, "creates->wire");
    assert_eq!(h.stats().trade_events.load(Relaxed), 3, "trades");
    // the router sell reaches pump through a lookup table only: not seen
    assert_eq!(h.stats().transactions.load(Relaxed), 7, "pump_tx");
    assert_eq!(wires, [Some(0), Some(1), Some(2)]);
    let by_mint = |m| trades.iter().find(|t| t.mint == m).copied().unwrap();
    // the slot's arrival index continues across the interleaved slot
    // (creates 0..2, then the buys; the next slot counts its own)
    assert_eq!(by_mint(m_v1).index, 3);
    assert_eq!(by_mint(m_v0).index, 4);
    assert_eq!(by_mint(m_alt).index, 5);
    for t in &trades {
        assert_eq!(t.slot, slot);
        assert!(t.is_buy && t.sol_amount > 0 && t.token_amount > 0, "{t:?}");
    }
    drop(g);
}
