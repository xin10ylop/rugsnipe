//! Our byte-level builders and PDAs against the official `pump-rust-client`
//! 0.4.1 (dev-dependency oracle, `default-features = false`) and the Solana
//! interface crates.

#![allow(deprecated)] // pump-rust-client marks the legacy builders deprecated

mod common;

use common::pk;
use pump_codec::constants as c;
use pump_codec::instructions as ixs;
use pump_codec::{pda, Pubkey};
use pump_rust_client::PumpSdk;

/// Convert to the pump-rust-client pubkey type (solana-pubkey 3.x), inferred.
fn p<P: From<[u8; 32]>>(k: &Pubkey) -> P {
    P::from(k.to_bytes())
}

/// Flatten any Instruction-shaped value (ours or the SDK's: different crate
/// versions, same fields) into comparable bytes.
macro_rules! flat {
    ($ix:expr) => {{
        let ix = &$ix;
        (
            ix.program_id.to_bytes(),
            ix.accounts
                .iter()
                .map(|m| (m.pubkey.to_bytes(), m.is_signer, m.is_writable))
                .collect::<Vec<_>>(),
            ix.data.clone(),
        )
    }};
}

/// Mint, user and buyback recipient of the real on-chain `buy_v3` in the
/// `v0_create_v2_buy_v3` fixture, plus that instruction's account keys.
fn real_buy_v3() -> (Pubkey, Pubkey, Pubkey, Vec<Pubkey>) {
    use pump_codec::instructions::{classify, Classified, PumpInstruction, PumpTradeKind};
    let fx = common::TxFixture::load("v0_create_v2_buy_v3");
    let tx = fx.view();
    let full = fx.full_keys();
    for ix in tx.instructions() {
        let a = tx.instruction_accounts(&ix, Some(&fx.loaded));
        if let Classified::Pump(PumpInstruction::Trade(t)) =
            classify(tx.program_id(&ix), ix.data, &a).unwrap()
        {
            if t.kind == PumpTradeKind::BuyV3 {
                let keys = ix.accounts.iter().map(|&i| full[i as usize]).collect();
                return (
                    t.mint.unwrap(),
                    t.user.unwrap(),
                    t.buyback_fee_recipient.unwrap(),
                    keys,
                );
            }
        }
    }
    panic!("no buy_v3 in fixture")
}

