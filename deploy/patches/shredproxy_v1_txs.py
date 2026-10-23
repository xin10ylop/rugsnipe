#!/usr/bin/env python3
"""Patch a jito-shredstream-proxy checkout (AllenHark fork v0.2.13 / upstream v0.2.14)
so entry batches holding version-1 (SIMD-0385) transactions are forwarded to the
gRPC entry subscribers instead of being dropped.

Why: the proxy is pinned to solana-sdk 2.2.1, which cannot deserialize a v1
VersionedTransaction (first byte 0x81). deshred.rs runs
`bincode::deserialize::<Vec<Entry>>` on every reconstructed batch and `continue`s on
an error (debug log only), so every batch with at least one v1 transaction is never
sent, although subscribers only ever receive the raw bytes (`Entry { slot, entries }`).

Fix (no crate bump): bincode stays the fast path; when it fails, a version-agnostic
walk of the bincode `Vec<Entry>` framing (legacy, v0, v1 transactions) decides whether
the payload is a complete batch. If so the raw bytes are forwarded unchanged and the
new `v1_entry_batch_count` metric (datapoint shredstream_proxy-service_metrics) counts
it; a payload neither accepts (e.g. a deshred from a wrong guessed start) is still
skipped and retried as before, so the guessed-start check keeps working.

Changes:
  1. deshred.rs: the deserialize step falls back to `entry_batch_framing`;
  2. deshred.rs: `entry_batch_framing` / `wire_transaction_len` (no new dependency);
  3. deshred.rs: tests (a real mainnet v1 transaction through shredding, FEC recovery
     and deshredding: the batch comes out byte for byte);
  4. forwarder.rs: the `v1_entry_batch_count` metric (field, init, report);
  5. proxy/Cargo.toml: `rayon` dev-dependency for the tests (already in Cargo.lock,
     no effect on the release binary).

Apply after deploy/patches/shredproxy_recovery.py (independent of it):
  python3 -I deploy/patches/shredproxy_v1_txs.py /opt/shredproxy
Idempotent: a change already present is left alone. Every anchor is checked before any
file is written; on a text it does not recognise (a different proxy version) it exits
non-zero without changing anything: patch by hand then.
"""
import sys, pathlib

root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else ".")
deshred = root / "proxy/src/deshred.rs"
forwarder = root / "proxy/src/forwarder.rs"
cargo_toml = root / "proxy/Cargo.toml"

PATCHES = []  # (path, old, new, marker)

# 1. deserialize step: fall back to the framing walk instead of dropping the batch
PATCHES.append((deshred,
"""        let entries = match bincode::deserialize::<Vec<solana_entry::entry::Entry>>(
            &deshredded_payload,
        ) {
            Ok(entries) => entries,
            Err(e) => {
                debug!(
                        "Failed to deserialize bincode payload of size {} for slot {slot}, start_data_complete_idx: {start_data_complete_idx}, end_data_complete_idx: {end_data_complete_idx}, unknown_start: {unknown_start}. Err: {e}",
                        deshredded_payload.len()
                    );
                metrics
                    .bincode_deserialize_error_count
                    .fetch_add(1, Ordering::Relaxed);
                if unknown_start {
                    metrics
                        .unknown_start_position_error_count
                        .fetch_add(1, Ordering::Relaxed);
                }
                continue;
            }
        };
        metrics
            .entry_count
            .fetch_add(entries.len() as u64, Ordering::Relaxed);
        let txn_count = entries.iter().map(|e| e.transactions.len() as u64).sum();
        metrics.txn_count.fetch_add(txn_count, Ordering::Relaxed);
        debug!(
            "Successfully decoded slot: {slot} start_data_complete_idx: {start_data_complete_idx} end_data_complete_idx: {end_data_complete_idx} with entry count: {}, txn count: {txn_count}",
            entries.len(),
        );
""",
"""        // rugsnipe: v1 transactions forwarded as raw bytes. solana-sdk 2.2 cannot
        // deserialize a v1 (SIMD-0385) transaction and bincode then rejects the whole
        // Vec<Entry>: such a batch is checked with a version-agnostic walk of the entry
        // framing instead and forwarded as is (subscribers only receive the raw bytes).
        // A payload neither accepts (e.g. a wrong guessed start) is still skipped.
        let (entries, entry_count, txn_count) =
            match bincode::deserialize::<Vec<solana_entry::entry::Entry>>(&deshredded_payload) {
                Ok(entries) => {
                    let txn_count: u64 =
                        entries.iter().map(|e| e.transactions.len() as u64).sum();
                    let entry_count = entries.len() as u64;
                    (entries, entry_count, txn_count)
                }
                Err(e) => match entry_batch_framing(&deshredded_payload) {
                    Some((entry_count, txn_count)) => {
                        metrics.v1_entry_batch_count.fetch_add(1, Ordering::Relaxed);
                        (Vec::new(), entry_count, txn_count)
                    }
                    None => {
                        debug!(
                            "Failed to deserialize bincode payload of size {} for slot {slot}, start_data_complete_idx: {start_data_complete_idx}, end_data_complete_idx: {end_data_complete_idx}, unknown_start: {unknown_start}. Err: {e}",
                            deshredded_payload.len()
                        );
                        metrics
                            .bincode_deserialize_error_count
                            .fetch_add(1, Ordering::Relaxed);
                        if unknown_start {
                            metrics
                                .unknown_start_position_error_count
                                .fetch_add(1, Ordering::Relaxed);
                        }
                        continue;
                    }
                },
            };
        metrics.entry_count.fetch_add(entry_count, Ordering::Relaxed);
        metrics.txn_count.fetch_add(txn_count, Ordering::Relaxed);
        debug!(
            "Successfully decoded slot: {slot} start_data_complete_idx: {start_data_complete_idx} end_data_complete_idx: {end_data_complete_idx} with entry count: {entry_count}, txn count: {txn_count}",
        );
""",
"rugsnipe: v1 transactions forwarded as raw bytes"))

