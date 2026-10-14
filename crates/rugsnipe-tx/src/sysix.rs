//! System-program instructions for durable-nonce account management
//! (`rugsnipe-ctl nonce init/close`). Nonce accounts are created **with a
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
pub fn create_account_with_seed(from: &Pubkey, to: &Pubkey, base: &Pubkey, seed: &str, lamports: u64, space: u64, owner: &Pubkey) -> Instruction {
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
    Instruction { program_id: SYSTEM_PROGRAM_ID, accounts, data }
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
pub fn withdraw_nonce_account(nonce: &Pubkey, authority: &Pubkey, to: &Pubkey, lamports: u64) -> Instruction {
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
pub fn nonce_init_instructions(authority: &Pubkey, pool: &str, i: usize, lamports: u64) -> (Pubkey, [Instruction; 2]) {
    let seed = nonce_seed(pool, i);
    let addr = create_with_seed(authority, &seed, &SYSTEM_PROGRAM_ID);
    (
        addr,
        [
            create_account_with_seed(authority, &addr, authority, &seed, lamports, NONCE_ACCOUNT_LEN, &SYSTEM_PROGRAM_ID),
            initialize_nonce_account(&addr, authority),
        ],
    )
}

/// Every nonce address of a pool.
pub fn pool_addresses(authority: &Pubkey, pool: &str, n: usize) -> Vec<Pubkey> {
    (0..n).map(|i| create_with_seed(authority, &nonce_seed(pool, i), &SYSTEM_PROGRAM_ID)).collect()
}
