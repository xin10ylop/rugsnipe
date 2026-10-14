//! Curve math reproduced exactly against real TradeEvents, account decoders
//! against real `Global` / `FeeConfig` / `BondingCurve` data, and PDAs /
//! market caps against the pump.fun API.

mod common;

use base64::Engine as _;
use common::*;
use pump_codec::constants as c;
use pump_codec::curve::{
    self, buyback_fee, fee_ceil, BondingCurve, CurveState, FeeBps, FeeConfig, Global,
};
use pump_codec::events::{events_from_inner_instructions, PumpEvent, TradeEvent};
use pump_codec::instructions::{classify, Classified, PumpInstruction, PumpTrade, TradeAmounts};
use pump_codec::{pda, Pubkey};

fn account_data(name: &str) -> Vec<u8> {
    let v = read_json(&format!("accounts/{name}.json"));
    let d = &v["result"]["value"]["data"];
    assert_eq!(d[1], "base64");
    base64::engine::general_purpose::STANDARD
        .decode(d[0].as_str().unwrap())
        .unwrap()
}

/// (trade instruction that produced the event, event) pairs of a fixture.
/// Top-level trades and router CPI trades are both matched, in order.
fn trades_with_ix(fx: &TxFixture) -> Vec<(PumpTrade, TradeEvent<'_>)> {
    let keys = fx.full_keys();
    let tx = fx.view();
    // pump trade instructions in execution order: each top-level ix, then its CPIs
    let mut trade_ixs = Vec::new();
    for (i, ix) in tx.instructions().enumerate() {
        let a = tx.instruction_accounts(&ix, Some(&fx.loaded));
        if let Classified::Pump(PumpInstruction::Trade(t)) =
            classify(tx.program_id(&ix), ix.data, &a).unwrap()
        {
            trade_ixs.push(t);
        }
        for (o, p, accts, data) in &fx.inner {
            if *o as usize != i {
                continue;
            }
            let ak: Vec<Pubkey> = accts.iter().map(|&k| keys[k as usize]).collect();
            if let Classified::Pump(PumpInstruction::Trade(t)) =
                classify(&keys[*p as usize], data, ak.as_slice()).unwrap()
            {
                trade_ixs.push(t);
            }
        }
    }
    let events: Vec<TradeEvent<'_>> =
        events_from_inner_instructions(&keys, fx.inner_instructions(), true)
            .unwrap()
            .into_iter()
            .filter_map(|r| {
                if let PumpEvent::Trade(t) = r.event {
                    Some(t)
                } else {
                    None
                }
            })
            .collect();
    assert_eq!(
        trade_ixs.len(),
        events.len(),
        "{}: one TradeEvent per trade ix",
        fx.name
    );
    trade_ixs.into_iter().zip(events).collect()
}

fn fees_of(t: &TradeEvent<'_>) -> FeeBps {
    FeeBps {
        protocol: t.fee_basis_points.unwrap(),
        creator: t.creator_fee_basis_points.unwrap(),
    }
}

