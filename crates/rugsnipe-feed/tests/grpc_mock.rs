//! The Yellowstone gRPC adapter against a local mock server (tonic, loopback
//! h2c) replaying recorded mainnet transactions (pump-codec fixtures):
//!
//! * every fixture re-serializes byte for byte (protobuf -> wire -> protobuf);
//! * the meta trailer resolves v0 lookups and launcher CPIs for `create_hit`;
//! * end to end: x-token, the subscription filters, creates as wire events
//!   that decode as the hot thread decodes them, our wallet's trade and a
//!   held mint's trade as Trade events, watch / unwatch resubscribes, server
//!   pings answered, the bundle run of the creation slot, stale detection and
//!   reconnects (the filters re-sent with the current watches).
//!
//! No network beyond 127.0.0.1.

use pump_codec::events::InnerInstruction;
use pump_codec::tx::TxView;
use pump_codec::Pubkey;
use rugsnipe_feed::bundle::CloseReason;
use rugsnipe_feed::grpc::convert::{tx_to_wire, wire_to_tx};
use rugsnipe_feed::grpc::mock::{fixture_update, ping_update, slot_update, MockServer};
use rugsnipe_feed::grpc::{Grpc, GrpcConfig, F_CREATES, F_MINTS, F_PUMP, F_WALLET};
use rugsnipe_feed::health::MissKind;
use rugsnipe_feed::wiremeta::{self, WireMeta, EMPTY_INNER, MAX_INNER, MAX_LOADED};
use rugsnipe_feed::{Feed, FeedBuf, Payload};
use serde_json::Value;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};
use yellowstone_grpc_proto::prelude::subscribe_update::UpdateOneof;
use yellowstone_grpc_proto::prelude::{
    CompiledInstruction, Message, MessageHeader, SubscribeUpdate, SubscribeUpdateTransaction,
    SubscribeUpdateTransactionInfo, Transaction, TransactionStatusMeta,
};