#[test]
fn buy_v3_matches_pump_rust_client_for_a_real_mint() {
    let (mint, user, real_bb, onchain) = real_buy_v3();
    assert_eq!(mint, pk("CzBigdLBY9C261pL2fgJ2icAeZcuZFDnivk8FB1mxzCX"));
    // SDK and our builder agree with each other and with the chain.
    let sdk_onchain = PumpSdk.buy_v3_instruction(
        p(&mint),
        p(&c::WSOL_MINT),
        p(&c::TOKEN_2022_PROGRAM_ID),
        p(&c::SPL_TOKEN_PROGRAM_ID),
        p(&user),
        p(&real_bb),
        1,
        1,
    );
    let sdk_keys: Vec<[u8; 32]> = sdk_onchain
        .accounts
        .iter()
        .map(|m| m.pubkey.to_bytes())
        .collect();
    let chain_keys: Vec<[u8; 32]> = onchain.iter().map(|k| k.to_bytes()).collect();
    assert_eq!(
        sdk_keys, chain_keys,
        "pump-rust-client buy_v3 accounts == on-chain buy_v3 accounts"
    );
    let bb = c::BUYBACK_FEE_RECIPIENTS[0];
    let keys = ixs::SolV3Keys::derive(mint, user, c::TOKEN_2022_PROGRAM_ID, bb);
    let sdk = PumpSdk;
    // The SDK always sends partial_fill = [true].
    let theirs = sdk.buy_v3_instruction(
        p(&mint),
        p(&c::WSOL_MINT),
        p(&c::TOKEN_2022_PROGRAM_ID),
        p(&c::SPL_TOKEN_PROGRAM_ID),
        p(&user),
        p(&bb),
        1_000_000,
        10_000_000,
    );
    let ours = ixs::buy_v3(&keys, 1_000_000, 10_000_000, Some(true));
    assert_eq!(flat!(ours), flat!(theirs));
    // and the documented bytes
    let hex: String = ours.data.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        hex,
        "07051dc4f517655040420f00000000008096980000000000".to_string() + "01"
    );
    // default quote mint (Pubkey::default) resolves to wSOL in the SDK too
    let theirs_default = sdk.buy_v3_instruction(
        p(&mint),
        p(&Pubkey::default()),
        p(&c::TOKEN_2022_PROGRAM_ID),
        p(&c::SPL_TOKEN_PROGRAM_ID),
        p(&user),
        p(&bb),
        1_000_000,
        10_000_000,
    );
    assert_eq!(flat!(ours), flat!(theirs_default));

    let theirs = sdk.buy_exact_quote_in_v3_instruction(
        p(&mint),
        p(&c::WSOL_MINT),
        p(&c::TOKEN_2022_PROGRAM_ID),
        p(&c::SPL_TOKEN_PROGRAM_ID),
        p(&user),
        p(&bb),
        500_000_000,
        1,
    );
    assert_eq!(
        flat!(ixs::buy_exact_quote_in_v3(
            &keys,
            500_000_000,
            1,
            Some(true)
        )),
        flat!(theirs)
    );

    let theirs = sdk.sell_v3_instruction(
        p(&mint),
        p(&c::WSOL_MINT),
        p(&c::TOKEN_2022_PROGRAM_ID),
        p(&c::SPL_TOKEN_PROGRAM_ID),
        p(&user),
        p(&bb),
        123,
        456,
    );
    assert_eq!(flat!(ixs::sell_v3(&keys, 123, 456)), flat!(theirs));

    // SDK's prelude: idempotent base ATA create, then the trade
    let theirs = sdk.buy_v3_instructions(
        p(&mint),
        p(&c::WSOL_MINT),
        p(&c::TOKEN_2022_PROGRAM_ID),
        p(&c::SPL_TOKEN_PROGRAM_ID),
        p(&user),
        p(&bb),
        7,
        8,
    );
    let ours = ixs::buy_v3_with_ata(&keys, 7, 8, Some(true));
    assert_eq!(theirs.len(), 2);
    assert_eq!(flat!(ours[0]), flat!(theirs[0]));
    assert_eq!(flat!(ours[1]), flat!(theirs[1]));
}

#[test]
fn spl_token_coin_v3_and_legacy_builders_match_the_sdk() {
    let mint = Pubkey::new_unique();
    let user = Pubkey::new_unique();
    let creator = Pubkey::new_unique();
    let fr = c::FEE_RECIPIENTS[3];
    let bb = c::BUYBACK_FEE_RECIPIENTS[5];
    let sdk = PumpSdk;
    // legacy-create coin (SPL Token base) on v3
    let keys = ixs::SolV3Keys::derive(mint, user, c::SPL_TOKEN_PROGRAM_ID, bb);
    let theirs = sdk.buy_v3_instruction(
        p(&mint),
        p(&c::WSOL_MINT),
        p(&c::SPL_TOKEN_PROGRAM_ID),
        p(&c::SPL_TOKEN_PROGRAM_ID),
        p(&user),
        p(&bb),
        1,
        2,
    );
    assert_eq!(flat!(ixs::buy_v3(&keys, 1, 2, Some(true))), flat!(theirs));

    for tp in [c::TOKEN_2022_PROGRAM_ID, c::SPL_TOKEN_PROGRAM_ID] {
        let k = ixs::LegacyTradeKeys::derive(mint, user, creator, tp, fr, bb);
        // SDK legacy buy sends track_volume = [true], partial_fill = [true]
        let theirs = sdk.buy_instruction(
            p(&mint),
            p(&user),
            p(&creator),
            p(&fr),
            p(&bb),
            11,
            22,
            p(&tp),
        );
        assert_eq!(
            flat!(ixs::legacy_buy(&k, 11, 22, Some(true), Some(true)).unwrap()),
            flat!(theirs)
        );
        for cashback in [false, true] {
            let theirs = sdk.sell_instruction(
                p(&mint),
                p(&user),
                p(&creator),
                p(&fr),
                p(&bb),
                33,
                44,
                p(&tp),
                cashback,
            );
            assert_eq!(
                flat!(ixs::legacy_sell(&k, 33, 44, cashback)),
                flat!(theirs),
                "cashback={cashback}"
            );
        }
    }
}