#[test]
fn every_sol_trade_event_is_reproduced_exactly() {
    let mut checked = 0;
    for name in tx_names() {
        let fx = TxFixture::load(&name);
        for (ix, ev) in trades_with_ix(&fx) {
            assert_eq!(ix.mint, Some(ev.mint), "{name}");
            assert_eq!(ix.kind.is_buy(), ev.is_buy, "{name}");
            // On-chain ix_name drops the version suffix (doc says "buy_v3"...).
            use pump_codec::instructions::PumpTradeKind as K;
            let want = match ix.kind {
                K::Buy | K::BuyV2 | K::BuyV3 => "buy",
                K::BuyExactSolIn => "buy_exact_sol_in",
                K::BuyExactQuoteInV2 | K::BuyExactQuoteInV3 => "buy_exact_quote_in",
                K::Sell | K::SellV2 | K::SellV3 => "sell",
            };
            assert_eq!(ev.ix_name, Some(want), "{name}: {:?}", ix.kind);
            if ev.quote_mint != Some(Pubkey::default()) {
                continue; // non-SOL quote: reserves are in quote units with other fees
            }
            let pre = ev.pre_state().unwrap();
            let fees = fees_of(&ev);
            let (sol, tokens, pf, cf) = match ix.amounts {
                TradeAmounts::ExactTokensOut {
                    amount,
                    max_quote_cost,
                } => {
                    let q = pre.quote_buy_exact_tokens(amount, fees).unwrap();
                    assert!(q.total <= max_quote_cost, "{name}: slippage bound");
                    (q.sol_cost, q.tokens, q.protocol_fee, q.creator_fee)
                }
                TradeAmounts::ExactQuoteIn {
                    spendable_quote_in,
                    min_tokens_out,
                } => {
                    let q = pre
                        .quote_buy_exact_sol_in(spendable_quote_in, fees)
                        .unwrap();
                    assert!(q.tokens >= min_tokens_out, "{name}: min out");
                    assert!(q.total <= spendable_quote_in);
                    (q.sol_cost, q.tokens, q.protocol_fee, q.creator_fee)
                }
                TradeAmounts::Sell {
                    amount,
                    min_quote_out,
                } => {
                    let q = pre.quote_sell(amount, fees).unwrap();
                    assert!(q.net >= min_quote_out, "{name}: min out");
                    (q.sol_out, q.tokens, q.protocol_fee, q.creator_fee)
                }
            };
            assert_eq!(
                sol,
                ev.sol_amount,
                "{name}: sol_amount ({})",
                ix.kind.name()
            );
            assert_eq!(tokens, ev.token_amount, "{name}: token_amount");
            assert_eq!(pf, ev.fee.unwrap(), "{name}: protocol fee");
            assert_eq!(cf, ev.creator_fee.unwrap(), "{name}: creator fee");
            assert_eq!(fee_ceil(ev.sol_amount, fees.protocol), ev.fee.unwrap());
            assert_eq!(
                buyback_fee(ev.fee.unwrap(), ev.buyback_fee_basis_points.unwrap()),
                ev.buyback_fee.unwrap(),
                "{name}: buyback fee"
            );
            // post-trade reserves
            let mut post = pre;
            if ev.is_buy {
                post.apply_buy(tokens, sol);
            } else {
                post.apply_sell(tokens, sol);
            }
            assert_eq!(post.virtual_sol, ev.virtual_sol_reserves, "{name}");
            assert_eq!(post.virtual_token, ev.virtual_token_reserves, "{name}");
            assert_eq!(Some(post.real_sol), ev.real_sol_reserves, "{name}");
            assert_eq!(Some(post.real_token), ev.real_token_reserves, "{name}");
            assert_eq!(ev.quote_amount, Some(ev.sol_amount));
            checked += 1;
        }
    }
    assert!(checked >= 15, "checked {checked}");
}

#[test]
fn create_txs_start_from_fresh_reserves_and_chain() {
    // In a create tx, the first trade starts from the Global initial reserves
    // and each next trade starts from the previous post-trade state.
    for name in [
        "v1_create_v2_buy",
        "v1_create_v2_buy_b",
        "v1_create_v2_extend_bexq_v3",
        "v0_create_v2_buy_v3",
        "worked_dev_buy_v2",
        "legacy_create_spl",
        "create_buy_complete",
    ] {
        let fx = TxFixture::load(name);
        let trades = trades_with_ix(&fx);
        assert!(!trades.is_empty(), "{name}");
        let creator = trades[0].1.creator.unwrap();
        let mut state = CurveState::fresh(creator);
        for (ix, ev) in &trades {
            assert_eq!(ev.pre_state().unwrap(), state, "{name}: pre-state chain");
            let fees = fees_of(ev);
            let (tok, sol) = match ix.amounts {
                TradeAmounts::ExactTokensOut { amount, .. } => {
                    let q = state.quote_buy_exact_tokens(amount, fees).unwrap();
                    (q.tokens, q.sol_cost)
                }
                TradeAmounts::ExactQuoteIn {
                    spendable_quote_in, ..
                } => {
                    let q = state
                        .quote_buy_exact_sol_in(spendable_quote_in, fees)
                        .unwrap();
                    (q.tokens, q.sol_cost)
                }
                TradeAmounts::Sell { amount, .. } => (amount, state.sell_sol_out(amount)),
            };
            if ev.is_buy {
                state.apply_buy(tok, sol);
            } else {
                state.apply_sell(tok, sol);
            }
            assert_eq!(
                Some(state),
                ev.post_state().map(|mut s| {
                    s.complete = state.complete;
                    s
                }),
                "{name}"
            );
        }
        if name == "create_buy_complete" {
            assert!(state.complete);
            assert_eq!(state.virtual_token, curve::COMPLETE_VIRTUAL_TOKEN_RESERVES);
            assert_eq!(state.real_token, 0);
        }
    }
}

