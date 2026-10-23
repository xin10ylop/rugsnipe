#!/usr/bin/env python3
"""Patch a jito-shredstream-proxy checkout (AllenHark fork v0.2.13 / upstream v0.2.14)
so a gateway that mixes copies of the same shred does not break deshredding:

  1. deshred.rs: coding shreds are no longer dropped because a DATA shred of the
     same index was already deshredded (the two index spaces are different);
  2. deshred.rs: before a recovery attempt the FEC set keeps only the shreds that
     agree with the majority Merkle root (the gateway's conflicting copies fail
     recovery with "Invalid Merkle root" every time otherwise), and the attempt
     is skipped until enough consistent shreds exist;
  3. forwarder.rs: the reconstruct channel grows from 1,024 to 16,384 batches
     (try_send dropped batches while the reconstruct thread was busy).

Usage: python3 -I deploy/patches/shredproxy_recovery.py /opt/shredproxy
Idempotent: a file already patched is left alone. Exits non-zero on a text it
does not recognise (a different proxy version): patch by hand then.
"""
import sys, pathlib

def patch(path, old, new, marker):
    p = pathlib.Path(path)
    s = p.read_text()
    if marker in s:
        print(f"{p}: already patched ({marker})")
        return
    if old not in s:
        sys.exit(f"{p}: expected text not found for {marker}; proxy version differs, patch by hand")
    p.write_text(s.replace(old, new, 1))
    print(f"{p}: patched ({marker})")

root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else ".")
deshred = root / "proxy/src/deshred.rs"
forwarder = root / "proxy/src/forwarder.rs"

patch(deshred,
"""                if state_tracker.already_recovered_fec_sets[fec_set_index as usize]
                    || state_tracker.already_deshredded[index]
                {""",
"""                if state_tracker.already_recovered_fec_sets[fec_set_index as usize]
                    || (shred.shred_type() == ShredType::Data
                        && state_tracker.already_deshredded[index]) // rugsnipe: coding indexes are another space
                {""",
"rugsnipe: coding indexes are another space")

patch(deshred,
"""        // try to recover if we have enough shreds in the FEC set
        let merkle_shreds = shreds""",
"""        // rugsnipe: keep only the shreds agreeing with the majority Merkle root; a
        // gateway that mixes copies of a shred makes recovery fail on the odd one
        {
            let mut votes = ahash::HashMap::<solana_sdk::hash::Hash, usize>::default();
            for s in shreds.iter() {
                if let Some(r) = solana_ledger::shred::layout::get_merkle_root(s.payload()) {
                    *votes.entry(r).or_default() += 1;
                }
            }
            if let Some((root, _)) = votes.into_iter().max_by_key(|(_, n)| *n) {
                shreds.retain(|s| {
                    solana_ledger::shred::layout::get_merkle_root(s.payload()) == Some(root)
                });
            }
            if shreds.len() < min_shreds_needed_to_recover
                || !shreds.iter().any(|s| s.shred_type() == ShredType::Code)
            {
                continue;
            }
        }
        // try to recover if we have enough shreds in the FEC set
        let merkle_shreds = shreds""",
"rugsnipe: keep only the shreds agreeing with the majority Merkle root")

patch(forwarder,
"""    let (reconstruct_tx, reconstruct_rx) = crossbeam_channel::bounded(1_024);""",
"""    let (reconstruct_tx, reconstruct_rx) = crossbeam_channel::bounded(16_384); // rugsnipe: no silent batch drops""",
"rugsnipe: no silent batch drops")
print("done: rebuild with cargo build --release --bin jito-shredstream-proxy")
