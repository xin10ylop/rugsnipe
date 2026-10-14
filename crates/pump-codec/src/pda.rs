//! Program-derived addresses for pump, PumpSwap, Pump Fees, mayhem and ATAs.
//!
//! Each `find_*`/plain function returns the canonical (highest-bump) PDA via
//! `Pubkey::find_program_address`, which costs one or more SHA-256 rounds
//! plus an ed25519 off-curve check (tens of microseconds). In the hot path,
//! derive once per mint / per wallet and cache (see
//! [`instructions::SolV3Keys`](crate::instructions::SolV3Keys)).
//! Fixed PDAs are hard-coded in [`constants`](crate::constants) and verified here.

use crate::constants::{self as c, seeds};
use solana_pubkey::Pubkey;

#[inline]
fn find(seeds: &[&[u8]], program: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(seeds, program).0
}

// ------------------------------------------------------------------ pump
/// `["global"]` (pump). Equals [`c::GLOBAL`].
pub fn global() -> Pubkey {
    find(&[seeds::GLOBAL], &c::PUMP_PROGRAM_ID)
}
/// `["mint-authority"]` (pump). Equals [`c::MINT_AUTHORITY`].
pub fn mint_authority() -> Pubkey {
    find(&[seeds::MINT_AUTHORITY], &c::PUMP_PROGRAM_ID)
}
/// `["__event_authority"]` (pump). Equals [`c::EVENT_AUTHORITY`].
pub fn event_authority() -> Pubkey {
    find(&[seeds::EVENT_AUTHORITY], &c::PUMP_PROGRAM_ID)
}
/// `["global_volume_accumulator"]` (pump).
pub fn global_volume_accumulator() -> Pubkey {
    find(&[seeds::GLOBAL_VOLUME_ACCUMULATOR], &c::PUMP_PROGRAM_ID)
}
/// `["bonding-curve", mint]` (pump).
pub fn bonding_curve(mint: &Pubkey) -> Pubkey {
    find(&[seeds::BONDING_CURVE, mint.as_ref()], &c::PUMP_PROGRAM_ID)
}
/// `["bonding-curve", mint]` with its bump.
pub fn bonding_curve_with_bump(mint: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[seeds::BONDING_CURVE, mint.as_ref()], &c::PUMP_PROGRAM_ID)
}
/// `["bonding-curve-v2", mint]` (pump). The undocumented first remaining
/// account of legacy `buy`/`sell`.
pub fn bonding_curve_v2(mint: &Pubkey) -> Pubkey {
    find(
        &[seeds::BONDING_CURVE_V2, mint.as_ref()],
        &c::PUMP_PROGRAM_ID,
    )
}
/// `["creator-vault", creator]` (pump, hyphen). `creator` is `BondingCurve.creator`.
pub fn creator_vault(creator: &Pubkey) -> Pubkey {
    find(
        &[seeds::CREATOR_VAULT, creator.as_ref()],
        &c::PUMP_PROGRAM_ID,
    )
}
/// `["user_volume_accumulator", user]` (pump).
pub fn user_volume_accumulator(user: &Pubkey) -> Pubkey {
    find(
        &[seeds::USER_VOLUME_ACCUMULATOR, user.as_ref()],
        &c::PUMP_PROGRAM_ID,
    )
}
/// `["user_volume_accumulator", user]` (pump) with its bump.
pub fn user_volume_accumulator_with_bump(user: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[seeds::USER_VOLUME_ACCUMULATOR, user.as_ref()],
        &c::PUMP_PROGRAM_ID,
    )
}
/// `["pool-authority", mint]` (pump): creator of the canonical PumpSwap pool.
pub fn pool_authority(mint: &Pubkey) -> Pubkey {
    find(&[seeds::POOL_AUTHORITY, mint.as_ref()], &c::PUMP_PROGRAM_ID)
}
/// `["quote-control"]` (pump): optional 4th quote-tail account of `create_v2`.
pub fn quote_control() -> Pubkey {
    find(&[seeds::QUOTE_CONTROL], &c::PUMP_PROGRAM_ID)
}
/// Metaplex metadata PDA of a legacy `create` mint.
pub fn metadata(mint: &Pubkey) -> Pubkey {
    find(
        &[
            seeds::METADATA,
            c::MPL_TOKEN_METADATA_PROGRAM_ID.as_ref(),
            mint.as_ref(),
        ],
        &c::MPL_TOKEN_METADATA_PROGRAM_ID,
    )
}

