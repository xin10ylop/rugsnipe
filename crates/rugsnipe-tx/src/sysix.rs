//! System-program instructions for durable-nonce account management
//! (`rugsnipe-ctl nonce init/close`, and the signed
//! `rugsnipe-ctl nonce create`). Nonce accounts are created **with a
//! seed** from the authority, so no extra keypairs exist:
//! `address = sha256(base ‖ seed ‖ system_program)`.

use pump_codec::constants::{RECENT_BLOCKHASHES_SYSVAR_ID, RENT_SYSVAR_ID, SYSTEM_PROGRAM_ID};
use pump_codec::{AccountMeta, Instruction, Pubkey};
use sha2::{Digest, Sha256};

/// Size of a nonce account.
pub const NONCE_ACCOUNT_LEN: u64 = 80;
/// Rent-exempt minimum for 80 bytes (verified 2026-10-14: 1,056,640 lamports).
pub const NONCE_RENT_LAMPORTS: u64 = 1_056_640;

/// `Pubkey::create_with_seed(base, seed, owner)`.
pub fn create_with_seed(base: &Pubkey, seed: &str, owner: &Pubkey) -> Pubkey {
    let mut h = Sha256::new();
    h.update(base.as_ref());
    h.update(seed.as_bytes());
    h.update(owner.as_ref());
    Pubkey::new_from_array(h.finalize().into())
}

/// Seed of nonce `i` of `pool` (`"buy"` / `"sell"`): `rs-<pool>-<i>`.
pub fn nonce_seed(pool: &str, i: usize) -> String {
    format!("rs-{pool}-{i}")
}

/// System `CreateAccountWithSeed` (tag 3).
pub fn create_account_with_seed(
    from: &Pubkey,
    to: &Pubkey,
    base: &Pubkey,
    seed: &str,
    lamports: u64,
    space: u64,
    owner: &Pubkey,
) -> Instruction {
    let mut data = Vec::with_capacity(4 + 32 + 8 + seed.len() + 8 + 8 + 32);
    data.extend_from_slice(&3u32.to_le_bytes());
    data.extend_from_slice(base.as_ref());
    data.extend_from_slice(&(seed.len() as u64).to_le_bytes());
    data.extend_from_slice(seed.as_bytes());
    data.extend_from_slice(&lamports.to_le_bytes());
    data.extend_from_slice(&space.to_le_bytes());
    data.extend_from_slice(owner.as_ref());
    let mut accounts = vec![AccountMeta::new(*from, true), AccountMeta::new(*to, false)];
    if base != from {
        accounts.push(AccountMeta::new_readonly(*base, true));
    }
    Instruction {
        program_id: SYSTEM_PROGRAM_ID,
        accounts,
        data,
    }
}

/// System `InitializeNonceAccount` (tag 6).
pub fn initialize_nonce_account(nonce: &Pubkey, authority: &Pubkey) -> Instruction {
    let mut data = 6u32.to_le_bytes().to_vec();
    data.extend_from_slice(authority.as_ref());
    Instruction {
        program_id: SYSTEM_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*nonce, false),
            AccountMeta::new_readonly(RECENT_BLOCKHASHES_SYSVAR_ID, false),
            AccountMeta::new_readonly(RENT_SYSVAR_ID, false),
        ],
        data,
    }
}

/// System `WithdrawNonceAccount` (tag 5): withdrawing the full balance closes it.
pub fn withdraw_nonce_account(
    nonce: &Pubkey,
    authority: &Pubkey,
    to: &Pubkey,
    lamports: u64,
) -> Instruction {
    let mut data = 5u32.to_le_bytes().to_vec();
    data.extend_from_slice(&lamports.to_le_bytes());
    Instruction {
        program_id: SYSTEM_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*nonce, false),
            AccountMeta::new(*to, false),
            AccountMeta::new_readonly(RECENT_BLOCKHASHES_SYSVAR_ID, false),
            AccountMeta::new_readonly(RENT_SYSVAR_ID, false),
            AccountMeta::new_readonly(*authority, true),
        ],
        data,
    }
}

/// Instructions creating nonce `i` of `pool` owned by `authority` (payer = authority).
pub fn nonce_init_instructions(
    authority: &Pubkey,
    pool: &str,
    i: usize,
    lamports: u64,
) -> (Pubkey, [Instruction; 2]) {
    let seed = nonce_seed(pool, i);
    let addr = create_with_seed(authority, &seed, &SYSTEM_PROGRAM_ID);
    (
        addr,
        [
            create_account_with_seed(
                authority,
                &addr,
                authority,
                &seed,
                lamports,
                NONCE_ACCOUNT_LEN,
                &SYSTEM_PROGRAM_ID,
            ),
            initialize_nonce_account(&addr, authority),
        ],
    )
}

/// Every nonce address of a pool.
pub fn pool_addresses(authority: &Pubkey, pool: &str, n: usize) -> Vec<Pubkey> {
    (0..n)
        .map(|i| create_with_seed(authority, &nonce_seed(pool, i), &SYSTEM_PROGRAM_ID))
        .collect()
}