#[test]
fn worked_examples_from_the_mechanics_doc() {
    // section 4: dev buy, next buy, and the sell, on mint 2mFxK8...pump
    let ev = |name: &str| {
        let fx = TxFixture::load(name);
        let t = trades_with_ix(&fx).into_iter().next().unwrap().1;
        (
            t.sol_amount,
            t.token_amount,
            t.fee,
            t.creator_fee,
            t.buyback_fee,
            t.pre_state().unwrap(),
        )
    };
    let (sol, tok, _, _, _, pre) = ev("worked_dev_buy_v2");
    assert_eq!((sol, tok), (750_000_000, 26_170_731_707_316));
    assert_eq!(
        pre,
        CurveState {
            creator: pre.creator,
            ..CurveState::fresh(Pubkey::default())
        }
    );
    assert_eq!(pre.buy_cost(26_170_731_707_316), Some(750_000_000));

    let (sol, tok, _, _, _, pre) = ev("worked_buy");
    assert_eq!((sol, tok), (923_456_789, 30_520_874_344_997));
    assert_eq!(pre.tokens_for_sol(923_456_789), 30_520_874_344_997);

    let (sol, tok, fee, cfee, bb, pre) = ev("worked_sell");
    assert_eq!(sol, 2_412_711_917);
    assert_eq!(pre.sell_sol_out(tok), 2_412_711_917);
    assert_eq!(
        (fee, cfee, bb),
        (Some(22_920_764), Some(7_238_136), Some(11_460_382))
    );
}

#[test]
fn simulate_buys_reproduces_a_multi_buy_create_tx() {
    // v1_create_v2_buy: create, legacy buy (exact tokens), buy_exact_quote_in_v3 (exact SOL).
    let fx = TxFixture::load("v1_create_v2_buy");
    let trades = trades_with_ix(&fx);
    let (ix2, ev2) = &trades[1];
    let TradeAmounts::ExactQuoteIn {
        spendable_quote_in, ..
    } = ix2.amounts
    else {
        panic!()
    };
    let start = trades[0].1.post_state().unwrap();
    let sim = start.simulate_buys(&[spendable_quote_in], FeeBps::DEFAULT);
    assert_eq!(sim.len(), 1);
    assert_eq!(sim[0].0.tokens, ev2.token_amount);
    assert_eq!(sim[0].0.sol_cost, ev2.sol_amount);
    assert_eq!(sim[0].1.virtual_sol, ev2.virtual_sol_reserves);
    assert_eq!(sim[0].1.virtual_token, ev2.virtual_token_reserves);
}

#[test]
fn global_account_decodes_to_documented_values() {
    let g = Global::decode(&account_data("global")).unwrap();
    assert!(g.initialized);
    assert_eq!(
        g.initial_virtual_token_reserves,
        curve::INITIAL_VIRTUAL_TOKEN_RESERVES
    );
    assert_eq!(
        g.initial_virtual_sol_reserves,
        curve::INITIAL_VIRTUAL_SOL_RESERVES
    );
    assert_eq!(
        g.initial_real_token_reserves,
        curve::INITIAL_REAL_TOKEN_RESERVES
    );
    assert_eq!(g.token_total_supply, curve::TOKEN_TOTAL_SUPPLY);
    assert_eq!(g.fee_basis_points, 95);
    assert_eq!(g.creator_fee_basis_points, 5);
    assert_eq!(g.pool_migration_fee, curve::POOL_MIGRATION_FEE);
    assert_eq!(g.buyback_basis_points, curve::DEFAULT_BUYBACK_BPS);
    assert_eq!(g.max_curve_depth, 2);
    assert!(g.create_v2_enabled);
    assert_eq!(g.all_fee_recipients(), c::FEE_RECIPIENTS);
    assert_eq!(g.all_reserved_fee_recipients(), c::RESERVED_FEE_RECIPIENTS);
    assert_eq!(g.buyback_fee_recipients, c::BUYBACK_FEE_RECIPIENTS);
    assert_eq!(
        g.fresh_curve(Pubkey::default()),
        CurveState::fresh(Pubkey::default())
    );
    // wrong discriminator is rejected
    assert!(BondingCurve::decode(&account_data("global")).is_err());
}

#[test]
fn fee_config_is_flat_95_30() {
    let f = FeeConfig::decode(&account_data("fee_config")).unwrap();
    assert!(!f.fee_tiers.is_empty());
    for mcap in [0u128, 30_000_000_000, 400_000_000_000, u128::MAX / 2] {
        assert_eq!(
            f.curve_fees(&Pubkey::default(), mcap),
            Some(FeeBps::DEFAULT),
            "mcap {mcap}"
        );
        assert_eq!(f.curve_fees(&c::WSOL_MINT, mcap), Some(FeeBps::DEFAULT));
    }
}