// ------------------------------------------------------------- pump fees
/// `["fee_config", PUMP_PROGRAM_ID]` (fees program). Equals [`c::FEE_CONFIG`].
pub fn fee_config() -> Pubkey {
    find(
        &[seeds::FEE_CONFIG, c::PUMP_PROGRAM_ID.as_ref()],
        &c::PUMP_FEES_PROGRAM_ID,
    )
}
/// `["fee_config", PUMP_SWAP_PROGRAM_ID]` (fees program). Equals [`c::PUMP_SWAP_FEE_CONFIG`].
pub fn pump_swap_fee_config() -> Pubkey {
    find(
        &[seeds::FEE_CONFIG, c::PUMP_SWAP_PROGRAM_ID.as_ref()],
        &c::PUMP_FEES_PROGRAM_ID,
    )
}
/// `["sharing-config", mint]` (fees program): per-mint creator-fee sharing config.
pub fn fee_sharing_config(mint: &Pubkey) -> Pubkey {
    find(
        &[seeds::SHARING_CONFIG, mint.as_ref()],
        &c::PUMP_FEES_PROGRAM_ID,
    )
}

// ---------------------------------------------------------------- mayhem
/// `["global-params"]` (mayhem). Equals [`c::MAYHEM_GLOBAL_PARAMS`].
pub fn mayhem_global_params() -> Pubkey {
    find(&[seeds::MAYHEM_GLOBAL_PARAMS], &c::MAYHEM_PROGRAM_ID)
}
/// `["sol-vault"]` (mayhem). Equals [`c::MAYHEM_SOL_VAULT`].
pub fn mayhem_sol_vault() -> Pubkey {
    find(&[seeds::MAYHEM_SOL_VAULT], &c::MAYHEM_PROGRAM_ID)
}
/// `["mayhem-state", mint]` (mayhem). `create_v2` account #12.
pub fn mayhem_state(mint: &Pubkey) -> Pubkey {
    find(&[seeds::MAYHEM_STATE, mint.as_ref()], &c::MAYHEM_PROGRAM_ID)
}
/// Token-2022 ATA of the mayhem sol-vault for `mint`. `create_v2` account #13.
pub fn mayhem_token_vault(mint: &Pubkey) -> Pubkey {
    associated_token_address(&c::MAYHEM_SOL_VAULT, mint, &c::TOKEN_2022_PROGRAM_ID)
}

// -------------------------------------------------------------- PumpSwap
/// `["global_config"]` (PumpSwap). Equals [`c::PUMP_SWAP_GLOBAL_CONFIG`].
pub fn pump_swap_global_config() -> Pubkey {
    find(&[seeds::AMM_GLOBAL_CONFIG], &c::PUMP_SWAP_PROGRAM_ID)
}
/// `["__event_authority"]` (PumpSwap). Equals [`c::PUMP_SWAP_EVENT_AUTHORITY`].
pub fn pump_swap_event_authority() -> Pubkey {
    find(&[seeds::EVENT_AUTHORITY], &c::PUMP_SWAP_PROGRAM_ID)
}
/// `["pool", index u16 LE, creator, base_mint, quote_mint]` (PumpSwap).
pub fn pump_swap_pool(
    index: u16,
    creator: &Pubkey,
    base_mint: &Pubkey,
    quote_mint: &Pubkey,
) -> Pubkey {
    find(
        &[
            seeds::AMM_POOL,
            &index.to_le_bytes(),
            creator.as_ref(),
            base_mint.as_ref(),
            quote_mint.as_ref(),
        ],
        &c::PUMP_SWAP_PROGRAM_ID,
    )
}
/// The pool pump creates on migration: index 0, creator = `pool_authority(mint)`,
/// quote = wrapped SOL for SOL-paired coins.
pub fn canonical_pump_swap_pool(mint: &Pubkey, quote_mint: &Pubkey) -> Pubkey {
    pump_swap_pool(0, &pool_authority(mint), mint, quote_mint)
}
/// `["creator_vault", coin_creator]` (PumpSwap, underscore).
pub fn pump_swap_creator_vault_authority(coin_creator: &Pubkey) -> Pubkey {
    find(
        &[seeds::AMM_CREATOR_VAULT, coin_creator.as_ref()],
        &c::PUMP_SWAP_PROGRAM_ID,
    )
}
/// `["user_volume_accumulator", user]` (PumpSwap).
pub fn pump_swap_user_volume_accumulator(user: &Pubkey) -> Pubkey {
    find(
        &[seeds::USER_VOLUME_ACCUMULATOR, user.as_ref()],
        &c::PUMP_SWAP_PROGRAM_ID,
    )
}