#[test]
fn pdas_match_pump_rust_client() {
    use pump_rust_client::pda as sdk;
    for _ in 0..8 {
        let mint = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        assert_eq!(
            pda::bonding_curve(&mint).to_bytes(),
            sdk::pump::bonding_curve(&p(&mint)).0.to_bytes()
        );
        assert_eq!(
            pda::bonding_curve_v2(&mint).to_bytes(),
            sdk::pump::bonding_curve_v2(&p(&mint)).0.to_bytes()
        );
        assert_eq!(
            pda::creator_vault(&user).to_bytes(),
            sdk::pump::creator_vault(&p(&user)).0.to_bytes()
        );
        assert_eq!(
            pda::user_volume_accumulator(&user).to_bytes(),
            sdk::pump::user_volume_accumulator(&p(&user)).0.to_bytes()
        );
        assert_eq!(
            pda::pool_authority(&mint).to_bytes(),
            sdk::pump::pool_authority(&p(&mint)).0.to_bytes()
        );
        assert_eq!(
            pda::fee_sharing_config(&mint).to_bytes(),
            sdk::pump::sharing_config(&p(&mint)).0.to_bytes()
        );
        assert_eq!(
            pda::mayhem_state(&mint).to_bytes(),
            sdk::mayhem::mayhem_state(&p(&mint)).0.to_bytes()
        );
        assert_eq!(
            pda::mayhem_token_vault(&mint).to_bytes(),
            sdk::mayhem::mayhem_token_vault(&p(&mint)).0.to_bytes()
        );
        assert_eq!(
            pda::metadata(&mint).to_bytes(),
            sdk::pump::metadata(&p(&mint)).0.to_bytes()
        );
        assert_eq!(
            pda::canonical_pump_swap_pool(&mint, &c::WSOL_MINT).to_bytes(),
            sdk::pump_amm::canonical_pool(&p(&mint), &p(&c::WSOL_MINT))
                .0
                .to_bytes()
        );
        assert_eq!(
            pda::pump_swap_creator_vault_authority(&user).to_bytes(),
            sdk::pump_amm::coin_creator_vault_authority(&p(&user))
                .0
                .to_bytes()
        );
        assert_eq!(
            pda::pump_swap_user_volume_accumulator(&user).to_bytes(),
            sdk::pump_amm::user_volume_accumulator(&p(&user))
                .0
                .to_bytes()
        );
        for tp in [c::TOKEN_2022_PROGRAM_ID, c::SPL_TOKEN_PROGRAM_ID] {
            assert_eq!(
                pda::associated_token_address(&user, &mint, &tp).to_bytes(),
                sdk::associated_token(&p(&user), &p(&tp), &p(&mint))
                    .0
                    .to_bytes()
            );
        }
    }
    assert_eq!(
        pda::quote_control().to_bytes(),
        sdk::pump::quote_control().0.to_bytes()
    );
    assert_eq!(
        pda::pump_swap_global_config().to_bytes(),
        sdk::pump_amm::global_config().0.to_bytes()
    );
    assert_eq!(
        pda::pump_swap_event_authority().to_bytes(),
        sdk::pump_amm::event_authority().0.to_bytes()
    );
    assert_eq!(
        pda::pump_swap_fee_config().to_bytes(),
        sdk::pump_amm::fee_config().0.to_bytes()
    );
    assert_eq!(
        pda::mayhem_global_params().to_bytes(),
        sdk::mayhem::global_params().0.to_bytes()
    );
    assert_eq!(
        pda::mayhem_sol_vault().to_bytes(),
        sdk::mayhem::sol_vault().0.to_bytes()
    );
    assert_eq!(
        c::PUMP_MAINNET_ALT.to_bytes(),
        pump_rust_client::constants::MAINNET_ALT.to_bytes()
    );
    assert_eq!(
        c::PUMP_FEES_PROGRAM_ID.to_bytes(),
        pump_rust_client::constants::FEE_PROGRAM_ID.to_bytes()
    );
    assert_eq!(
        c::MAYHEM_PROGRAM_ID.to_bytes(),
        pump_rust_client::constants::MAYHEM_PROGRAM_ID.to_bytes()
    );
    assert_eq!(
        c::PUMP_PROGRAM_ID.to_bytes(),
        pump_rust_client::constants::pump::PROGRAM_ID.to_bytes()
    );
    assert_eq!(
        c::PUMP_SWAP_PROGRAM_ID.to_bytes(),
        pump_rust_client::constants::pump_amm::PROGRAM_ID.to_bytes()
    );
    assert_eq!(
        c::TOKEN_2022_PROGRAM_ID.to_bytes(),
        pump_rust_client::constants::SPL_TOKEN_2022_PROGRAM_ID.to_bytes()
    );
    assert_eq!(
        c::SPL_TOKEN_PROGRAM_ID.to_bytes(),
        pump_rust_client::constants::SPL_TOKEN_PROGRAM_ID.to_bytes()
    );
    assert_eq!(
        c::ASSOCIATED_TOKEN_PROGRAM_ID.to_bytes(),
        pump_rust_client::constants::SPL_ATA_PROGRAM_ID.to_bytes()
    );
    assert_eq!(
        c::WSOL_MINT.to_bytes(),
        pump_rust_client::constants::NATIVE_MINT.to_bytes()
    );
    assert_eq!(
        c::MPL_TOKEN_METADATA_PROGRAM_ID.to_bytes(),
        pump_rust_client::constants::MPL_TOKEN_METADATA_PROGRAM_ID.to_bytes()
    );
    assert_eq!(
        c::RENT_SYSVAR_ID.to_bytes(),
        pump_rust_client::constants::RENT_SYSVAR_ID.to_bytes()
    );
    assert_eq!(
        c::USDC_MINT.to_bytes(),
        pump_rust_client::math::fees::USDC_MINT.to_bytes()
    );
    for k in [
        c::GLOBAL,
        c::MINT_AUTHORITY,
        c::EVENT_AUTHORITY,
        c::GLOBAL_VOLUME_ACCUMULATOR,
        c::FEE_CONFIG,
    ] {
        let known = [
            sdk::pump::global().0.to_bytes(),
            sdk::pump::mint_authority().0.to_bytes(),
            sdk::pump::event_authority().0.to_bytes(),
            sdk::pump::global_volume_accumulator().0.to_bytes(),
            sdk::pump::fee_config().0.to_bytes(),
        ];
        assert!(known.contains(&k.to_bytes()), "{k}");
    }
}