# 2. the framing walk
PATCHES.append((deshred,
"""const SLOT_LOOKBACK: Slot = 50;
""",
"""// rugsnipe: version-agnostic Vec<Entry> framing
/// Walks the bincode framing of a `Vec<Entry>` (u64 entry count; per entry: u64
/// num_hashes, 32-byte hash, u64 transaction count, transactions) without deserializing
/// the transactions, so a batch holding v1 (SIMD-0385) transactions, which solana-sdk 2.2
/// cannot deserialize, is still recognised as a complete batch. Like `bincode::deserialize`,
/// bytes after the vector are allowed. Returns (entry count, transaction count), or None
/// when the bytes are not a well-formed batch (e.g. a deshred from a wrong guessed start).
fn entry_batch_framing(bytes: &[u8]) -> Option<(u64, u64)> {
    fn read_u64(b: &[u8], o: &mut usize) -> Option<u64> {
        let v = u64::from_le_bytes(b.get(*o..*o + 8)?.try_into().ok()?);
        *o += 8;
        Some(v)
    }
    let mut o = 0usize;
    let num_entries = read_u64(bytes, &mut o)?;
    // an entry takes at least 48 bytes: reject an absurd count up front
    if num_entries > (bytes.len() / 48) as u64 {
        return None;
    }
    let mut num_txs = 0u64;
    for _ in 0..num_entries {
        o += 8 + 32; // num_hashes, hash
        let n = read_u64(bytes, &mut o)?;
        if n > bytes.len().saturating_sub(o) as u64 {
            return None;
        }
        for _ in 0..n {
            o += wire_transaction_len(bytes.get(o..)?)?;
        }
        num_txs += n;
    }
    Some((num_entries, num_txs))
}

/// Length of the serialized transaction at the start of `b`, dispatched on the first byte
/// like Agave 4.x: below 0x80 a legacy/v0 transaction (signature count, signatures,
/// message), 0x81 a v1 transaction (SIMD-0385: message first, then num_required_signatures
/// signatures), anything else is rejected. None on a malformed or truncated transaction.
fn wire_transaction_len(b: &[u8]) -> Option<usize> {
    fn short_u16(b: &[u8], o: &mut usize) -> Option<usize> {
        let mut v = 0usize;
        for i in 0..3 {
            let byte = *b.get(*o)?;
            *o += 1;
            v |= ((byte & 0x7f) as usize) << (7 * i);
            if byte & 0x80 == 0 {
                return (v <= u16::MAX as usize).then_some(v);
            }
        }
        None
    }
    const SIGNATURE: usize = 64;
    const KEY: usize = 32;
    let first = *b.first()?;
    if first == 0x81 {
        // 0x81 | header (3) | config mask (u32) | lifetime (32) | num_ix (u8) |
        // num_addresses (u8) | addresses | config values (4 bytes per mask bit) |
        // ix headers (program u8, num_accounts u8, data_len u16) | per ix: accounts, data |
        // signatures
        let fixed = b.get(..42)?;
        let num_signatures = fixed[1] as usize;
        let mask = u32::from_le_bytes([fixed[4], fixed[5], fixed[6], fixed[7]]);
        let (num_ix, num_addresses) = (fixed[40] as usize, fixed[41] as usize);
        let mut o = 42 + num_addresses * KEY + 4 * mask.count_ones() as usize;
        let ix_headers = b.get(o..o + 4 * num_ix)?;
        o += 4 * num_ix;
        for h in ix_headers.chunks_exact(4) {
            o += h[1] as usize + u16::from_le_bytes([h[2], h[3]]) as usize;
        }
        o += num_signatures * SIGNATURE;
        return (num_signatures > 0 && o <= b.len()).then_some(o);
    }
    if first & 0x80 != 0 {
        return None; // unknown transaction version
    }
    // legacy / v0: one-byte signature count (< 128), signatures, message
    let num_signatures = first as usize;
    let mut o = 1 + num_signatures * SIGNATURE;
    let v0 = match *b.get(o)? {
        0x80 => true,
        prefix if prefix & 0x80 != 0 => return None, // unknown message version
        _ => false,
    };
    o += usize::from(v0);
    // header: num_required_signatures must match the signature count
    if num_signatures == 0 || *b.get(o)? as usize != num_signatures {
        return None;
    }
    o += 3;
    let num_keys = short_u16(b, &mut o)?;
    o += num_keys * KEY + 32; // account keys, recent blockhash
    let num_ix = short_u16(b, &mut o)?;
    for _ in 0..num_ix {
        o += 1; // program id index
        let n = short_u16(b, &mut o)?; // account indexes
        o += n;
        let n = short_u16(b, &mut o)?; // data
        o += n;
    }
    if v0 {
        let num_lookups = short_u16(b, &mut o)?;
        for _ in 0..num_lookups {
            o += KEY; // table address
            let n = short_u16(b, &mut o)?; // writable indexes
            o += n;
            let n = short_u16(b, &mut o)?; // readonly indexes
            o += n;
        }
    }
    (o <= b.len()).then_some(o)
}

const SLOT_LOOKBACK: Slot = 50;
""",
"rugsnipe: version-agnostic Vec<Entry> framing"))