// ------------------------------------------------------------------ ATAs
/// Associated token account `[owner, token_program, mint]` under the ATA program.
pub fn associated_token_address(owner: &Pubkey, mint: &Pubkey, token_program: &Pubkey) -> Pubkey {
    find(
        &[owner.as_ref(), token_program.as_ref(), mint.as_ref()],
        &c::ASSOCIATED_TOKEN_PROGRAM_ID,
    )
}
/// Token-2022 ATA (every `create_v2` coin).
pub fn ata_token_2022(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    associated_token_address(owner, mint, &c::TOKEN_2022_PROGRAM_ID)
}
/// Legacy SPL Token ATA (legacy `create` coins, wrapped SOL, USDC).
pub fn ata_spl(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    associated_token_address(owner, mint, &c::SPL_TOKEN_PROGRAM_ID)
}
/// The bonding curve's token account for `mint` (`associated_bonding_curve`).
pub fn associated_bonding_curve(mint: &Pubkey, token_program: &Pubkey) -> Pubkey {
    associated_token_address(&bonding_curve(mint), mint, token_program)
}

// ----------------------------------------------------------------- bumps
/// `(address, bump)` of an associated token account.
pub fn associated_token_address_with_bump(
    owner: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[owner.as_ref(), token_program.as_ref(), mint.as_ref()],
        &c::ASSOCIATED_TOKEN_PROGRAM_ID,
    )
}

/// Bump of a **known** PDA by hashing alone: the first bump from 255 down
/// whose `sha256(seeds ‖ bump ‖ program ‖ "ProgramDerivedAddress")` equals
/// `address`. No ed25519 curve check is needed because the address is
/// given: for a canonical PDA (what pump and the ATA program enforce) every
/// higher bump hashed to an on-curve point, so its hash is not `address`.
/// Cost is `256 - bump` SHA-256 rounds (~0.3 us each), against tens of
/// microseconds for a fresh [`Pubkey::find_program_address`]. `None` if no
/// bump matches (the address is not a PDA of these seeds).
pub fn known_pda_bump(seeds: &[&[u8]], program: &Pubkey, address: &Pubkey) -> Option<u8> {
    use sha2::{Digest, Sha256};
    let mut prefix = Sha256::new();
    for s in seeds {
        prefix.update(s);
    }
    for bump in (0..=u8::MAX).rev() {
        let mut h = prefix.clone();
        h.update([bump]);
        h.update(program.as_ref());
        h.update(b"ProgramDerivedAddress");
        if h.finalize().as_slice() == address.as_ref() {
            return Some(bump);
        }
    }
    None
}

/// Bump of a known bonding curve `["bonding-curve", mint]` (hash only).
pub fn bonding_curve_bump(mint: &Pubkey, bonding_curve: &Pubkey) -> Option<u8> {
    known_pda_bump(
        &[seeds::BONDING_CURVE, mint.as_ref()],
        &c::PUMP_PROGRAM_ID,
        bonding_curve,
    )
}

/// Bump of a known ATA `[owner, token_program, mint]` (hash only).
pub fn ata_bump(owner: &Pubkey, mint: &Pubkey, token_program: &Pubkey, ata: &Pubkey) -> Option<u8> {
    known_pda_bump(
        &[owner.as_ref(), token_program.as_ref(), mint.as_ref()],
        &c::ASSOCIATED_TOKEN_PROGRAM_ID,
        ata,
    )
}

/// The four per-mint PDA bumps that set the compute cost of a v3 snipe
/// (docs/research/gap-5.md section 1): the programs re-derive each of these
/// on chain with `find_program_address`, which costs 1,500 CU per missing
/// bump (`255 - bump`), so the snipe costs `base + 1,500 * S` with
/// `S = Σ (255 - bump)` ([`Self::missing`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SnipeBumps {
    /// `bonding_curve` = `["bonding-curve", mint]` (pump).
    pub bc: u8,
    /// `associated_base_bonding_curve` = ATA(bc, base token program, mint).
    pub abc: u8,
    /// `associated_quote_bonding_curve` = ATA(bc, SPL Token, wSOL).
    pub aqbc: u8,
    /// The user's base ATA = ATA(user, base token program, mint).
    pub user_ata: u8,
}

impl SnipeBumps {
    /// `S = Σ (255 - bump)` over the four PDAs (0..=1020).
    #[inline]
    pub fn missing(&self) -> u32 {
        [self.bc, self.abc, self.aqbc, self.user_ata]
            .iter()
            .map(|b| 255 - u32::from(*b))
            .sum()
    }

