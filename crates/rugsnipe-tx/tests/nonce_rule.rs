//! The no-double-buy rule: two boxes, one chain nonce, many mints.
//! Oracles: `solana-nonce 3.4` (next value, account layout) and
//! `solana-system-interface` (nonce management instructions).

use pump_codec::Pubkey;
use rugsnipe_tx::nonce::*;
use rugsnipe_tx::sysix;

/// A chain with N nonce accounts; a tx executes iff its lifetime equals the
/// current value; executing advances the value at that slot.
struct Chain {
    values: Vec<[u8; 32]>,
    changed_at: Vec<u64>,
    landed: Vec<(Pubkey, u64)>,
}

impl Chain {
    fn new(n: usize) -> Self {
        Self { values: (0..n).map(|i| next_value(&[i as u8; 32])).collect(), changed_at: vec![100; n], landed: vec![] }
    }
    fn submit(&mut self, idx: usize, lifetime: [u8; 32], mint: Pubkey, slot: u64) -> bool {
        if self.values[idx] != lifetime {
            return false;
        }
        let mut parent = [0u8; 32];
        parent[..8].copy_from_slice(&slot.to_le_bytes());
        self.values[idx] = next_value(&parent);
        self.changed_at[idx] = slot;
        self.landed.push((mint, slot));
        true
    }
    /// The account stream delivers the current value to a box.
    fn sync(&self, pool: &NoncePool) {
        for i in 0..self.values.len() {
            if pool.value(i) != self.values[i] || pool.packed(i).last_change_slot == UNKNOWN_SLOT {
                pool.on_value(i, self.values[i], self.changed_at[i]);
            }
        }
    }
}

fn pool(n: usize) -> NoncePool {
    let accts: Vec<Pubkey> = (0..n).map(|i| Pubkey::new_from_array([i as u8 + 1; 32])).collect();
    NoncePool::new(Pubkey::new_from_array([0xaa; 32]), &accts)
}

#[test]
fn two_boxes_same_pre_create_value_fire_at_most_once() {
    let mut chain = Chain::new(4);
    let (a, b) = (pool(4), pool(4));
    chain.sync(&a);
    chain.sync(&b);
    let mint = Pubkey::new_unique();
    let create_slot = 1_000;
    let ca = a.try_claim(&mint, create_slot, 1, create_slot).unwrap();
    let cb = b.try_claim(&mint, create_slot, 1, create_slot).unwrap();
    assert_eq!(ca.idx, cb.idx, "every box maps a mint to the same nonce");
    assert_eq!(ca.value, cb.value, "same pre-create value");
    // three variants from each box, any order
    let mut landed = 0;
    for _ in 0..3 {
        landed += chain.submit(ca.idx, ca.value, mint, create_slot) as u32;
        landed += chain.submit(cb.idx, cb.value, mint, create_slot + 1) as u32;
    }
    assert_eq!(landed, 1);
    // the value changed at a slot >= create: after sync, nobody may fire again
    chain.sync(&a);
    chain.sync(&b);
    assert_eq!(a.packed(ca.idx).state, NonceState::Free);
    for p in [&a, &b] {
        assert!(matches!(p.try_claim(&mint, create_slot, 2, create_slot + 5), Err(NonceSkip::ChangedSinceCreate { .. })));
    }
}

#[test]
fn a_change_at_slot_ge_create_disqualifies() {
    let p = pool(1);
    p.on_value(0, [1; 32], 500);
    let m = Pubkey::new_unique();
    assert!(p.try_claim(&m, 500, 1, 500).is_err(), "change at == create slot");
    assert!(matches!(p.try_claim(&m, 499, 1, 500), Err(NonceSkip::ChangedSinceCreate { last_change_slot: 500 })));
    let c = p.try_claim(&m, 501, 1, 501).unwrap();
    assert_eq!(c.value, [1; 32]);
    // InFlight: a second claim (another mint colliding on the slot) is skipped
    assert_eq!(p.try_claim(&Pubkey::new_unique(), 600, 2, 600), Err(NonceSkip::NotFree(NonceState::InFlight)));
    // landed -> value changes -> Free with the new slot
    assert_eq!(p.on_value(0, [2; 32], 502), NonceState::InFlight);
    assert_eq!(p.packed(0).state, NonceState::Free);
    // a mint created before the change can never fire again
    assert!(p.try_claim(&m, 501, 1, 503).is_err());
}