#[test]
fn bonding_curve_accounts_decode() {
    let bc = BondingCurve::decode(&account_data("bonding_curve_complete")).unwrap();
    assert_eq!(bc.decoded_fields, BondingCurve::FIELDS);
    assert!(bc.complete);
    assert_eq!(bc.real_token_reserves, 0);
    assert_eq!(bc.token_total_supply, curve::TOKEN_TOTAL_SUPPLY);
    assert!(bc.is_sol_quoted());
    // after migration the curve's reserves are zeroed (seen on-chain)
    assert_eq!(
        (
            bc.virtual_token_reserves,
            bc.virtual_quote_reserves,
            bc.real_quote_reserves
        ),
        (0, 0, 0)
    );
    let w = BondingCurve::decode(&account_data("bonding_curve_worked")).unwrap();
    assert!(w.is_sol_quoted());
    assert_ne!(w.creator, Pubkey::default());
    // the API snapshot of the same mint agrees with the account (same minute)
    let coin = &read_json("api/coin_worked.json")["result"];
    assert_eq!(
        coin["bonding_curve"].as_str().unwrap(),
        pda::bonding_curve(&pk("2mFxK8mUNr4MExKZ79t4wL3Z62yj8S3hDPTPZLbFpump")).to_string()
    );
    assert_eq!(coin["creator"].as_str().unwrap(), w.creator.to_string());
    // shorter (older) layouts decode with defaults
    let full = account_data("bonding_curve_worked");
    let old = BondingCurve::decode(&full[..8 + 8 * 5 + 1]).unwrap();
    assert_eq!(old.decoded_fields, 6);
    assert_eq!(old.virtual_token_reserves, w.virtual_token_reserves);
    assert_eq!(old.creator, Pubkey::default());
    let creator_era = BondingCurve::decode(&full[..8 + 8 * 5 + 1 + 32]).unwrap();
    assert_eq!(creator_era.creator, w.creator);
    assert!(BondingCurve::decode(&full[..8 + 8 * 5 + 1 + 16]).is_err());
}

#[test]
fn pdas_match_the_pump_api_for_real_mints() {
    let coins = read_json("api/coins_latest.json");
    let coins = coins["result"].as_array().unwrap();
    let mut n = 0;
    for coin in coins {
        let mint = pk(coin["mint"].as_str().unwrap());
        let bc = pda::bonding_curve(&mint);
        assert_eq!(
            bc.to_string(),
            coin["bonding_curve"].as_str().unwrap(),
            "{mint}"
        );
        if let Some(abc) = coin["associated_bonding_curve"].as_str() {
            let tp = coin["token_program"]
                .as_str()
                .map(pk)
                .unwrap_or(c::TOKEN_2022_PROGRAM_ID);
            assert_eq!(
                pda::associated_bonding_curve(&mint, &tp).to_string(),
                abc,
                "{mint}"
            );
        }
        n += 1;
    }
    assert!(n >= 3);
    let done = read_json("api/coins_completed.json");
    let mut pools = 0;
    for coin in done["result"].as_array().unwrap() {
        let mint = pk(coin["mint"].as_str().unwrap());
        assert_eq!(
            pda::bonding_curve(&mint).to_string(),
            coin["bonding_curve"].as_str().unwrap()
        );
        if let Some(pool) = coin["pool_address"].as_str() {
            assert_eq!(
                pda::canonical_pump_swap_pool(&mint, &c::WSOL_MINT).to_string(),
                pool,
                "{mint}"
            );
            pools += 1;
        }
    }
    assert!(pools >= 3);
}

#[test]
fn market_cap_matches_the_api() {
    let coins = read_json("api/coins_latest.json");
    let mut n = 0;
    for coin in coins["result"].as_array().unwrap() {
        let (Some(v), Some(t), Some(mc)) = (
            coin["virtual_sol_reserves"].as_u64(),
            coin["virtual_token_reserves"].as_u64(),
            coin["market_cap"].as_f64(),
        ) else {
            continue;
        };
        if coin["quote_mint"]
            .as_str()
            .is_some_and(|q| !c::is_sol_like_quote_mint(&pk(q)))
        {
            continue;
        }
        // Completed coins: the API prices them from the PumpSwap pool while the
        // curve reserves stay frozen at completion.
        if coin["complete"].as_bool() == Some(true) {
            continue;
        }
        let s = CurveState {
            virtual_sol: v,
            virtual_token: t,
            real_sol: 0,
            real_token: 0,
            complete: false,
            creator: Pubkey::default(),
        };
        let ours = s.market_cap_sol();
        assert!(
            (ours - mc).abs() <= 1e-6 * mc.max(1.0),
            "{}: {ours} vs {mc}",
            coin["mint"]
        );
        n += 1;
    }
    assert!(n >= 10, "{n}");
}