/// The keeper's nonce burn (docs/06 section 7.3): a legacy transaction whose
/// only instruction is `AdvanceNonceAccount`, signed by the authority (fee
/// payer), with the nonce's current value as its lifetime. Once it lands the
/// value changes and every pending buy variant on the old value is dead.
/// Returns the signed transaction and its signature.
pub fn burn_tx(
    authority: &crate::signer::HotSigner,
    nonce: &Pubkey,
    value: &[u8; 32],
) -> Result<(Vec<u8>, [u8; 64]), crate::msg::CompileError> {
    use crate::msg::{compile, Format, KeySpec};
    let auth = authority.pubkey();
    let keys = [
        KeySpec::new(auth, true, true),
        KeySpec::new(*nonce, false, true),
        KeySpec::new(RECENT_BLOCKHASHES_SYSVAR_ID, false, false),
        KeySpec::new(SYSTEM_PROGRAM_ID, false, false),
    ];
    let ix = pump_codec::instructions::advance_nonce_account(nonce, &auth);
    let (mut bytes, layout) = compile(&keys, &[ix], value, Format::Legacy)?;
    let sig = authority.sign(&bytes[layout.message.0..layout.message.1]);
    bytes[layout.sigs..layout.sigs + 64].copy_from_slice(&sig);
    Ok((bytes, sig))
}

/// A signed legacy transaction creating nonce `i` of `pool` for
/// `rugsnipe-ctl nonce create`: [`nonce_init_instructions`]
/// (`CreateAccountWithSeed` with the signer as payer and base, `lamports`,
/// 80 bytes, owner System; then `InitializeNonceAccount` with the signer as
/// authority), `blockhash` as its lifetime, one signature (the signer, fee
/// payer). Returns the transaction, its signature and the nonce address.
pub fn nonce_create_tx(
    authority: &crate::signer::HotSigner,
    pool: &str,
    i: usize,
    lamports: u64,
    blockhash: &[u8; 32],
) -> Result<(Vec<u8>, [u8; 64], Pubkey), crate::msg::CompileError> {
    use crate::msg::{compile, Format, KeySpec};
    let auth = authority.pubkey();
    let (addr, ixs) = nonce_init_instructions(&auth, pool, i, lamports);
    let keys = [
        KeySpec::new(auth, true, true),
        KeySpec::new(addr, false, true),
        KeySpec::new(SYSTEM_PROGRAM_ID, false, false),
        KeySpec::new(RECENT_BLOCKHASHES_SYSVAR_ID, false, false),
        KeySpec::new(RENT_SYSVAR_ID, false, false),
    ];
    let (mut bytes, layout) = compile(&keys, &ixs, blockhash, Format::Legacy)?;
    let sig = authority.sign(&bytes[layout.message.0..layout.message.1]);
    bytes[layout.sigs..layout.sigs + 64].copy_from_slice(&sig);
    Ok((bytes, sig, addr))
}

#[cfg(test)]
mod burn_tests {
    use super::*;
    use zeroize::Zeroizing;

    #[test]
    fn burn_is_one_signed_advance_on_the_nonce_value() {
        let s = crate::signer::HotSigner::from_seed(&Zeroizing::new([5u8; 32]));
        let nonce = pool_addresses(&s.pubkey(), "buy", 1)[0];
        let value = [9u8; 32];
        let (tx, sig) = burn_tx(&s, &nonce, &value).unwrap();
        assert_eq!(tx[0], 1, "one signature");
        assert_eq!(&tx[1..65], &sig);
        // header 1 signer, 0 ro signed, 2 ro unsigned; 4 keys
        assert_eq!(&tx[65..69], &[1, 0, 2, 4]);
        let keys_end = 69 + 4 * 32;
        assert_eq!(&tx[69..101], s.pubkey().as_ref());
        assert_eq!(&tx[101..133], nonce.as_ref());
        assert_eq!(
            &tx[keys_end..keys_end + 32],
            &value,
            "lifetime = nonce value"
        );
        assert!(crate::signer::verify(&s.pubkey(), &tx[65..], &sig));
        // one instruction: system program (key 3), accounts [1, 2, 0], tag 4
        assert_eq!(&tx[keys_end + 32..], &[1, 3, 3, 1, 2, 0, 4, 4, 0, 0, 0]);
    }

    #[test]
    fn nonce_create_tx_is_solanas_create_nonce_account_with_seed_signed() {
        use solana_system_interface::instruction as si;
        let s = crate::signer::HotSigner::from_seed(&Zeroizing::new([7u8; 32]));
        let auth = s.pubkey();
        let hash = [3u8; 32];
        let (tx, sig, addr) = nonce_create_tx(&s, "buy", 2, NONCE_RENT_LAMPORTS, &hash).unwrap();
        assert_eq!(addr, pool_addresses(&auth, "buy", 3)[2]);
        assert_eq!(
            addr,
            create_with_seed(&auth, "rs-buy-2", &SYSTEM_PROGRAM_ID)
        );
        assert_eq!(tx[0], 1, "one signature");
        assert_eq!(&tx[1..65], &sig);
        assert!(crate::signer::verify(&auth, &tx[65..], &sig));
        // the message is byte-identical to Solana's own compiler on the
        // system interface's create_nonce_account_with_seed
        fn k<P: From<[u8; 32]>>(p: &Pubkey) -> P {
            P::from(p.to_bytes())
        }
        let ixs = si::create_nonce_account_with_seed(
            &k(&auth),
            &k(&addr),
            &k(&auth),
            "rs-buy-2",
            &k(&auth),
            NONCE_RENT_LAMPORTS,
        );
        let m = solana_message::legacy::Message::new_with_blockhash(
            &ixs,
            Some(&k(&auth)),
            &solana_hash::Hash::new_from_array(hash),
        );
        assert_eq!(&tx[65..], &m.serialize()[..]);
    }
}
