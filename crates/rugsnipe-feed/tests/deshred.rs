//! The pre-execution feed (`SubscribeDeshred`) against the mock server: the
//! subscription shape, a recorded create reaching the hot ring as a wire
//! frame with an arrival-order index and no execution meta, the bundle run
//! completing after five more pump transactions, and pings answered.

use pump_codec::events::InnerInstruction;
use pump_codec::tx::TxView;
use pump_codec::Pubkey;
use rugsnipe_feed::grpc::mock::{deshred_slot_update, fixture_deshred_update, MockServer};
use rugsnipe_feed::grpc::{Grpc, GrpcConfig, F_DESHRED, F_SLOTS};
use rugsnipe_feed::wiremeta::{WireMeta, EMPTY_INNER, MAX_INNER, MAX_LOADED};
use rugsnipe_feed::{Feed, FeedBuf, FeedSrc, Payload};
use serde_json::Value;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};
use yellowstone_grpc_proto::prelude::subscribe_update_deshred::UpdateOneof;
use yellowstone_grpc_proto::prelude::{SubscribeUpdateDeshred, SubscribeUpdatePing};

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

fn hot_decode(tx: &[u8], meta: &[u8]) -> Option<pump_codec::CreateHit> {
    let view = TxView::parse(tx).expect("wire parses");
    let m = WireMeta::parse(meta);
    let mut loaded = [Pubkey::default(); MAX_LOADED];
    let mut inner: [InnerInstruction<'_>; MAX_INNER] = [EMPTY_INNER; MAX_INNER];
    let (nl, ni) = match &m {
        Some(m) => (m.loaded_into(&mut loaded), m.inner_into(&mut inner)),
        None => (0, 0),
    };
    pump_codec::create_hit(
        &view,
        (nl > 0).then_some(&loaded[..nl]),
        &inner[..ni],
        pump_codec::curve::FeeBps::DEFAULT,
    )
    .expect("decodes")
}

#[test]
fn a_create_from_the_deshred_stream_reaches_the_hot_ring_before_execution() {
    let srv = MockServer::start(Some("tok-deshred"));
    let mut g = Grpc::spawn(GrpcConfig {
        src: FeedSrc::Shreds,
        url: srv.url(),
        x_token: Some("tok-deshred".into()),
        stale_ms: 5_000,
        bundle_run: true,
        deshred: true,
        ..Default::default()
    })
    .unwrap();
    let h = g.handle();
    wait("deshred subscription", 10, || {
        srv.geyser.deshred_connected() && !srv.geyser.deshred_requests().is_empty()
    });
    assert!(
        !srv.geyser.connected(),
        "deshred mode never opens the post-execution subscription"
    );
    let req = &srv.geyser.deshred_requests()[0];
    assert!(req.deshred_transactions.contains_key(F_DESHRED));
    assert!(req.slots.contains_key(F_SLOTS));
    assert!(req.ping.is_none());

    let slot = 460_400_000u64;
    assert!(srv.geyser.push_deshred(deshred_slot_update(slot - 1)));
    // a v0 create with lookup tables: the loaded addresses ride the trailer
    let create = fixture("fresh_v0alt_create_v2_buy_v3");
    assert!(srv
        .geyser
        .push_deshred(fixture_deshred_update(&create, slot).unwrap()));
    let mut buf = FeedBuf::with_capacity(256, 1 << 20);
    let mut hit = None;
    let mut index = None;
    wait("the create's wire event", 10, || {
        buf.clear();
        g.poll(&mut buf);
        for e in buf.events() {
            assert_eq!(e.src, FeedSrc::Shreds);
            if let Payload::Wire(w) = e.payload {
                let m = WireMeta::parse(buf.wire_meta(w)).expect("trailer");
                assert!(!m.failed(), "pre-execution: never failed");
                assert!(!m.has_inner(), "pre-execution: no inner instructions");
                index = m.tx_index;
                hit = hot_decode(buf.wire_tx(w), buf.wire_meta(w));
            }
        }
        hit.is_some()
    });
    let hit = hit.unwrap();
    assert_eq!(index, Some(0), "the first transaction of the slot");
    assert!(
        hit.intx_sol_in > 0,
        "the dev buy is read from the arguments"
    );
    assert_eq!(h.health().creates.load(Relaxed), 1);
    assert_eq!(h.stats().wire_events.load(Relaxed), 1);

    // five more pump transactions of the slot complete the run (create + 5):
    // the sixth is beyond it and closes the run -> RunDone for the mint
    let others = [
        "legacy_buy18",
        "legacy_buy19",
        "legacy_buy_exact_sol_in",
        "fresh_v0_sell16",
        "fresh_v0alt_inner_sell_v2",
        "fresh_v1_inner_sell_v3",
    ];
    for n in others {
        assert!(srv
            .geyser
            .push_deshred(fixture_deshred_update(&fixture(n), slot).unwrap()));
    }
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
    assert_eq!(done.mint, hit.mint);
    assert_eq!(done.slot, slot);
    assert!(done.index > 5, "{done:?}");
    assert_eq!(h.stats().transactions.load(Relaxed), 7);

    // a server ping is answered on the deshred stream
    let before = srv.geyser.deshred_requests().len();
    assert!(srv.geyser.push_deshred(SubscribeUpdateDeshred {
        filters: vec![],
        update_oneof: Some(UpdateOneof::Ping(SubscribeUpdatePing {})),
        created_at: None,
    }));
    wait("the ping reply", 10, || {
        srv.geyser
            .deshred_requests()
            .iter()
            .skip(before)
            .any(|r| r.ping.is_some())
    });
    g.shutdown();
}