    /// Reference derivation from scratch (four full PDA searches; slow).
    pub fn derive(mint: &Pubkey, user: &Pubkey, base_token_program: &Pubkey) -> Self {
        let (bc, bc_bump) = bonding_curve_with_bump(mint);
        Self {
            bc: bc_bump,
            abc: associated_token_address_with_bump(&bc, mint, base_token_program).1,
            aqbc: associated_token_address_with_bump(&bc, &c::WSOL_MINT, &c::SPL_TOKEN_PROGRAM_ID)
                .1,
            user_ata: associated_token_address_with_bump(user, mint, base_token_program).1,
        }
    }

    /// Hot-path recovery from keys already in hand (from the create
    /// instruction or derived): `S + 4` SHA-256 rounds in all, no curve
    /// checks. `None` if a key is not the PDA its seeds say (such a buy
    /// would fail the program's seeds constraint anyway).
    pub fn of_keys(
        mint: &Pubkey,
        bonding_curve: &Pubkey,
        associated_base_bonding_curve: &Pubkey,
        associated_quote_bonding_curve: &Pubkey,
        user: &Pubkey,
        user_ata: &Pubkey,
        base_token_program: &Pubkey,
    ) -> Option<Self> {
        Some(Self {
            bc: bonding_curve_bump(mint, bonding_curve)?,
            abc: ata_bump(
                bonding_curve,
                mint,
                base_token_program,
                associated_base_bonding_curve,
            )?,
            aqbc: ata_bump(
                bonding_curve,
                &c::WSOL_MINT,
                &c::SPL_TOKEN_PROGRAM_ID,
                associated_quote_bonding_curve,
            )?,
            user_ata: ata_bump(user, mint, base_token_program, user_ata)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_pdas_match_known_addresses() {
        assert_eq!(
            global().to_string(),
            "4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf"
        );
        assert_eq!(
            mint_authority().to_string(),
            "TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM"
        );
        assert_eq!(
            event_authority().to_string(),
            "Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1"
        );
        assert_eq!(
            fee_config().to_string(),
            "8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt"
        );
        assert_eq!(global(), c::GLOBAL);
        assert_eq!(mint_authority(), c::MINT_AUTHORITY);
        assert_eq!(event_authority(), c::EVENT_AUTHORITY);
        assert_eq!(fee_config(), c::FEE_CONFIG);
        assert_eq!(global_volume_accumulator(), c::GLOBAL_VOLUME_ACCUMULATOR);
        assert_eq!(pump_swap_fee_config(), c::PUMP_SWAP_FEE_CONFIG);
        assert_eq!(pump_swap_global_config(), c::PUMP_SWAP_GLOBAL_CONFIG);
        assert_eq!(pump_swap_event_authority(), c::PUMP_SWAP_EVENT_AUTHORITY);
        assert_eq!(mayhem_global_params(), c::MAYHEM_GLOBAL_PARAMS);
        assert_eq!(mayhem_sol_vault(), c::MAYHEM_SOL_VAULT);
    }

    #[test]
    fn known_bump_equals_find_and_snipe_bumps_agree() {
        let user = Pubkey::new_unique();
        let mut total_missing = 0u32;
        for i in 0..300u32 {
            let mint = Pubkey::new_unique();
            let tp = if i % 3 == 0 {
                c::SPL_TOKEN_PROGRAM_ID
            } else {
                c::TOKEN_2022_PROGRAM_ID
            };
            let (bc, b) = bonding_curve_with_bump(&mint);
            assert_eq!(bonding_curve_bump(&mint, &bc), Some(b));
            let (abc, ab) = associated_token_address_with_bump(&bc, &mint, &tp);
            assert_eq!(abc, associated_token_address(&bc, &mint, &tp));
            assert_eq!(ata_bump(&bc, &mint, &tp, &abc), Some(ab));
            let aqbc = ata_spl(&bc, &c::WSOL_MINT);
            let ua = associated_token_address(&user, &mint, &tp);
            let want = SnipeBumps::derive(&mint, &user, &tp);
            let got = SnipeBumps::of_keys(&mint, &bc, &abc, &aqbc, &user, &ua, &tp).unwrap();
            assert_eq!(got, want);
            assert_eq!(got.bc, b);
            assert_eq!(got.abc, ab);
            total_missing += got.missing();
            // a wrong key is not a PDA of those seeds
            assert_eq!(
                SnipeBumps::of_keys(&mint, &bc, &aqbc, &aqbc, &user, &ua, &tp),
                None
            );
        }
        // S ~ NegBin(4, 1/2): mean 4 per mint
        let mean = total_missing as f64 / 300.0;
        assert!((2.5..6.0).contains(&mean), "mean S {mean}");
        assert_eq!(
            SnipeBumps {
                bc: 255,
                abc: 254,
                aqbc: 250,
                user_ata: 255
            }
            .missing(),
            6
        );
    }
}