# 3. tests
PATCHES.append((deshred,
"""#[cfg(test)]
mod get_indexes_tests {
""",
"""// rugsnipe: v1 entry batch tests
#[cfg(test)]
mod v1_entry_batch_tests {
    use std::sync::atomic::Ordering;

    use itertools::Itertools;
    use solana_ledger::shred::{merkle, ProcessShredsStats, ReedSolomonCache, ShredType};
    use solana_perf::packet::{Packet, PacketBatch};
    use solana_sdk::{
        hash::Hash,
        message::{v0, VersionedMessage},
        pubkey::Pubkey,
        signature::Keypair,
        signer::Signer,
        system_instruction, system_transaction,
        transaction::VersionedTransaction,
    };

    use super::{entry_batch_framing, reconstruct_shreds, wire_transaction_len};
    use crate::forwarder::ShredMetrics;

    /// Mainnet v1 (SIMD-0385) transaction, wire bytes: pump buy_exact_quote_in_v3, slot
    /// 456782408, 4HvEBDRtTnKteXsCFPbfdisLPwmsEu3cXcJsev4WdZUnPY6FjQSKpwHKowFa9kGJWcdFUCGU4wMNktiMehV8rA3q
    /// (rugsnipe crates/pump-codec/fixtures/tx/v1_buy_exact_quote_in_v3.json).
    const V1_TX_HEX: &str = concat!(
        "8101000b0f000000e3e549193786afa149eeab194cbd22b138203ff132e2161e7d28e43e772ddda10213703687ef3946",
        "acce87a5f61a018f88114b82b62f5116dc98324a5d01c56ea057207cecda5bcc6cb1eaf0f16d68404566b18d56d2481a",
        "cb317032656e90551c78394212ce4ed4e276940bb3e043446ebc326763aa5bdc1493095756ee841b5bb23f1ce0d0da8c",
        "30857a11f826e18de3f8e5748c8831dd5710b5d6cf46c5accffd82579be55b011b418bd0c822db6161a0f2b86ff038f1",
        "6faca503c27e0bc0d34eba835a98d37a3fd62e82f7ccba8ab4ee2404e255f12390c19abd211658fccdd4c00054546880",
        "6070b8e53559be60b2bfc655ea361f80f37d3b22315f2fa7f49fd948f8c1f4e89e0c8bdc214468073a45d1da605c8fd6",
        "1482f898d9e28a9e55d600000000000000000000000000000000000000000000000000000000000000000156e0f69366",
        "5acf44db1568bf175baa5189cb97f5d2ff3b655d2bb6fd6d18b0069b8857feab8184fb687f634618c035dac439dc1aeb",
        "3b5598a0f0000000000106ddf6e1d765a193d9cbe146ceeb79ac1cb485ed5f5b37913a8cf5857eff00a906ddf6e1ee75",
        "8fde18425dbce46ccddab61afc4d83b90d27febdf928d8a18bfc3a865e69ee0f5480cabcf66357e4dc2f18d58d45c1ea",
        "7489fb3723d9793c72a643fe2dbba5581d5cc8e9e88b3aa3d1fb68dfa00f8b5c312b334cc01fb2fa48d56f9ab4a4f195",
        "8dc0a9c94c3fb72c07995843eda485e3a24f10c69399f819940f8c97258f4e2489f1bb3d1029148e0d830b5a1399daff",
        "1084048e7bd8dbe9f859936c9a6971bf58aae30708ac8239e603fc19bf5b26a303d96871cb3d9430433facf136eb01fc",
        "1c4e883d23c8b5844ab59a37f66add57c5e9ac3b53e059d35c64bc3d030000000000a80b010000000001100601000911",
        "190000040e11080c010d110a0c0b050703000406020f01081209e1f7501ed5b38488fb271f0100000000400426762e18",
        "000000a4a3e33aacbd58c743c3d9961fa17f15c34456c6b2ea513554491df5292ae50b360471be7bf250dba80d2e5b76",
        "e43fe14644a8319eb429c420557363e5dbe200",
    );

    fn v1_tx() -> Vec<u8> {
        (0..V1_TX_HEX.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&V1_TX_HEX[i..i + 2], 16).unwrap())
            .collect()
    }

    /// bincode `Vec<Entry>`: [legacy], [legacy, v1, v0] (the v1 one left out when
    /// `with_v1` is false), a tick. Returns the bytes and the offset of the v1 transaction.
    fn entry_batch(with_v1: bool) -> (Vec<u8>, usize) {
        let payer = Keypair::new();
        let legacy = VersionedTransaction::from(system_transaction::transfer(
            &payer,
            &Pubkey::new_unique(),
            1,
            Hash::new_unique(),
        ));
        let message = v0::Message::try_compile(
            &payer.pubkey(),
            &[system_instruction::transfer(&payer.pubkey(), &Pubkey::new_unique(), 2)],
            &[],
            Hash::new_unique(),
        )
        .unwrap();
        let v0 = VersionedTransaction::try_new(VersionedMessage::V0(message), &[&payer]).unwrap();
        let (legacy, v0, v1) = (
            bincode::serialize(&legacy).unwrap(),
            bincode::serialize(&v0).unwrap(),
            v1_tx(),
        );
        let entry = |num_hashes: u64, txs: &[&[u8]]| {
            let mut e = num_hashes.to_le_bytes().to_vec();
            e.extend_from_slice(Hash::new_unique().as_ref());
            e.extend_from_slice(&(txs.len() as u64).to_le_bytes());
            txs.iter().for_each(|tx| e.extend_from_slice(tx));
            e
        };
        let mut batch = 3u64.to_le_bytes().to_vec();
        batch.extend(entry(1, &[&legacy]));
        let v1_at = batch.len() + 48 + legacy.len();
        if with_v1 {
            batch.extend(entry(7, &[&legacy, &v1, &v0]));
        } else {
            batch.extend(entry(7, &[&legacy, &v0]));
        }
        batch.extend(entry(12_500, &[]));
        (batch, v1_at)
    }

    #[test]
    fn test_v1_transaction_fails_bincode_but_not_the_framing() {
        // the same batch without the v1 transaction: solana-sdk 2.2 reads it
        let (without, _) = entry_batch(false);
        let entries = bincode::deserialize::<Vec<solana_entry::entry::Entry>>(&without).unwrap();
        assert_eq!(entries.iter().map(|e| e.transactions.len()).collect_vec(), [1, 2, 0]);
        assert_eq!(entry_batch_framing(&without), Some((3, 3)));

        // with it, the whole batch fails to deserialize (what dropped it before)
        let (batch, v1_at) = entry_batch(true);
        assert!(bincode::deserialize::<Vec<solana_entry::entry::Entry>>(&batch).is_err());
        assert_eq!(wire_transaction_len(&batch[v1_at..]), Some(v1_tx().len()));
        assert_eq!(entry_batch_framing(&batch), Some((3, 4)));

        // trailing bytes are allowed (as bincode allows them) ...
        let mut trailing = batch.clone();
        trailing.extend_from_slice(&[0u8; 16]);
        assert_eq!(entry_batch_framing(&trailing), Some((3, 4)));
        // ... a truncated batch, an unknown version or a cut transaction is not
        for cut in 0..batch.len() {
            assert_eq!(entry_batch_framing(&batch[..cut]), None, "cut at {cut}");
        }
        let mut unknown = batch.clone();
        unknown[v1_at] = 0x82;
        assert_eq!(entry_batch_framing(&unknown), None);
        assert_eq!(entry_batch_framing(&batch[v1_at..]), None);
    }

    #[test]
    fn test_v1_entry_batch_is_reconstructed_and_forwarded() {
        let (batch, _) = entry_batch(true);
        let slot = 33_333;
        let rs_cache = ReedSolomonCache::default();
        let thread_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap();
        let shreds = merkle::make_shreds_from_data(
            &thread_pool,
            &Keypair::new(),
            Some(Hash::new_unique()), // chained_merkle_root
            &batch,
            slot,
            slot - 1, // parent_slot
            0,        // shred_version
            0,        // reference_tick
            true,     // is_last_in_slot
            0,        // next_shred_index
            0,        // next_code_index
            &rs_cache,
            &mut ProcessShredsStats::default(),
        )
        .unwrap();
        assert!(shreds.iter().any(|s| s.shred_type() == ShredType::Code));
        let to_packet = |s: &merkle::Shred| {
            let bytes: &[u8] = s.payload();
            let mut packet = Packet::default();
            packet.buffer_mut()[..bytes.len()].copy_from_slice(bytes);
            packet.meta_mut().size = bytes.len();
            packet
        };

        // every shred; then without the first two data shreds (FEC recovery)
        for drop_data_shreds in [0, 2] {
            let packets = shreds
                .iter()
                .filter(|s| s.shred_type() == ShredType::Code || s.index() >= drop_data_shreds)
                .map(to_packet)
                .collect_vec();
            let metrics = ShredMetrics::default();
            let mut all_shreds = ahash::HashMap::default();
            let mut slot_fec_indexes_to_iterate = Vec::new();
            let mut deshredded_entries = Vec::new();
            let mut highest_slot_seen = 0;
            let recovered_count = reconstruct_shreds(
                PacketBatch::new(packets),
                &mut all_shreds,
                &mut slot_fec_indexes_to_iterate,
                &mut deshredded_entries,
                &mut highest_slot_seen,
                &rs_cache,
                &metrics,
            );
            assert_eq!(recovered_count > 0, drop_data_shreds > 0);
            // exactly what the forwarder sends as Entry { slot, entries }
            assert_eq!(deshredded_entries.len(), 1, "dropped {drop_data_shreds}");
            assert_eq!(deshredded_entries[0].0, slot);
            assert_eq!(deshredded_entries[0].2, batch);
            assert_eq!(metrics.v1_entry_batch_count.load(Ordering::Relaxed), 1);
            assert_eq!(metrics.entry_count.load(Ordering::Relaxed), 3);
            assert_eq!(metrics.txn_count.load(Ordering::Relaxed), 4);
            assert_eq!(metrics.bincode_deserialize_error_count.load(Ordering::Relaxed), 0);
        }
    }
}

#[cfg(test)]
mod get_indexes_tests {
""",
"rugsnipe: v1 entry batch tests"))

