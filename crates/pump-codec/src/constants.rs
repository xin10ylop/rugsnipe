//! Program IDs, well-known addresses, fee recipients and Anchor discriminators.
//!
//! Sources (all checked 2026-10-14):
//! - `pump-fun/pump-public-docs` `idl/pump.json`, `idl/pump_amm.json`,
//!   `idl/pump_fees.json`, `docs/FEE_RECIPIENTS.md`, `docs/instructions/*.md`;
//! - the on-chain `Global` account (fixture `fixtures/accounts/global.json`);
//! - Jito block engine `getTipAccounts`.
//!
//! Every discriminator is re-derived from `sha256("global:<ix>")`,
//! `sha256("event:<Event>")` or `sha256("account:<Account>")` in this
//! module's tests, and every PDA in [`pda`](crate::pda)'s tests.

use solana_pubkey::Pubkey;

const fn pk(s: &str) -> Pubkey {
    Pubkey::from_str_const(s)
}

// ---------------------------------------------------------------- programs
/// pump.fun bonding-curve program.
pub const PUMP_PROGRAM_ID: Pubkey = pk("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P");
/// PumpSwap AMM (where completed curves migrate to).
pub const PUMP_SWAP_PROGRAM_ID: Pubkey = pk("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA");
/// Pump Fees program (fee schedule `FeeConfig`, sharing configs).
pub const PUMP_FEES_PROGRAM_ID: Pubkey = pk("pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ");
/// Mayhem-mode program referenced by `create_v2`.
pub const MAYHEM_PROGRAM_ID: Pubkey = pk("MAyhSmzXzV1pTf7LsNkrNwkWKTo4ougAJ1PPg47MD4e");
/// SPL Token-2022 (all `create_v2` mints).
pub const TOKEN_2022_PROGRAM_ID: Pubkey = pk("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
/// Legacy SPL Token (legacy `create` mints, wrapped SOL, USDC).
pub const SPL_TOKEN_PROGRAM_ID: Pubkey = pk("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
/// Associated Token Account program.
pub const ASSOCIATED_TOKEN_PROGRAM_ID: Pubkey = pk("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
/// System program (the all-zero key).
pub const SYSTEM_PROGRAM_ID: Pubkey = pk("11111111111111111111111111111111");
/// Compute Budget program.
pub const COMPUTE_BUDGET_PROGRAM_ID: Pubkey = pk("ComputeBudget111111111111111111111111111111");
/// Metaplex Token Metadata (legacy `create` only).
pub const MPL_TOKEN_METADATA_PROGRAM_ID: Pubkey = pk("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s");
/// Wrapped SOL mint (`So111...112`), the quote mint passed for SOL-paired coins.
pub const WSOL_MINT: Pubkey = pk("So11111111111111111111111111111111111111112");
/// Token-2022 native mint (counted as SOL-like by the pump fee code).
pub const TOKEN_2022_NATIVE_MINT: Pubkey = pk("9pan9bMn5HatX4EJdBwg9VgCa7Uz5HL8N1m5D3NdXejP");
/// USDC mint (routed to `stable_fee_tiers`).
pub const USDC_MINT: Pubkey = pk("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
/// Rent sysvar (legacy `create`, `migrate`).
pub const RENT_SYSVAR_ID: Pubkey = pk("SysvarRent111111111111111111111111111111111");
/// RecentBlockhashes sysvar (durable-nonce `AdvanceNonceAccount`).
pub const RECENT_BLOCKHASHES_SYSVAR_ID: Pubkey = pk("SysvarRecentB1ockHashes11111111111111111111");
/// pump's mainnet address lookup table (from `pump-rust-client` 0.4.1 `constants.rs`).
pub const PUMP_MAINNET_ALT: Pubkey = pk("Hyif6eWb8x88RVrvjPfabsgRYnwkVnyByEXTVTXbUcyP");

// ------------------------------------------------------ well-known PDAs
/// pump `Global`: `["global"]`.
pub const GLOBAL: Pubkey = pk("4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf");
/// pump mint authority: `["mint-authority"]`. Appears in every create tx.
pub const MINT_AUTHORITY: Pubkey = pk("TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM");
/// pump event authority: `["__event_authority"]` (Anchor `#[event_cpi]`).
pub const EVENT_AUTHORITY: Pubkey = pk("Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1");
/// pump `GlobalVolumeAccumulator`: `["global_volume_accumulator"]`.
pub const GLOBAL_VOLUME_ACCUMULATOR: Pubkey = pk("Hq2wp8uJ9jCPsYgNHex8RtqdvMPfVGoYwjvF1ATiwn2Y");
/// Pump Fees `FeeConfig` for the pump program: `["fee_config", PUMP_PROGRAM_ID]` (fees program).
pub const FEE_CONFIG: Pubkey = pk("8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt");
/// Pump Fees `FeeConfig` for PumpSwap: `["fee_config", PUMP_SWAP_PROGRAM_ID]` (fees program).
pub const PUMP_SWAP_FEE_CONFIG: Pubkey = pk("5PHirr8joyTMp9JMm6nW7hNDVyEYdkzDqazxPD7RaTjx");
/// PumpSwap `GlobalConfig`: `["global_config"]`.
pub const PUMP_SWAP_GLOBAL_CONFIG: Pubkey = pk("ADyA8hdefvWN2dbGGWFotbzWxrAvLW83WG6QCVXvJKqw");
/// PumpSwap event authority: `["__event_authority"]`.
pub const PUMP_SWAP_EVENT_AUTHORITY: Pubkey = pk("GS4CU59F31iL7aR2Q8zVS8DRrcRnXX1yjQ66TqNVQnaR");
/// Mayhem `global-params`: `["global-params"]` (mayhem program). `create_v2` account #10.
pub const MAYHEM_GLOBAL_PARAMS: Pubkey = pk("13ec7XdrjF3h3YcqBTFDSReRcUFwbCnJaAQspM4j6DDJ");
/// Mayhem `sol-vault`: `["sol-vault"]` (mayhem program). `create_v2` account #11.
pub const MAYHEM_SOL_VAULT: Pubkey = pk("BwWK17cbHxwWBKZkUYvzxLcNQ1YVyaFezduWbtm2de6s");

// ------------------------------------------------------- fee recipients
/// The 8 normal protocol fee recipients (non-mayhem coins, legacy/v2 `fee_recipient`).
/// `docs/FEE_RECIPIENTS.md`; equals `Global.fee_recipient` + `Global.fee_recipients[7]`.
pub const FEE_RECIPIENTS: [Pubkey; 8] = [
    pk("62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV"),
    pk("7VtfL8fvgNfhz17qKRMjzQEXgbdpnHHHQRh54R9jP2RJ"),
    pk("7hTckgnGnLQR6sdH7YkqFTAA7VwTfYFaZ6EhEsU3saCX"),
    pk("9rPYyANsfQZw3DnDmKE3YCQF5E8oD89UXoHn9JFEhJUz"),
    pk("AVmoTthdrX6tKt4nDjco2D775W2YK3sDhxPcMmzUAmTY"),
    pk("CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM"),
    pk("FWsW1xNtWscwNmKv6wVsU1iTzRN6wmmk3MjxRP5tT7hz"),
    pk("G5UZAVbAf46s7cKWoyKu8kYTip9DGTpbLZ2qa9Aq69dP"),
];

/// The 8 "reserved" fee recipients, used as `fee_recipient` for mayhem-mode coins.
/// Equals `Global.reserved_fee_recipient` + `Global.reserved_fee_recipients[7]`.
pub const RESERVED_FEE_RECIPIENTS: [Pubkey; 8] = [
    pk("GesfTA3X2arioaHp8bbKdjG9vJtskViWACZoYvxp4twS"),
    pk("4budycTjhs9fD6xw62VBducVTNgMgJJ5BgtKq7mAZwn6"),
    pk("8SBKzEQU4nLSzcwF4a74F2iaUDQyTfjGndn6qUWBnrpR"),
    pk("4UQeTP1T39KZ9Sfxzo3WR5skgsaP6NZa87BAkuazLEKH"),
    pk("8sNeir4QsLsJdYpc9RZacohhK1Y5FLU3nC5LXgYB4aa6"),
    pk("Fh9HmeLNUMVCvejxCtCL2DbYaRyBFVJ5xrWkLnMH6fdk"),
    pk("463MEnMeGyJekNZFQSTUABBEbLnvMTALbT6ZmsxAbAdq"),
    pk("6AUH3WEHucYZyC61hqpqYUWVto5qA5hjHuNQ32GNnNxA"),
];

/// The 8 buyback fee recipients (any coin). For SOL-paired v3 trades pass the
/// wallet itself as `buyback_fee_recipient`. Equals `Global.buyback_fee_recipients`.
pub const BUYBACK_FEE_RECIPIENTS: [Pubkey; 8] = [
    pk("5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD"),
    pk("9M4giFFMxmFGXtc3feFzRai56WbBqehoSeRE5GK7gf7"),
    pk("GXPFM2caqTtQYC2cJ5yJRi9VDkpsYZXzYdwYpGnLmtDL"),
    pk("3BpXnfJaUTiwXnJNe7Ej1rcbzqTTQUvLShZaWazebsVR"),
    pk("5cjcW9wExnJJiqgLjq7DEG75Pm6JBgE1hNv4B2vHXUW6"),
    pk("EHAAiTxcdDwQ3U4bU6YcMsQGaekdzLS3B5SmYo46kJtL"),
    pk("5eHhjP8JaYkz83CWwvGU2uMUXefd3AazWGx4gpcuEEYD"),
    pk("A7hAgCzFw14fejgCp387JUJRMNyz4j89JKnhtKU8piqW"),
];

/// Jito tip accounts (block engine `getTipAccounts`, 2026-10-14, fixture
/// `fixtures/api/jito_tip_accounts.json`).
pub const JITO_TIP_ACCOUNTS: [Pubkey; 8] = [
    pk("96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5"),
    pk("HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe"),
    pk("Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY"),
    pk("ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49"),
    pk("DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh"),
    pk("ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt"),
    pk("DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL"),
    pk("3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT"),
];

// ------------------------------------------------------------ seeds
/// PDA seed strings (pump unless noted). Note the hyphen/underscore mix.
pub mod seeds {
    pub const GLOBAL: &[u8] = b"global";
    pub const MINT_AUTHORITY: &[u8] = b"mint-authority";
    pub const BONDING_CURVE: &[u8] = b"bonding-curve";
    pub const BONDING_CURVE_V2: &[u8] = b"bonding-curve-v2";
    /// pump creator vault (hyphen). PumpSwap's is `creator_vault` (underscore).
    pub const CREATOR_VAULT: &[u8] = b"creator-vault";
    pub const EVENT_AUTHORITY: &[u8] = b"__event_authority";
    pub const POOL_AUTHORITY: &[u8] = b"pool-authority";
    pub const GLOBAL_VOLUME_ACCUMULATOR: &[u8] = b"global_volume_accumulator";
    pub const USER_VOLUME_ACCUMULATOR: &[u8] = b"user_volume_accumulator";
    pub const QUOTE_CONTROL: &[u8] = b"quote-control";
    pub const METADATA: &[u8] = b"metadata";
    /// Pump Fees program.
    pub const FEE_CONFIG: &[u8] = b"fee_config";
    /// Pump Fees program.
    pub const SHARING_CONFIG: &[u8] = b"sharing-config";
    /// Mayhem program.
    pub const MAYHEM_GLOBAL_PARAMS: &[u8] = b"global-params";
    /// Mayhem program.
    pub const MAYHEM_SOL_VAULT: &[u8] = b"sol-vault";
    /// Mayhem program.
    pub const MAYHEM_STATE: &[u8] = b"mayhem-state";
    /// PumpSwap.
    pub const AMM_POOL: &[u8] = b"pool";
    /// PumpSwap.
    pub const AMM_GLOBAL_CONFIG: &[u8] = b"global_config";
    /// PumpSwap (underscore).
    pub const AMM_CREATOR_VAULT: &[u8] = b"creator_vault";
    /// PumpSwap.
    pub const AMM_POOL_V2: &[u8] = b"pool-v2";
}

// --------------------------------------------------- discriminators
/// Anchor event-CPI tag: first 8 bytes of every self-CPI event instruction,
/// followed by the event discriminator. Anchor defines it as the `u64`
/// `0x1d9acb512ea545e4` (= `sha256("anchor:event")[..8]` read big-endian) and
/// writes it little-endian, so on the wire it is the *byte-reversed* hash prefix.
pub const EVENT_IX_TAG: [u8; 8] = hex8(b"e445a52e51cb9a1d");

/// pump instruction discriminators (`sha256("global:<name>")[..8]`).
pub mod ix {
    use super::hex8;
    pub const CREATE: [u8; 8] = hex8(b"181ec828051c0777");
    pub const CREATE_V2: [u8; 8] = hex8(b"d6904cec5f8b31b4");
    pub const BUY: [u8; 8] = hex8(b"66063d1201daebea");
    pub const BUY_EXACT_SOL_IN: [u8; 8] = hex8(b"38fc74089edfcd5f");
    pub const SELL: [u8; 8] = hex8(b"33e685a4017f83ad");
    pub const BUY_V2: [u8; 8] = hex8(b"b817ee6167c5d33d");
    pub const SELL_V2: [u8; 8] = hex8(b"5df6823ce7e940b2");
    pub const BUY_EXACT_QUOTE_IN_V2: [u8; 8] = hex8(b"c2ab1c46684d5b2f");
    pub const BUY_V3: [u8; 8] = hex8(b"07051dc4f5176550");
    pub const SELL_V3: [u8; 8] = hex8(b"1c92de7726c469d5");
    pub const BUY_EXACT_QUOTE_IN_V3: [u8; 8] = hex8(b"e1f7501ed5b38488");
    pub const MIGRATE: [u8; 8] = hex8(b"9beae792ec9ea21e");
    pub const MIGRATE_V2: [u8; 8] = hex8(b"bbcb121fceedfe29");
    pub const EXTEND_ACCOUNT: [u8; 8] = hex8(b"ea66c2cb96483ee5");
    pub const COLLECT_CREATOR_FEE: [u8; 8] = hex8(b"1416567bc61cdb84");
    pub const SWEEP_PROTOCOL_FEE: [u8; 8] = hex8(b"0830be07b644b7e5");
    pub const SWEEP_CREATOR_FEE: [u8; 8] = hex8(b"20f6bf3408c949ba");
    pub const SET_PARAMS: [u8; 8] = hex8(b"1beab2349302bb8d");
    pub const INIT_USER_VOLUME_ACCUMULATOR: [u8; 8] = hex8(b"5e06ca73ff60e8b7");
    pub const MULTI_HOP_CURVE_SWAP: [u8; 8] = hex8(b"e19a7516d756f667");

    /// `(name, discriminator)` for every pump instruction above (IDL names).
    pub const ALL: &[(&str, [u8; 8])] = &[
        ("create", CREATE),
        ("create_v2", CREATE_V2),
        ("buy", BUY),
        ("buy_exact_sol_in", BUY_EXACT_SOL_IN),
        ("sell", SELL),
        ("buy_v2", BUY_V2),
        ("sell_v2", SELL_V2),
        ("buy_exact_quote_in_v2", BUY_EXACT_QUOTE_IN_V2),
        ("buy_v3", BUY_V3),
        ("sell_v3", SELL_V3),
        ("buy_exact_quote_in_v3", BUY_EXACT_QUOTE_IN_V3),
        ("migrate", MIGRATE),
        ("migrate_v2", MIGRATE_V2),
        ("extend_account", EXTEND_ACCOUNT),
        ("collect_creator_fee", COLLECT_CREATOR_FEE),
        ("sweep_protocol_fee", SWEEP_PROTOCOL_FEE),
        ("sweep_creator_fee", SWEEP_CREATOR_FEE),
        ("set_params", SET_PARAMS),
        ("init_user_volume_accumulator", INIT_USER_VOLUME_ACCUMULATOR),
        ("multi_hop_curve_swap", MULTI_HOP_CURVE_SWAP),
    ];
}

/// PumpSwap instruction discriminators. `buy`, `sell`, `buy_v2`, `sell_v2`
/// and `buy_exact_quote_in_v2` are byte-identical to pump's: always key on the
/// program ID first.
pub mod amm_ix {
    use super::hex8;
    pub const BUY: [u8; 8] = hex8(b"66063d1201daebea");
    pub const SELL: [u8; 8] = hex8(b"33e685a4017f83ad");
    pub const BUY_EXACT_QUOTE_IN: [u8; 8] = hex8(b"c62e1552b4d9e870");
    pub const BUY_V2: [u8; 8] = hex8(b"b817ee6167c5d33d");
    pub const SELL_V2: [u8; 8] = hex8(b"5df6823ce7e940b2");
    pub const BUY_EXACT_QUOTE_IN_V2: [u8; 8] = hex8(b"c2ab1c46684d5b2f");
    pub const CREATE_POOL: [u8; 8] = hex8(b"e992d18ecf6840bc");
    pub const MULTI_HOP_SWAP: [u8; 8] = hex8(b"2b644913e9f66f94");

    pub const ALL: &[(&str, [u8; 8])] = &[
        ("buy", BUY),
        ("sell", SELL),
        ("buy_exact_quote_in", BUY_EXACT_QUOTE_IN),
        ("buy_v2", BUY_V2),
        ("sell_v2", SELL_V2),
        ("buy_exact_quote_in_v2", BUY_EXACT_QUOTE_IN_V2),
        ("create_pool", CREATE_POOL),
        ("multi_hop_swap", MULTI_HOP_SWAP),
    ];
}

/// Event discriminators (`sha256("event:<Name>")[..8]`).
pub mod event {
    use super::hex8;
    // pump
    pub const CREATE: [u8; 8] = hex8(b"1b72a94ddeeb6376");
    pub const TRADE: [u8; 8] = hex8(b"bddb7fd34ee661ee");
    pub const COMPLETE: [u8; 8] = hex8(b"5f72619cd42e9808");
    pub const COMPLETE_PUMP_AMM_MIGRATION: [u8; 8] = hex8(b"bde95db95c94ea94");
    pub const POST_COMPLETE_BUY: [u8; 8] = hex8(b"6fb06d8b316cd5fb");
    pub const SET_PARAMS: [u8; 8] = hex8(b"dfc39ff63e308f83");
    pub const EXTEND_ACCOUNT: [u8; 8] = hex8(b"6161d7905d92167c");
    pub const SWEEP_BONDING_CURVE_FEE: [u8; 8] = hex8(b"742b4dbd117a482b");
    // PumpSwap
    pub const AMM_BUY: [u8; 8] = hex8(b"67f4521f2cf57777");
    pub const AMM_SELL: [u8; 8] = hex8(b"3e2f370aa503dc2a");
    pub const AMM_CREATE_POOL: [u8; 8] = hex8(b"b1310cd2a076a774");

    pub const ALL: &[(&str, [u8; 8])] = &[
        ("CreateEvent", CREATE),
        ("TradeEvent", TRADE),
        ("CompleteEvent", COMPLETE),
        ("CompletePumpAmmMigrationEvent", COMPLETE_PUMP_AMM_MIGRATION),
        ("PostCompleteBuyEvent", POST_COMPLETE_BUY),
        ("SetParamsEvent", SET_PARAMS),
        ("ExtendAccountEvent", EXTEND_ACCOUNT),
        ("SweepBondingCurveFeeEvent", SWEEP_BONDING_CURVE_FEE),
        ("BuyEvent", AMM_BUY),
        ("SellEvent", AMM_SELL),
        ("CreatePoolEvent", AMM_CREATE_POOL),
    ];
}

/// Account discriminators (`sha256("account:<Name>")[..8]`).
pub mod account {
    use super::hex8;
    pub const GLOBAL: [u8; 8] = hex8(b"a7e8e8b1c86c727f");
    pub const BONDING_CURVE: [u8; 8] = hex8(b"17b7f83760d8ac60");
    /// Pump Fees `FeeConfig`.
    pub const FEE_CONFIG: [u8; 8] = hex8(b"8f3492bbdb7b4c9b");
    pub const USER_VOLUME_ACCUMULATOR: [u8; 8] = hex8(b"56ff700e66359afa");
    pub const GLOBAL_VOLUME_ACCUMULATOR: [u8; 8] = hex8(b"ca2af62b8ebe1eff");
    /// PumpSwap `Pool`.
    pub const AMM_POOL: [u8; 8] = hex8(b"f19a6d0411b16dbc");
    /// PumpSwap `GlobalConfig`.
    pub const AMM_GLOBAL_CONFIG: [u8; 8] = hex8(b"95089ccaa0fcb0d9");

    pub const ALL: &[(&str, [u8; 8])] = &[
        ("Global", GLOBAL),
        ("BondingCurve", BONDING_CURVE),
        ("FeeConfig", FEE_CONFIG),
        ("UserVolumeAccumulator", USER_VOLUME_ACCUMULATOR),
        ("GlobalVolumeAccumulator", GLOBAL_VOLUME_ACCUMULATOR),
        ("Pool", AMM_POOL),
        ("GlobalConfig", AMM_GLOBAL_CONFIG),
    ];
}

/// Compile-time hex decode of exactly 16 hex chars.
const fn hex8(s: &[u8; 16]) -> [u8; 8] {
    const fn nib(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            _ => panic!("bad hex"),
        }
    }
    let mut out = [0u8; 8];
    let mut i = 0;
    while i < 8 {
        out[i] = (nib(s[2 * i]) << 4) | nib(s[2 * i + 1]);
        i += 1;
    }
    out
}

/// `true` for the quote mints the pump fee code treats as SOL: the zero key
/// (what `BondingCurve.quote_mint` stores for SOL coins), wrapped SOL and the
/// Token-2022 native mint.
#[inline]
pub fn is_sol_like_quote_mint(quote_mint: &Pubkey) -> bool {
    *quote_mint == Pubkey::default()
        || *quote_mint == WSOL_MINT
        || *quote_mint == TOKEN_2022_NATIVE_MINT
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn sighash(prefix: &str, name: &str) -> [u8; 8] {
        let h = Sha256::digest(format!("{prefix}:{name}").as_bytes());
        h[..8].try_into().unwrap()
    }

    #[test]
    fn instruction_discriminators_match_sha256_global() {
        for (name, d) in ix::ALL.iter().chain(amm_ix::ALL.iter()) {
            assert_eq!(sighash("global", name), *d, "global:{name}");
        }
        assert!(ix::ALL.len() >= 11 && amm_ix::ALL.len() >= 7);
    }

    #[test]
    fn event_discriminators_match_sha256_event() {
        for (name, d) in event::ALL {
            assert_eq!(sighash("event", name), *d, "event:{name}");
        }
    }

    #[test]
    fn account_discriminators_match_sha256_account() {
        for (name, d) in account::ALL {
            assert_eq!(sighash("account", name), *d, "account:{name}");
        }
    }

    #[test]
    fn event_cpi_tag_is_sha256_anchor_event_le() {
        let mut h = sighash("anchor", "event");
        assert_eq!(u64::from_be_bytes(h), 0x1d9a_cb51_2ea5_45e4);
        h.reverse();
        assert_eq!(h, EVENT_IX_TAG);
    }

    #[test]
    fn shared_discriminators_between_pump_and_pumpswap() {
        // The reason every classifier keys on program ID first.
        assert_eq!(ix::BUY, amm_ix::BUY);
        assert_eq!(ix::SELL, amm_ix::SELL);
        assert_eq!(ix::BUY_V2, amm_ix::BUY_V2);
        assert_eq!(ix::SELL_V2, amm_ix::SELL_V2);
        assert_eq!(ix::BUY_EXACT_QUOTE_IN_V2, amm_ix::BUY_EXACT_QUOTE_IN_V2);
    }

    #[test]
    fn recipient_sets_are_distinct() {
        let mut all: Vec<Pubkey> = FEE_RECIPIENTS
            .iter()
            .chain(RESERVED_FEE_RECIPIENTS.iter())
            .chain(BUYBACK_FEE_RECIPIENTS.iter())
            .chain(JITO_TIP_ACCOUNTS.iter())
            .copied()
            .collect();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), 32);
    }
}
