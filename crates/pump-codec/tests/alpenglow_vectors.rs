//! `alpenglow::parse_block_component` against bytes serialized by the real
//! `solana-entry 4.3.1` (fixtures/alpenglow/solana_entry_4_3_1.json).

mod common;

use common::*;
use pump_codec::alpenglow::{parse_block_component, BlockComponent, BlockMarker};
use pump_codec::tx::{TxVersion, TxView};

fn vector(name: &str) -> Vec<u8> {
    let v = read_json("alpenglow/solana_entry_4_3_1.json");
    let h = v["vectors"][name].as_str().unwrap();
    (0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn entry_batch_from_solana_entry_slices_real_transactions() {
    let b = vector("entry_batch");
    let BlockComponent::EntryBatch(eb) = parse_block_component(&b).unwrap() else {
        panic!("not an entry batch")
    };
    assert_eq!(eb.num_entries(), 3);
    assert_eq!(eb.num_transactions(), 3);
    let entries: Vec<_> = eb.entries().collect();
    assert_eq!(entries[0].num_hashes, 12_345);
    assert_eq!(entries[0].hash, &[7u8; 32]);
    assert_eq!(entries[0].num_transactions(), 2);
    assert!(entries[1].is_tick());
    assert_eq!(entries[1].num_hashes, 62_500);
    assert_eq!(entries[2].hash, &[9u8; 32]);
    let want = ["legacy_buy18", "v0_create_v2_buy_v3", "v1_create_v2_buy"];
    let versions = [TxVersion::Legacy, TxVersion::V0, TxVersion::V1];
    for ((raw, name), ver) in eb.transactions().zip(want).zip(versions) {
        let fx = TxFixture::load(name);
        assert_eq!(raw, fx.raw.as_slice(), "{name}");
        let tx = TxView::parse(raw).unwrap();
        assert_eq!(tx.version(), ver);
        assert_eq!(tx.transaction_id().unwrap(), fx.signature);
    }
    // any truncation is an error, never a panic or a partial batch
    for cut in [9, 50, b.len() / 2, b.len() - 1] {
        assert!(parse_block_component(&b[..cut]).is_err(), "cut {cut}");
    }
}

#[test]
fn markers_and_abort_from_solana_entry() {
    match parse_block_component(&vector("header")).unwrap() {
        BlockComponent::Marker(BlockMarker::Header(p)) => {
            assert_eq!(p.slot, 456_789_012);
            assert_eq!(p.block_id, [0xab; 32]);
        }
        o => panic!("{o:?}"),
    }
    match parse_block_component(&vector("update_parent")).unwrap() {
        BlockComponent::Marker(BlockMarker::UpdateParent(p)) => {
            assert_eq!(p.slot, 456_789_010);
            assert_eq!(p.block_id, [0xcd; 32]);
        }
        o => panic!("{o:?}"),
    }
    match parse_block_component(&vector("footer")).unwrap() {
        BlockComponent::Marker(BlockMarker::Footer(f)) => {
            assert_eq!(f.bank_hash, [0x11; 32]);
            assert_eq!(f.block_producer_time_nanos, 1_791_968_762_123_456_789);
            assert_eq!(f.user_agent, b"agave/4.3.1");
        }
        o => panic!("{o:?}"),
    }
    assert_eq!(
        parse_block_component(&vector("abort")).unwrap(),
        BlockComponent::Abort
    );
}

#[test]
fn parse_prefix_slices_concatenated_transactions() {
    let mut buf = Vec::new();
    let names = [
        "legacy_buy18",
        "fresh_v0alt_create_v2_buy_v3",
        "v1_create_v2_buy",
    ];
    for n in names {
        buf.extend_from_slice(&TxFixture::load(n).raw);
    }
    let mut o = 0;
    for n in names {
        let (tx, len) = TxView::parse_prefix(&buf[o..]).unwrap();
        assert_eq!(tx.raw(), TxFixture::load(n).raw.as_slice());
        o += len;
    }
    assert_eq!(o, buf.len());
    // exact parse still rejects trailing bytes
    assert!(TxView::parse(&buf).is_err());
}