# 4. metric
PATCHES.append((forwarder,
"""    /// Number of bincode Entry deserialization errors
    pub bincode_deserialize_error_count: AtomicU64,
""",
"""    /// Number of bincode Entry deserialization errors
    pub bincode_deserialize_error_count: AtomicU64,
    /// rugsnipe: entry batches solana-sdk 2.2 cannot deserialize (v1 transactions), forwarded raw
    pub v1_entry_batch_count: AtomicU64,
""",
"rugsnipe: entry batches solana-sdk 2.2 cannot deserialize"))

PATCHES.append((forwarder,
"""            bincode_deserialize_error_count: Default::default(),
""",
"""            bincode_deserialize_error_count: Default::default(),
            v1_entry_batch_count: Default::default(), // rugsnipe: v1_entry_batch_count init
""",
"rugsnipe: v1_entry_batch_count init"))

PATCHES.append((forwarder,
"""                (
                    "bincode_deserialize_error_count",
                    self.bincode_deserialize_error_count
                        .swap(0, Ordering::Relaxed),
                    i64
                ),
""",
"""                (
                    "bincode_deserialize_error_count",
                    self.bincode_deserialize_error_count
                        .swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    // rugsnipe: v1_entry_batch_count report
                    "v1_entry_batch_count",
                    self.v1_entry_batch_count.swap(0, Ordering::Relaxed),
                    i64
                ),
""",
"rugsnipe: v1_entry_batch_count report"))