fn fixture(name: &str) -> Value {
    let p = format!(
        "{}/../pump-codec/fixtures/tx/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap()
}

fn all_fixtures() -> Vec<(String, Value)> {
    let dir = format!("{}/../pump-codec/fixtures/tx", env!("CARGO_MANIFEST_DIR"));
    let mut v: Vec<(String, Value)> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .map(|e| {
            let n = e.path().file_stem().unwrap().to_string_lossy().to_string();
            let j = serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap();
            (n, j)
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

fn raw_of(f: &Value) -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(f["result"]["transaction"][0].as_str().unwrap())
        .unwrap()
}

/// What the hot loop does with a wire event: parse the transaction, read the
/// trailer, run `create_hit` with the loaded addresses and inner instructions.
fn hot_decode(
    tx: &[u8],
    meta: &[u8],
) -> Result<Option<pump_codec::CreateHit>, pump_codec::HitError> {
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
}

#[test]
fn every_fixture_roundtrips_wire_protobuf_wire() {
    let fx = all_fixtures();
    assert!(fx.len() >= 40, "{} fixtures", fx.len());
    let mut versions = std::collections::BTreeSet::new();
    for (name, f) in &fx {
        let raw = raw_of(f);
        let tx = wire_to_tx(&raw).unwrap_or_else(|e| panic!("{name}: {e}"));
        let back = tx_to_wire(&tx).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(back, raw, "{name}: re-serialized bytes differ");
        versions.insert(format!("{:?}", TxView::parse(&raw).unwrap().version()));
    }
    assert_eq!(versions.len(), 3, "legacy, v0 and v1 covered: {versions:?}");
}

#[test]
fn the_trailer_resolves_lookups_and_launcher_cpis() {
    // a launcher CPI create: no top-level pump instruction
    let f = fixture("fresh_v1_cpi_create_v2_bexq_v2");
    let raw = raw_of(&f);
    let up = fixture_update(&f, 1, 7, &[F_CREATES]).unwrap();
    let Some(yellowstone_grpc_proto::prelude::subscribe_update::UpdateOneof::Transaction(t)) =
        up.update_oneof
    else {
        panic!()
    };
    let info = t.transaction.unwrap();
    let mut meta = vec![];
    wiremeta::encode(
        &rugsnipe_feed::grpc::convert::meta_in(info.meta.as_ref(), Some(7)),
        &mut meta,
    );
    assert!(
        matches!(
            hot_decode(&raw, &[]),
            Err(pump_codec::HitError::CpiCreateNeedsMeta { .. })
        ),
        "without inner instructions a CPI create is unresolvable"
    );
    let hit = hot_decode(&raw, &meta)
        .unwrap()
        .expect("create found through the CPI");
    assert_ne!(hit.mint, Pubkey::default());
    assert_eq!(WireMeta::parse(&meta).unwrap().tx_index, Some(7));
    // a v0 create with a lookup table: the trailer's loaded addresses resolve it
    for name in [
        "fresh_v0alt_create_v2_buy_v3",
        "fresh_v0alt_create_v2_inner_buy",
        "v1_create_v2_buy",
    ] {
        let f = fixture(name);
        let raw = raw_of(&f);
        let up = fixture_update(&f, 1, 3, &[F_CREATES]).unwrap();
        let Some(yellowstone_grpc_proto::prelude::subscribe_update::UpdateOneof::Transaction(t)) =
            up.update_oneof
        else {
            panic!()
        };
        let mut meta = vec![];
        wiremeta::encode(
            &rugsnipe_feed::grpc::convert::meta_in(t.transaction.unwrap().meta.as_ref(), Some(3)),
            &mut meta,
        );
        let h = hot_decode(&raw, &meta)
            .unwrap()
            .unwrap_or_else(|| panic!("{name}"));
        assert!(h.is_sol_quote(), "{name}: quote resolved");
        assert!(
            h.intx_exact,
            "{name}: in-tx buys exact from the TradeEvents"
        );
        assert!(h.intx_buys >= 1, "{name}");
    }
}

/// The trailer the adapter builds for a fixture.
fn trailer(f: &Value) -> Vec<u8> {
    let up = fixture_update(f, 1, 0, &[F_CREATES]).unwrap();
    let Some(yellowstone_grpc_proto::prelude::subscribe_update::UpdateOneof::Transaction(t)) =
        up.update_oneof
    else {
        panic!()
    };
    let mut meta = vec![];
    wiremeta::encode(
        &rugsnipe_feed::grpc::convert::meta_in(t.transaction.unwrap().meta.as_ref(), Some(0)),
        &mut meta,
    );
    meta
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

/// The first TradeEvent user of a fixture (from its logs).
fn first_trade(f: &Value) -> (Pubkey, Pubkey) {
    let logs: Vec<String> = f["result"]["meta"]["logMessages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    let mut trades = vec![];
    rugsnipe_feed::decode::decode_logs(0, Default::default(), false, &logs, &mut trades);
    (trades[0].user, trades[0].mint)
}

/// A `creates`-filter update of a hand-made legacy transaction whose pump
/// CreateV2 arguments are cut short (a borsh string of length 5 with one
/// byte present): the decoder rejects it with `UnexpectedEof`.
fn malformed_create_update(slot: u64, index: u64) -> SubscribeUpdate {
    use pump_codec::constants::{ix, MINT_AUTHORITY, PUMP_PROGRAM_ID};
    let keys = [
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        MINT_AUTHORITY,
        PUMP_PROGRAM_ID,
    ];
    let mut data = ix::CREATE_V2.to_vec();
    data.extend_from_slice(&[5, 0, 0, 0, b'a']);
    let tx = Transaction {
        signatures: vec![vec![7; 64]],
        message: Some(Message {
            header: Some(MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 2,
            }),
            account_keys: keys.iter().map(|k| k.to_bytes().to_vec()).collect(),
            recent_blockhash: vec![3; 32],
            instructions: vec![CompiledInstruction {
                program_id_index: 3,
                accounts: vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                data,
            }],
            ..Default::default()
        }),
    };
    SubscribeUpdate {
        filters: vec![F_CREATES.to_string()],
        update_oneof: Some(UpdateOneof::Transaction(SubscribeUpdateTransaction {
            transaction: Some(SubscribeUpdateTransactionInfo {
                signature: vec![7; 64],
                is_vote: false,
                transaction: Some(tx),
                meta: Some(TransactionStatusMeta::default()),
                index,
            }),
            slot,
            bank_id: 0,
        })),
        created_at: None,
    }
}

#[test]
fn malformed_create_is_a_counted_miss_end_to_end() {
    let srv = MockServer::start(None);
    let mut g = Grpc::spawn(GrpcConfig {
        url: srv.url(),
        stale_ms: 2_000,
        ..Default::default()
    })
    .unwrap();
    let h = g.handle();
    wait("connection", 10, || {
        srv.geyser.connected() && !srv.geyser.requests().is_empty()
    });
    let slot = 456_000_100u64;
    let good = fixture("v0_create_v2_buy_v3");
    assert!(srv
        .geyser
        .push(fixture_update(&good, slot, 1, &[F_CREATES]).unwrap()));
    assert!(srv.geyser.push(malformed_create_update(slot, 2)));
    wait("the miss is counted", 10, || {
        h.health().decode_miss.load(Relaxed) == 1
    });
    assert_eq!(
        h.health().decode_miss_kind[MissKind::Eof as usize].load(Relaxed),
        1
    );
    wait("the good create is counted", 10, || {
        h.health().creates.load(Relaxed) == 1
    });
    // both reach the hot ring; the hot side rejects the same bytes the same way
    let mut buf = FeedBuf::with_capacity(256, 1 << 20);
    let mut seen = vec![];
    wait("two wire events", 10, || {
        buf.clear();
        g.poll(&mut buf);
        for e in buf.events() {
            if let Payload::Wire(w) = e.payload {
                seen.push(hot_decode(buf.wire_tx(w), buf.wire_meta(w)).map(|h| h.map(|h| h.mint)));
            }
        }
        seen.len() == 2
    });
    assert!(matches!(seen[0], Ok(Some(_))), "{:?}", seen[0]);
    assert!(
        matches!(
            seen[1],
            Err(pump_codec::HitError::Decode(
                pump_codec::DecodeError::UnexpectedEof {
                    offset: 12,
                    needed: 4
                }
            ))
        ),
        "{:?}",
        seen[1]
    );
    assert_eq!(h.health().creates.load(Relaxed), 1);
    assert_eq!(h.health().decode_miss.load(Relaxed), 1);
}

#[test]
fn adapter_end_to_end_against_the_mock_server() {
    let srv = MockServer::start(Some("tok-123"));
    let creates = [
        "fresh_v0alt_create_v2_buy_v3",
        "v1_create_v2_buy",
        "fresh_v1_cpi_create_v2_bexq_v2",
    ];
    let wallet_fx = fixture("legacy_buy18");
    let (wallet, _) = first_trade(&wallet_fx);
    let held_fx = fixture("fresh_v0_sell16");
    let (_, held_mint) = first_trade(&held_fx);
    let mut g = Grpc::spawn(GrpcConfig {
        url: srv.url(),
        x_token: Some("tok-123".into()),
        stale_ms: 400,
        min_backoff_ms: 20,
        max_backoff_ms: 100,
        wallet: Some(wallet),
        bundle_run: true,
        ..Default::default()
    })
    .unwrap();
    let h = g.handle();
    let bundles = g.take_bundles().expect("bundle runs enabled");
    wait("connection", 10, || {
        srv.geyser.connected() && !srv.geyser.requests().is_empty()
    });
    let first = srv.geyser.requests()[0].clone();
    assert_eq!(first.commitment, Some(0), "processed");
    for f in [F_CREATES, F_WALLET, F_PUMP] {
        assert!(first.transactions.contains_key(f), "filter {f}");
    }
    assert!(
        !first.transactions.contains_key(F_MINTS),
        "nothing held yet"
    );
    // the creation slot: three creates, our own trade, then the next slot
    let slot = 456_000_000u64;
    let (mut mints, mut bare) = (vec![], vec![]);
    for (i, n) in creates.iter().enumerate() {
        let f = fixture(n);
        assert!(srv
            .geyser
            .push(fixture_update(&f, slot, 10 * i as u64, &[F_CREATES, F_PUMP]).unwrap()));
        mints.push(hot_decode(&raw_of(&f), &trailer(&f)).unwrap().unwrap().mint);
        bare.push(hot_decode(&raw_of(&f), &[]).ok().flatten().map(|h| h.mint));
    }
    assert!(srv
        .geyser
        .push(fixture_update(&wallet_fx, slot, 40, &[F_WALLET, F_PUMP]).unwrap()));
    assert!(srv.geyser.push(slot_update(slot + 1)));
    assert!(srv.geyser.push(ping_update()));
    let mut buf = FeedBuf::with_capacity(256, 1 << 20);
    let (mut hits, mut own, mut held) = (vec![], 0, 0);
    let mut poll = |buf: &mut FeedBuf, hits: &mut Vec<Pubkey>, own: &mut u32, held: &mut u32| {
        buf.clear();
        g.poll(buf);
        for e in buf.events() {
            match e.payload {
                Payload::Wire(w) => {
                    assert_eq!(e.slot, slot);
                    let m = WireMeta::parse(buf.wire_meta(w)).expect("trailer");
                    assert!(m.tx_index.is_some() && !m.failed() && m.has_inner());
                    let hit = hot_decode(buf.wire_tx(w), buf.wire_meta(w))
                        .unwrap()
                        .unwrap();
                    hits.push(hit.mint);
                }
                Payload::Trade(t) if t.user == wallet => *own += 1,
                Payload::Trade(t) if t.mint == held_mint => *held += 1,
                _ => {}
            }
        }
    };
    wait("creates and own trade", 10, || {
        poll(&mut buf, &mut hits, &mut own, &mut held);
        hits.len() == 3 && own >= 1
    });
    assert_eq!(
        hits, mints,
        "the creates decode on the hot side, in stream order"
    );
    assert!(bare[2].is_none(), "the CPI create needs the trailer");
    assert_eq!(h.health().creates.load(Relaxed), 3);
    assert_eq!(h.health().decode_miss.load(Relaxed), 0);
    // the server's ping is answered with a ping request
    wait("ping reply", 10, || {
        srv.geyser.requests().iter().any(|r| r.ping.is_some())
    });
    // bundle runs of the three creates, closed by the next slot
    let mut runs = vec![];
    wait("bundle runs", 10, || {
        while let Ok(r) = bundles.try_recv() {
            runs.push(r);
        }
        runs.len() >= 2
    });
    for r in &runs {
        assert_eq!(r.closed, CloseReason::SlotEnded);
        assert_eq!(r.slot, slot);
        assert!(
            r.trades.iter().all(|t| t.is_create),
            "only the create's own buys in these fixtures"
        );
        assert!(!r.trades.is_empty(), "the dev buy is in the run");
        assert!(r.trades.windows(2).all(|w| w[0].index <= w[1].index));
    }
    // watch a held mint: the filters are re-sent with it; its trade arrives
    let n_req = srv.geyser.requests().len();
    assert!(h.watch_mint(held_mint));
    wait("resubscribe with mints", 10, || {
        srv.geyser.requests()[n_req..].iter().any(|r| {
            r.transactions
                .get(F_MINTS)
                .is_some_and(|f| f.account_include == vec![held_mint.to_string()])
        })
    });
    assert!(srv
        .geyser
        .push(fixture_update(&held_fx, slot + 2, 1, &[F_MINTS, F_PUMP]).unwrap()));
    wait("held mint trade", 10, || {
        poll(&mut buf, &mut hits, &mut own, &mut held);
        held >= 1
    });
    let n_req = srv.geyser.requests().len();
    assert!(h.unwatch_mint(held_mint));
    wait("resubscribe without mints", 10, || {
        srv.geyser.requests()[n_req..]
            .iter()
            .any(|r| !r.transactions.is_empty() && !r.transactions.contains_key(F_MINTS))
    });
    // re-watch, then go silent: the stale detector reconnects and the new
    // subscription carries the current watch
    assert!(h.watch_mint(held_mint));
    let conns = srv.geyser.connections();
    wait("stale reconnect", 10, || srv.geyser.connections() > conns);
    assert!(h.health().stale_reconnects.load(Relaxed) >= 1);
    wait("resubscribed", 10, || {
        srv.geyser
            .requests()
            .last()
            .is_some_and(|r| r.transactions.contains_key(F_MINTS) || r.ping.is_some())
    });
    // the server drops the stream: reconnect again
    srv.geyser.push(slot_update(slot + 5));
    let conns = srv.geyser.connections();
    srv.geyser.drop_stream();
    wait("reconnect after a dropped stream", 10, || {
        srv.geyser.connections() > conns
    });
    assert!(h.health().reconnects.load(Relaxed) >= 2);
    let subs: Vec<_> = srv
        .geyser
        .requests()
        .into_iter()
        .filter(|r| r.transactions.contains_key(F_CREATES))
        .collect();
    assert!(
        subs.last().unwrap().transactions.contains_key(F_MINTS),
        "a reconnect re-sends the held mints"
    );
    assert!(h.stats().slot.load(Relaxed) > slot);
    assert_eq!(h.stats().convert_errors.load(Relaxed), 0);
    g.shutdown();
}

#[test]
fn a_wrong_token_is_refused_and_retried() {
    let srv = MockServer::start(Some("right"));
    let mut g = Grpc::spawn(GrpcConfig {
        url: srv.url(),
        x_token: Some("wrong".into()),
        stale_ms: 300,
        min_backoff_ms: 10,
        max_backoff_ms: 50,
        ..Default::default()
    })
    .unwrap();
    let h = g.handle();
    wait("refusals", 10, || srv.geyser.refused() >= 2);
    assert_eq!(srv.geyser.connections(), 0);
    assert!(!h.health().connected.load(Relaxed));
    wait("retries", 10, || h.health().reconnects.load(Relaxed) >= 2);
    g.shutdown();
    assert!(Grpc::spawn(GrpcConfig {
        url: "ftp://x".into(),
        ..Default::default()
    })
    .is_err());
}