#[test]
fn unknown_value_never_fires_and_no_probing() {
    let p = pool(8);
    let m = Pubkey::new_unique();
    assert!(matches!(p.try_claim(&m, u64::MAX >> 30, 1, 0), Err(NonceSkip::ChangedSinceCreate { last_change_slot: UNKNOWN_SLOT })));
    for i in 0..8 {
        p.on_value(i, [i as u8; 32], 10);
    }
    let idx = p.index_for(&m);
    // occupy the mint's slot: the claim must fail rather than probe idx+1
    p.try_claim(&m, 20, 1, 20).unwrap();
    for i in 0..8 {
        if i != idx {
            assert_eq!(p.packed(i).state, NonceState::Free);
        }
    }
    assert_eq!(p.try_claim(&m, 20, 1, 20), Err(NonceSkip::NotFree(NonceState::InFlight)));
    assert_eq!(p.free_count(), 7);
}

#[test]
fn burn_lifecycle_and_quarantine() {
    let p = pool(2);
    p.on_value(0, [1; 32], 10);
    p.on_value(1, [1; 32], 10);
    let m = (0..).map(|_| Pubkey::new_unique()).find(|m| p.index_for(m) == 0).unwrap();
    p.try_claim(&m, 20, 77, 20).unwrap();
    assert_eq!(p.intent(0), 77);
    assert!(p.due_for_burn(22, 3).is_empty());
    assert_eq!(p.due_for_burn(23, 3), vec![0]);
    assert!(p.start_burn(0));
    assert!(!p.start_burn(0));
    assert_eq!(p.burn_failed(0), NonceState::Burning);
    assert_eq!(p.burn_failed(0), NonceState::Burning);
    assert_eq!(p.burn_failed(0), NonceState::Quarantined);
    // a value change does not silently un-quarantine
    p.on_value(0, [2; 32], 30);
    assert_eq!(p.packed(0).state, NonceState::Quarantined);
    p.unquarantine(0);
    assert_eq!(p.packed(0).state, NonceState::Free);
    // burn that lands: Burning -> Free on the value change (late_fill detection is the caller's)
    let c = p.try_claim(&m, 40, 1, 40).unwrap();
    assert!(p.start_burn(c.idx));
    assert_eq!(p.on_value(0, [3; 32], 41), NonceState::Burning);
    assert_eq!(p.packed(0).state, NonceState::Free);
    // release of an unsent claim
    let c = p.try_claim(&m, 50, 1, 50).unwrap();
    p.release_unsent(&c);
    assert_eq!(p.packed(0).state, NonceState::Free);
}

#[test]
fn randomized_two_box_interleavings_never_double_buy() {
    let mut s: u64 = 0x1234_5678;
    let mut rnd = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    for round in 0..1_000 {
        let n = 1 + (rnd() % 4) as usize;
        let mut chain = Chain::new(n);
        let mut boxes = [pool(n), pool(n)];
        let mints: Vec<Pubkey> = (0..6).map(|_| Pubkey::new_unique()).collect();
        let mut slot = 200u64;
        let mut pending: Vec<(usize, usize, [u8; 32], Pubkey)> = vec![];
        // a mint is created exactly once; later sightings (other feeds, late
        // frames) carry the same create slot
        let mut created: std::collections::HashMap<Pubkey, u64> = Default::default();
        for _step in 0..60 {
            slot += 1 + rnd() % 2;
            match rnd() % 4 {
                0 => {
                    // a create seen by one or both boxes (possibly late)
                    let m = mints[(rnd() % 6) as usize];
                    let first = slot - rnd() % 3;
                    let create_slot = *created.entry(m).or_insert(first);
                    for (bi, b) in boxes.iter().enumerate() {
                        if rnd() % 3 == 0 {
                            continue;
                        }
                        if let Ok(c) = b.try_claim(&m, create_slot, round, slot) {
                            for _ in 0..(1 + rnd() % 3) {
                                pending.push((bi, c.idx, c.value, m));
                            }
                        }
                    }
                }
                1 => {
                    // deliver some pending variants to the chain
                    for _ in 0..(rnd() % 4) {
                        if pending.is_empty() {
                            break;
                        }
                        let i = (rnd() as usize) % pending.len();
                        let (_, idx, v, m) = pending.swap_remove(i);
                        chain.submit(idx, v, m, slot);
                    }
                }
                2 => {
                    // account stream to a random box (possibly stale for the other)
                    chain.sync(&boxes[(rnd() % 2) as usize]);
                }
                _ => {
                    // restart: a box loses its in-memory InFlight state
                    let b = (rnd() % 2) as usize;
                    let fresh = pool(n);
                    chain.sync(&fresh);
                    boxes[b] = fresh;
                }
            }
        }
        // flush everything still in flight
        for (_, idx, v, m) in pending.drain(..) {
            slot += 1;
            chain.submit(idx, v, m, slot);
        }
        let mut per_mint = std::collections::HashMap::new();
        for (m, _) in &chain.landed {
            *per_mint.entry(*m).or_insert(0) += 1;
        }
        assert!(per_mint.values().all(|&c| c <= 1), "round {round}: double buy {per_mint:?}");
    }
}