# 5. dev-dependency for the tests (appended; the proxy has no [dev-dependencies] yet)
CARGO_MARKER = "rugsnipe: shreds built from raw entry bytes in tests"
cargo_text = cargo_toml.read_text()
if CARGO_MARKER not in cargo_text and "[dev-dependencies]" in cargo_text:
    sys.exit(f"{cargo_toml}: already has [dev-dependencies]; add `rayon = \"1.10\"` there by hand")
PATCHES.append((cargo_toml,
"""tonic = { workspace = true }
""",
"""tonic = { workspace = true }

[dev-dependencies]
rayon = "1.10" # rugsnipe: shreds built from raw entry bytes in tests
""",
CARGO_MARKER))

# check every anchor first, then write: never leave a half-patched tree
texts = {}
todo = []
for path, old, new, marker in PATCHES:
    s = texts.setdefault(path, path.read_text())
    if marker in s:
        print(f"{path}: already patched ({marker})")
        continue
    if s.count(old) != 1:
        sys.exit(f"{path}: expected text not found once for {marker}; proxy version differs, patch by hand (nothing written)")
    todo.append((path, old, new, marker))
for path, old, new, marker in todo:
    texts[path] = texts[path].replace(old, new, 1)
for path in {p for p, _, _, _ in todo}:
    path.write_text(texts[path])
