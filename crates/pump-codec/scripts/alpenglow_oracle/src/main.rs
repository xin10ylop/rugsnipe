use base64::Engine;
use solana_entry::block_component::*;
use solana_entry::entry::Entry;
use solana_hash::Hash;
use solana_transaction::versioned::VersionedTransaction;
fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{:02x}", x)).collect() }
fn load(p: &str) -> Vec<u8> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    base64::engine::general_purpose::STANDARD.decode(v["result"]["transaction"][0].as_str().unwrap()).unwrap()
}
fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let names = ["legacy_buy18", "v0_create_v2_buy_v3", "v1_create_v2_buy"];
    let mut txs = vec![];
    for n in names {
        let raw = load(&format!("{dir}/{n}.json"));
        let tx: VersionedTransaction = wincode::deserialize(&raw).unwrap();
        assert_eq!(wincode::serialize(&tx).unwrap(), raw, "{n} re-serializes identically");
        txs.push(tx);
    }
    let e1 = Entry { num_hashes: 12_345, hash: Hash::new_from_array([7u8; 32]), transactions: vec![txs[0].clone(), txs[1].clone()] };
    let e2 = Entry { num_hashes: 1, hash: Hash::new_from_array([9u8; 32]), transactions: vec![txs[2].clone()] };
    let tick = Entry { num_hashes: 62_500, hash: Hash::new_from_array([3u8; 32]), transactions: vec![] };
    let batch = BlockComponent::new_entry_batch(vec![e1, tick, e2]).unwrap();
    let b = wincode::serialize(&batch).unwrap();
    println!("entry_batch {}", hex(&b));
    let hdr = BlockComponent::new_block_header(456_789_012, Hash::new_from_array([0xab; 32]));
    println!("header {}", hex(&wincode::serialize(&hdr).unwrap()));
    let up = BlockComponent::new_block_marker(VersionedBlockMarker::from_update_parent(UpdateParentV1 { new_parent_slot: 456_789_010, new_parent_block_id: Hash::new_from_array([0xcd; 32]) }));
    println!("update_parent {}", hex(&wincode::serialize(&up).unwrap()));
    let ft = BlockComponent::new_block_marker(VersionedBlockMarker::from_block_footer(BlockFooterV1 { bank_hash: Hash::new_from_array([0x11; 32]), block_producer_time_nanos: 1_791_968_762_123_456_789, block_user_agent: b"agave/4.3.1".to_vec(), block_final_cert: None, skip_reward_cert: None, notar_reward_cert: None }));
    println!("footer {}", hex(&wincode::serialize(&ft).unwrap()));
    // round trip check of our abort understanding
    println!("abort_is_empty {}", BlockComponent::infer_is_empty_entry_batch(&[0u8; 8]));
}