#[test]
fn next_value_matches_solana_nonce() {
    for seed in 0..20u8 {
        let bh = [seed.wrapping_mul(37); 32];
        let theirs = solana_nonce::state::DurableNonce::from_blockhash(&solana_hash::Hash::new_from_array(bh));
        assert_eq!(next_value(&bh), theirs.as_hash().to_bytes());
    }
}

#[test]
fn nonce_account_layout_matches_solana_nonce() {
    let auth = Pubkey::new_unique();
    let dn = solana_nonce::state::DurableNonce::from_blockhash(&solana_hash::Hash::new_from_array([5; 32]));
    let data = solana_nonce::state::Data::new(solana_pubkey_from(&auth), dn, 5_000);
    let v = solana_nonce::versions::Versions::new(solana_nonce::state::State::Initialized(data));
    let bytes = bincode::serialize(&v).unwrap();
    assert_eq!(bytes.len(), 80);
    let (a, val, lps) = parse_nonce_account(&bytes).unwrap();
    assert_eq!((a, val, lps), (auth, dn.as_hash().to_bytes(), 5_000));
    let un = bincode::serialize(&solana_nonce::versions::Versions::new(solana_nonce::state::State::Uninitialized)).unwrap();
    assert!(parse_nonce_account(&un).is_none());
}

fn solana_pubkey_from<P: From<[u8; 32]>>(k: &Pubkey) -> P {
    P::from(k.to_bytes())
}

#[test]
fn nonce_management_instructions_match_system_interface() {
    use solana_system_interface::instruction as si;
    let auth = Pubkey::new_unique();
    let (addr, ixs) = sysix::nonce_init_instructions(&auth, "buy", 3, sysix::NONCE_RENT_LAMPORTS);
    assert_eq!(addr, sysix::create_with_seed(&auth, "rs-buy-3", &pump_codec::constants::SYSTEM_PROGRAM_ID));
    let theirs = si::create_nonce_account_with_seed(&solana_pubkey_from(&auth), &solana_pubkey_from(&addr), &solana_pubkey_from(&auth), "rs-buy-3", &solana_pubkey_from(&auth), sysix::NONCE_RENT_LAMPORTS);
    assert_eq!(theirs.len(), 2);
    for (ours, t) in ixs.iter().zip(&theirs) {
        assert_eq!(ours.data, t.data);
        let om: Vec<_> = ours.accounts.iter().map(|m| (m.pubkey.to_bytes(), m.is_signer, m.is_writable)).collect();
        let tm: Vec<_> = t.accounts.iter().map(|m| (m.pubkey.to_bytes(), m.is_signer, m.is_writable)).collect();
        assert_eq!(om, tm);
    }
    let to = Pubkey::new_unique();
    let w = sysix::withdraw_nonce_account(&addr, &auth, &to, 42);
    let tw = si::withdraw_nonce_account(&solana_pubkey_from(&addr), &solana_pubkey_from(&auth), &solana_pubkey_from(&to), 42);
    assert_eq!(w.data, tw.data);
    assert_eq!(w.accounts.len(), tw.accounts.len());
    // advance (template ix) vs system interface
    let a = pump_codec::instructions::advance_nonce_account(&addr, &auth);
    let ta = si::advance_nonce_account(&solana_pubkey_from(&addr), &solana_pubkey_from(&auth));
    assert_eq!(a.data, ta.data);
    assert_eq!(sysix::pool_addresses(&auth, "buy", 4)[3], addr);
}