for path, _, _, marker in todo:
    print(f"{path}: patched ({marker})")

# Cargo.lock (best effort, never fails): the rayon edge of the dev-dependency, so a
# `cargo build --locked` works too; without it a plain cargo build adds the line itself
lock = root / "Cargo.lock"
if lock.exists():
    s = lock.read_text()
    head = '[[package]]\nname = "jito-shredstream-proxy"\n'
    start = s.find(head)
    end = s.find("[[package]]", start + len(head)) if start >= 0 else -1
    block = s[start:end if end >= 0 else len(s)] if start >= 0 else ""
    if ' "rayon",\n' in block:
        print(f"{lock}: already has the rayon edge")
    elif block.count('\n "rand 0.8.5",\n') == 1:
        lock.write_text(s[:start] + block.replace('\n "rand 0.8.5",\n', '\n "rand 0.8.5",\n "rayon",\n', 1)
                        + s[start + len(block):])
        print(f"{lock}: added the rayon edge to jito-shredstream-proxy")
    else:
        print(f"{lock}: not updated (cargo adds the rayon edge itself; build without --locked)")
if "rugsnipe: no silent batch drops" not in forwarder.read_text():
    print("note: deploy/patches/shredproxy_recovery.py is not applied to this tree")
print("done: rebuild with cargo build --release --bin jito-shredstream-proxy")
