//! Account decoders and fee-schedule selection against `pump-rust-client`
//! 0.4.1 (dev-dependency oracle) on real mainnet accounts: pump `Global`,
//! two `BondingCurve`s, pump's flat `FeeConfig` and PumpSwap's 25-tier one.

mod common;

use base64::Engine as _;
use common::*;
use pump_codec::constants as c;
use pump_codec::curve::{BondingCurve, FeeConfig, Fees, Global};
use pump_codec::Pubkey;
use pump_rust_client::accounts as sdk;
use pump_rust_client::math::fees as sdk_fees;

fn account(name: &str) -> Vec<u8> {
    let v = read_json(&format!("accounts/{name}.json"));
    base64::engine::general_purpose::STANDARD
        .decode(v["result"]["value"]["data"][0].as_str().unwrap())
        .unwrap()
}

/// Convert to the SDK's pubkey type (inferred).
fn p<P: From<[u8; 32]>>(k: &Pubkey) -> P {
    P::from(k.to_bytes())
}

macro_rules! same_keys {
    ($ours:expr, $theirs:expr, $($f:ident),+) => {
        $( assert_eq!($ours.$f.to_bytes(), $theirs.$f.to_bytes(), stringify!($f)); )+
    };
}
macro_rules! same_vals {
    ($ours:expr, $theirs:expr, $($f:ident),+) => {
        $( assert_eq!($ours.$f, $theirs.$f, stringify!($f)); )+
    };
}

#[test]
fn account_decoders_agree_with_the_sdk() {
    let raw = account("global");
    let (o, t) = (
        Global::decode(&raw).unwrap(),
        sdk::decode_global(&raw).unwrap(),
    );
    same_vals!(
        o,
        t,
        initialized,
        initial_virtual_token_reserves,
        initial_virtual_sol_reserves,
        initial_real_token_reserves,
        token_total_supply,
        fee_basis_points,
        enable_migrate,
        pool_migration_fee,
        creator_fee_basis_points,
        create_v2_enabled,
        mayhem_mode_enabled,
        is_cashback_enabled,
        buyback_basis_points,
        initial_virtual_quote_reserves,
        creator_fee_configurable,
        max_configurable_creator_fee_bps,
        is_holder_reward_enabled,
        max_curve_depth
    );
    same_keys!(
        o,
        t,
        authority,
        fee_recipient,
        withdraw_authority,
        set_creator_authority,
        admin_set_creator_authority,
        whitelist_pda,
        reserved_fee_recipient,
        holder_reward_claim_authority
    );
    let keys = |v: &[Pubkey]| v.iter().map(|k| k.to_bytes()).collect::<Vec<_>>();
    let sdk_keys = |v: &[_]| v.iter().map(|k: &_| p_bytes(k)).collect::<Vec<_>>();
    assert_eq!(keys(&o.fee_recipients), sdk_keys(&t.fee_recipients));
    assert_eq!(
        keys(&o.reserved_fee_recipients),
        sdk_keys(&t.reserved_fee_recipients)
    );
    assert_eq!(
        keys(&o.buyback_fee_recipients),
        sdk_keys(&t.buyback_fee_recipients)
    );
    assert_eq!(
        keys(&o.whitelisted_quote_mints),
        sdk_keys(&t.whitelisted_quote_mints)
    );
    // and the hard-coded recipient sets equal the on-chain Global
    assert_eq!(o.all_fee_recipients(), c::FEE_RECIPIENTS);
    assert_eq!(o.all_reserved_fee_recipients(), c::RESERVED_FEE_RECIPIENTS);
    assert_eq!(o.buyback_fee_recipients, c::BUYBACK_FEE_RECIPIENTS);

    for name in ["bonding_curve_complete", "bonding_curve_worked"] {
        let raw = account(name);
        let (o, t) = (
            BondingCurve::decode(&raw).unwrap(),
            sdk::decode_bonding_curve(&raw).unwrap(),
        );
        same_vals!(
            o,
            t,
            virtual_token_reserves,
            virtual_quote_reserves,
            real_token_reserves,
            real_quote_reserves,
            token_total_supply,
            complete,
            is_mayhem_mode,
            is_cashback_coin,
            creator_fee_bps,
            can_edit_creator_fee,
            is_holder_reward,
            creator_fee,
            protocol_fees,
            depth,
            initial_virtual_quote_reserves,
            post_complete_base_out,
            post_complete_quote_in
        );
        same_keys!(o, t, creator, quote_mint);
    }

    for name in ["fee_config", "pump_swap_fee_config"] {
        let raw = account(name);
        let (o, t) = (
            FeeConfig::decode(&raw).unwrap(),
            sdk::decode_fee_config(&raw).unwrap(),
        );
        assert_eq!(o.bump, t.bump);
        assert_eq!(o.admin.to_bytes(), t.admin.to_bytes());
        let f = |x: &Fees| (x.lp_fee_bps, x.protocol_fee_bps, x.creator_fee_bps);
        let g = |x: &pump_rust_client::pump::types::Fees| {
            (x.lp_fee_bps, x.protocol_fee_bps, x.creator_fee_bps)
        };
        assert_eq!(f(&o.flat_fees), g(&t.flat_fees), "{name}");
        assert_eq!(f(&o.exotic_flat_fees), g(&t.exotic_flat_fees), "{name}");
        for (ours, theirs) in [
            (&o.fee_tiers, &t.fee_tiers),
            (&o.stable_fee_tiers, &t.stable_fee_tiers),
        ] {
            assert_eq!(ours.len(), theirs.len(), "{name}");
            for (a, b) in ours.iter().zip(theirs.iter()) {
                assert_eq!(
                    a.market_cap_lamports_threshold,
                    b.market_cap_lamports_threshold
                );
                assert_eq!(f(&a.fees), g(&b.fees), "{name}");
            }
        }
    }
    // PumpSwap's schedule really is the tiered one (25 tiers).
    let amm = FeeConfig::decode(&account("pump_swap_fee_config")).unwrap();
    assert_eq!(amm.fee_tiers.len(), 25);
}