#[test]
fn generic_solana_builders_match_interface_crates() {
    let a = Pubkey::new_unique();
    let b = Pubkey::new_unique();
    let mint = Pubkey::new_unique();
    for tp in [c::TOKEN_2022_PROGRAM_ID, c::SPL_TOKEN_PROGRAM_ID] {
        let theirs = spl_associated_token_account_interface::instruction::create_associated_token_account_idempotent(&a, &b, &mint, &tp);
        assert_eq!(
            flat!(ixs::create_associated_token_account_idempotent(
                &a, &b, &mint, &tp
            )),
            flat!(theirs)
        );
        assert_eq!(
            pda::associated_token_address(&b, &mint, &tp),
            spl_associated_token_account_interface::address::get_associated_token_address_with_program_id(&b, &mint, &tp)
        );
    }
    use solana_compute_budget_interface::ComputeBudgetInstruction as Cb;
    assert_eq!(
        flat!(ixs::set_compute_unit_limit(100_000)),
        flat!(Cb::set_compute_unit_limit(100_000))
    );
    assert_eq!(
        flat!(ixs::set_compute_unit_price(1_234_567)),
        flat!(Cb::set_compute_unit_price(1_234_567))
    );
    assert_eq!(
        flat!(ixs::set_loaded_accounts_data_size_limit(65_536)),
        flat!(Cb::set_loaded_accounts_data_size_limit(65_536))
    );
    assert_eq!(
        flat!(ixs::system_transfer(&a, &b, 15_000_000)),
        flat!(solana_system_interface::instruction::transfer(
            &a, &b, 15_000_000
        ))
    );
    assert_eq!(
        flat!(ixs::advance_nonce_account(&a, &b)),
        flat!(solana_system_interface::instruction::advance_nonce_account(
            &a, &b
        ))
    );
    assert_eq!(
        ixs::jito_tip(&a, 9, 1).accounts[1].pubkey,
        c::JITO_TIP_ACCOUNTS[1]
    );
}

#[test]
fn jito_tip_accounts_match_block_engine_fixture() {
    let v = common::read_json("api/jito_tip_accounts.json");
    let mut api: Vec<String> = v["result"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    let mut ours: Vec<String> = c::JITO_TIP_ACCOUNTS.iter().map(|k| k.to_string()).collect();
    api.sort();
    ours.sort();
    assert_eq!(api, ours);
}