fn p_bytes<K: AsRef<[u8]>>(k: &K) -> [u8; 32] {
    k.as_ref().try_into().unwrap()
}

#[test]
fn fee_selection_matches_the_sdk_on_real_schedules() {
    let mut global = sdk::decode_global(&account("global")).unwrap();
    let other = Pubkey::new_from_array([0x5a; 32]);
    let quotes = [
        Pubkey::default(),
        c::WSOL_MINT,
        c::TOKEN_2022_NATIVE_MINT,
        c::USDC_MINT,
        other,
    ];
    let mut compared = 0;
    for name in ["fee_config", "pump_swap_fee_config"] {
        let raw = account(name);
        let ours = FeeConfig::decode(&raw).unwrap();
        let theirs = sdk::decode_fee_config(&raw).unwrap();
        // every tier boundary, its neighbours, and the extremes (market caps
        // are passed to the SDK as vq * supply / vt with supply = vt = 1)
        let mut caps = vec![0u128, 1, u64::MAX as u128];
        for t in ours.fee_tiers.iter().chain(&ours.stable_fee_tiers) {
            let m = t.market_cap_lamports_threshold;
            caps.extend([m.saturating_sub(1), m, m + 1]);
        }
        caps.retain(|m| *m <= u64::MAX as u128);
        for &m in &caps {
            for q in &quotes {
                for coin_bps in [0u64, 300] {
                    for gate in [false, true] {
                        global.creator_fee_configurable = gate;
                        let sdk_res = sdk_fees::compute_bonding_curve_fee_bps(
                            &global,
                            &theirs,
                            &p(q),
                            coin_bps,
                            1,
                            m as u64,
                            1,
                        );
                        let our_res = ours.curve_fees_for_coin(q, m, coin_bps, gate);
                        match (our_res, sdk_res) {
                            (Some(o), Ok(t)) => {
                                assert_eq!(
                                    (o.protocol, o.creator),
                                    (t.protocol_fee_bps, t.creator_fee_bps),
                                    "{name} cap {m} quote {q} bps {coin_bps} gate {gate}"
                                );
                            }
                            (None, Err(_)) => {}
                            (o, t) => panic!("{name} cap {m} quote {q}: {o:?} vs {t:?}"),
                        }
                        compared += 1;
                    }
                }
            }
            // PumpSwap canonical-pool tier (all three components) for SOL quotes
            let sdk_tier =
                sdk_fees::fees_for_quote_mint(&theirs, true, m, &p(&c::WSOL_MINT)).unwrap();
            let our_tier = FeeConfig::tier_for(&ours.fee_tiers, m).unwrap();
            assert_eq!(
                (
                    our_tier.lp_fee_bps,
                    our_tier.protocol_fee_bps,
                    our_tier.creator_fee_bps
                ),
                (
                    sdk_tier.lp_fee_bps,
                    sdk_tier.protocol_fee_bps,
                    sdk_tier.creator_fee_bps
                ),
                "{name} tier at {m}"
            );
        }
    }
    assert!(compared > 1000, "{compared}");
    // The custom-pair override is what `curve_fees` alone would miss.
    let ours = FeeConfig::decode(&account("fee_config")).unwrap();
    assert_eq!(ours.curve_fees(&other, 0).unwrap().creator, 30);
    assert_eq!(
        ours.curve_fees_for_coin(&other, 0, 300, true)
            .unwrap()
            .creator,
        300
    );
    assert_eq!(
        ours.curve_fees_for_coin(&other, 0, 300, false)
            .unwrap()
            .creator,
        30
    );
}
