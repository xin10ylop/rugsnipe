//! Instruction classification on real transactions, account-order checks of
//! the v3 / legacy builders against on-chain instructions.

mod common;

use common::*;
use pump_codec::constants as c;
use pump_codec::instructions::{
    classify, AccountLookup, AmmInstruction, AmmTradeKind, Classified, LegacyTradeKeys,
    PumpInstruction, PumpTrade, PumpTradeKind, SolV3Keys, TradeAmounts,
};
use pump_codec::{pda, Pubkey};

/// (top-level index, classification summary) of every pump/PumpSwap ix.
fn top_level(fx: &TxFixture) -> Vec<(usize, String)> {
    let tx = fx.view();
    let mut out = Vec::new();
    for (i, ix) in tx.instructions().enumerate() {
        let a = tx.instruction_accounts(&ix, Some(&fx.loaded));
        let s = match classify(tx.program_id(&ix), ix.data, &a).unwrap() {
            Classified::Pump(PumpInstruction::Create(c)) => {
                format!("pump.create:{:?}/{}", c.kind, c.num_accounts)
            }
            Classified::Pump(PumpInstruction::Trade(t)) => {
                format!("pump.{}/{}", t.kind.name(), t.num_accounts)
            }
            Classified::Pump(PumpInstruction::Other { discriminator }) => {
                format!("pump.other:{}", hex(&discriminator))
            }
            Classified::PumpSwap(AmmInstruction::Trade(t)) => {
                format!("amm.{:?}/{}", t.kind, t.num_accounts)
            }
            Classified::PumpSwap(AmmInstruction::CreatePool(_)) => "amm.create_pool".into(),
            Classified::PumpSwap(AmmInstruction::Other { discriminator }) => {
                format!("amm.other:{}", hex(&discriminator))
            }
            Classified::NotPump => continue,
        };
        out.push((i, s));
    }
    out
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[test]
fn classify_top_level_instructions() {
    let expected: &[(&str, &[&str])] = &[
        (
            "v1_create_v2_buy",
            &[
                "pump.create:V2/16",
                "pump.buy/18",
                "pump.buy_exact_quote_in_v3/17",
            ],
        ),
        ("v1_create_v2_buy_b", &["pump.create:V2/16", "pump.buy/18"]),
        (
            "v1_create_v2_extend_bexq_v3",
            &[
                "pump.create:V2/16",
                "pump.other:ea66c2cb96483ee5",
                "pump.buy_exact_quote_in_v3/17",
            ],
        ),
        (
            "v0_create_v2_buy_v3",
            &["pump.create:V2/16", "pump.buy_v3/17"],
        ),
        (
            "v0_create_v2_quote_tail",
            &["pump.create:V2/20", "pump.buy_v2/27"],
        ),
        (
            "worked_dev_buy_v2",
            &["pump.create:V2/16", "pump.buy_v2/27"],
        ),
        (
            "legacy_create_spl",
            &["pump.create:Legacy/14", "pump.buy/18"],
        ),
        ("legacy_buy18", &["pump.buy/18"]),
        ("legacy_buy19", &["pump.buy/19"]),
        ("legacy_buy_exact_sol_in", &["pump.buy_exact_sol_in/18"]),
        ("legacy_sell16", &["pump.sell/16"]),
        ("v1_sell16", &["pump.sell/16"]),
        (
            "v1_buy_exact_quote_in_v3",
            &["pump.buy_exact_quote_in_v3/17"],
        ),
        ("v1_router_inner_trade", &[]),
        ("worked_buy", &[]),
        ("worked_sell", &[]),
        ("migrate", &["pump.other:bbcb121fceedfe29"]),
        ("pumpswap_buy", &["amm.BuyExactQuoteIn/26"]),
    ];
    for (name, want) in expected {
        let fx = TxFixture::load(name);
        let got: Vec<String> = top_level(&fx).into_iter().map(|x| x.1).collect();
        assert_eq!(&got, want, "{name}");
    }
    // PumpSwap sell: shared `sell` discriminator classified by program ID.
    let got: Vec<String> = top_level(&TxFixture::load("pumpswap_sell"))
        .into_iter()
        .map(|x| x.1)
        .collect();
    assert!(got.iter().any(|g| g.starts_with("amm.Sell")), "{got:?}");
}

fn first_trade(
    fx: &TxFixture,
    kind: PumpTradeKind,
) -> (usize, PumpTrade, Vec<(Pubkey, bool, bool)>) {
    let tx = fx.view();
    let full = fx.full_keys();
    for (i, ix) in tx.instructions().enumerate() {
        let a = tx.instruction_accounts(&ix, Some(&fx.loaded));
        if let Classified::Pump(PumpInstruction::Trade(t)) =
            classify(tx.program_id(&ix), ix.data, &a).unwrap()
        {
            if t.kind == kind {
                let metas = ix
                    .accounts
                    .iter()
                    .map(|&k| {
                        (
                            full[k as usize],
                            tx.is_signer(k as usize),
                            tx.is_writable(k as usize),
                        )
                    })
                    .collect();
                return (i, t, metas);
            }
        }
    }
    panic!("{}: no {kind:?}", fx.name)
}

#[test]
fn on_chain_buy_v3_matches_our_17_account_builder() {
    // v0 create_v2 + buy_v3 (17 accounts) and v1 / legacy buy_exact_quote_in_v3.
    let mut user_ata_matches = 0;
    for (name, kind) in [
        ("v0_create_v2_buy_v3", PumpTradeKind::BuyV3),
        ("v1_buy_exact_quote_in_v3", PumpTradeKind::BuyExactQuoteInV3),
        ("v1_create_v2_buy", PumpTradeKind::BuyExactQuoteInV3),
        (
            "v1_create_v2_extend_bexq_v3",
            PumpTradeKind::BuyExactQuoteInV3,
        ),
    ] {
        let fx = TxFixture::load(name);
        let (_, t, metas) = first_trade(&fx, kind);
        assert_eq!(metas.len(), 17);
        let mint = t.mint.unwrap();
        let user = t.user.unwrap();
        let buyback = t.buyback_fee_recipient.unwrap();
        assert!(c::BUYBACK_FEE_RECIPIENTS.contains(&buyback), "{name}");
        assert_eq!(t.quote_mint, Some(c::WSOL_MINT));
        let keys = SolV3Keys::derive(mint, user, t.base_token_program.unwrap(), buyback);
        let ours = keys.account_metas();
        for (i, (m, (k, s, w))) in ours.iter().zip(&metas).enumerate() {
            // #9/#10 are the user's token accounts: the program accepts any
            // token account of the user (no PDA constraint in the IDL; the wSOL
            // one is ignored on SOL coins). We default to the ATAs.
            if i == 9 || i == 10 {
                if m.pubkey == *k {
                    user_ata_matches += 1;
                }
                continue;
            }
            assert_eq!(m.pubkey, *k, "{name}: account #{i}");
            // Signer/writable are tx-level properties: what we require must
            // hold on-chain; the reverse can be widened by other instructions
            // (e.g. the mint signs the create in the same tx).
            if m.is_signer {
                assert!(*s, "{name}: #{i} should sign");
            }
            // On-chain writability is a tx-level property: an account we mark
            // writable must be writable on-chain; read-only ones may have been
            // made writable by another instruction in the same tx.
            if m.is_writable {
                assert!(*w, "{name}: #{i} should be writable on-chain");
            }
        }
        // buy_v3 / buy_exact_quote_in_v3 args
        match (kind, t.amounts) {
            (PumpTradeKind::BuyV3, TradeAmounts::ExactTokensOut { .. }) => {}
            (PumpTradeKind::BuyExactQuoteInV3, TradeAmounts::ExactQuoteIn { .. }) => {}
            other => panic!("{name}: {other:?}"),
        }
    }
    assert!(
        user_ata_matches >= 4,
        "most senders use the ATAs: {user_ata_matches}/8"
    );
}

#[test]
fn legacy_trades_carry_bonding_curve_v2_and_buyback_remaining_accounts() {
    let mut non_ata_users = Vec::new();
    for (name, kind) in [
        ("legacy_buy18", PumpTradeKind::Buy),
        ("legacy_buy19", PumpTradeKind::Buy),
        ("legacy_buy_exact_sol_in", PumpTradeKind::BuyExactSolIn),
        ("v1_create_v2_buy", PumpTradeKind::Buy),
        ("legacy_create_spl", PumpTradeKind::Buy),
        ("legacy_sell16", PumpTradeKind::Sell),
        ("v1_sell16", PumpTradeKind::Sell),
    ] {
        let fx = TxFixture::load(name);
        let (_, t, metas) = first_trade(&fx, kind);
        let mint = t.mint.unwrap();
        let first_remaining = if kind == PumpTradeKind::Sell { 14 } else { 16 };
        assert_eq!(
            metas[first_remaining].0,
            pda::bonding_curve_v2(&mint),
            "{name}: bonding-curve-v2"
        );
        assert!(
            !metas[first_remaining].2 || name == "legacy_buy19",
            "{name}: bc-v2 is read-only"
        );
        let bb = metas[first_remaining + 1].0;
        assert!(
            c::BUYBACK_FEE_RECIPIENTS.contains(&bb),
            "{name}: buyback {bb}"
        );
        assert_eq!(t.buyback_fee_recipient, Some(bb), "{name}");
        let fr = t.fee_recipient.unwrap();
        assert!(
            c::FEE_RECIPIENTS.contains(&fr) || c::RESERVED_FEE_RECIPIENTS.contains(&fr),
            "{name}"
        );
        // Our legacy builder reproduces the on-chain account list. The creator
        // vault is ["creator-vault", creator]; recover the creator from the TradeEvent.
        let keys_full = fx.full_keys();
        let ev = pump_codec::events::events_from_inner_instructions(
            &keys_full,
            fx.inner_instructions(),
            true,
        )
        .unwrap();
        let creator = ev
            .iter()
            .find_map(|r| match &r.event {
                pump_codec::events::PumpEvent::Trade(te) if te.mint == mint => te.creator,
                _ => None,
            })
            .unwrap();
        let k = LegacyTradeKeys::derive(
            mint,
            t.user.unwrap(),
            creator,
            t.base_token_program.unwrap(),
            fr,
            bb,
        );
        let ours = if kind == PumpTradeKind::Sell {
            k.sell_metas(false)
        } else {
            k.buy_metas()
        };
        for (i, m) in ours.iter().enumerate() {
            if i == 5 {
                // associated_user: any token account of the user is accepted.
                if m.pubkey != metas[i].0 {
                    non_ata_users.push(name);
                }
                continue;
            }
            assert_eq!(m.pubkey, metas[i].0, "{name}: account #{i}");
            if m.is_signer {
                assert!(metas[i].1, "{name}: signer #{i}");
            }
            if m.is_writable {
                assert!(metas[i].2, "{name}: #{i} writable");
            }
        }
        if name == "legacy_buy19" {
            // 19th account: an extra trailing account appended by the sender
            // (not used by the program); parsing tolerates it.
            assert_eq!(metas.len(), 19);
        }
    }
    // Two of the seven senders trade from a non-ATA token account; every other
    // account matches our builder exactly.
    assert_eq!(
        non_ata_users,
        vec!["legacy_buy_exact_sol_in", "legacy_sell16"]
    );
}

#[test]
fn router_cpi_trades_classify_from_inner_instructions() {
    // worked_buy / worked_sell go through routers: the pump ix is a CPI.
    for (name, kind) in [
        ("worked_buy", PumpTradeKind::Buy),
        ("worked_sell", PumpTradeKind::Sell),
    ] {
        let fx = TxFixture::load(name);
        let keys = fx.full_keys();
        let mut found = 0;
        for (_, prog, accts, data) in &fx.inner {
            let program = keys[*prog as usize];
            let ak: Vec<Pubkey> = accts.iter().map(|&i| keys[i as usize]).collect();
            if let Classified::Pump(PumpInstruction::Trade(t)) =
                classify(&program, data, ak.as_slice()).unwrap()
            {
                assert_eq!(t.kind, kind, "{name}");
                assert_eq!(
                    t.mint,
                    Some(pk("2mFxK8mUNr4MExKZ79t4wL3Z62yj8S3hDPTPZLbFpump"))
                );
                found += 1;
            }
        }
        assert_eq!(found, 1, "{name}");
    }
}

#[test]
fn worked_sell_tx_starts_with_durable_nonce_advance() {
    // tx 54rjuNFq...: AdvanceNonceAccount (system, tag 4) is instruction #0.
    let fx = TxFixture::load("worked_sell");
    let tx = fx.view();
    let ix0 = tx.instruction(0).unwrap();
    assert_eq!(*tx.program_id(&ix0), c::SYSTEM_PROGRAM_ID);
    assert_eq!(ix0.data, &4u32.to_le_bytes());
    let a = tx.instruction_accounts(&ix0, None);
    assert_eq!(a.account(1), Some(c::RECENT_BLOCKHASHES_SYSVAR_ID));
    let ours = pump_codec::instructions::advance_nonce_account(
        &a.account(0).unwrap(),
        &a.account(2).unwrap(),
    );
    assert_eq!(ours.data, ix0.data);
    assert_eq!(
        ours.accounts.iter().map(|m| m.pubkey).collect::<Vec<_>>(),
        (0..3).map(|i| a.account(i).unwrap()).collect::<Vec<_>>()
    );
}

#[test]
fn pumpswap_trade_fields() {
    let fx = TxFixture::load("pumpswap_buy");
    let tx = fx.view();
    let mut seen = false;
    for ix in tx.instructions() {
        let a = tx.instruction_accounts(&ix, Some(&fx.loaded));
        if let Classified::PumpSwap(AmmInstruction::Trade(t)) =
            classify(tx.program_id(&ix), ix.data, &a).unwrap()
        {
            assert_eq!(t.kind, AmmTradeKind::BuyExactQuoteIn);
            assert_eq!(
                t.base_mint,
                Some(pk("Cc2QsnFfxFYXaTvdXgJkUctM9ZsNBKfgaaaSihjepump"))
            );
            assert_eq!(t.quote_mint, Some(c::WSOL_MINT));
            assert_eq!(
                t.pool,
                Some(pda::canonical_pump_swap_pool(
                    &t.base_mint.unwrap(),
                    &c::WSOL_MINT
                ))
            );
            assert_eq!(a.account(2), Some(c::PUMP_SWAP_GLOBAL_CONFIG));
            assert!(t.kind.is_buy());
            seen = true;
        }
    }
    assert!(seen);
}

#[test]
fn v3_keys_from_create_equal_full_derivation() {
    use pump_codec::instructions::UserKeys;
    for name in [
        "v1_create_v2_buy",
        "v0_create_v2_buy_v3",
        "legacy_create_spl",
        "worked_dev_buy_v2",
    ] {
        let fx = TxFixture::load(name);
        let tx = fx.view();
        let create = tx
            .instructions()
            .find_map(|ix| {
                match classify(
                    tx.program_id(&ix),
                    ix.data,
                    &tx.instruction_accounts(&ix, Some(&fx.loaded)),
                )
                .unwrap()
                {
                    Classified::Pump(PumpInstruction::Create(cr)) => Some(cr),
                    _ => None,
                }
            })
            .unwrap();
        let sniper = Pubkey::new_unique();
        let uk = UserKeys::derive(sniper);
        let fast = SolV3Keys::from_create(&create, &uk, c::BUYBACK_FEE_RECIPIENTS[2]).unwrap();
        let slow = SolV3Keys::derive(
            create.mint.unwrap(),
            sniper,
            create.token_program.unwrap(),
            c::BUYBACK_FEE_RECIPIENTS[2],
        );
        assert_eq!(fast, slow, "{name}");
    }
    // Non-SOL quote: no SOL v3 keys.
    let fx = TxFixture::load("v0_create_v2_quote_tail");
    let tx = fx.view();
    let create = tx
        .instructions()
        .find_map(|ix| {
            match classify(
                tx.program_id(&ix),
                ix.data,
                &tx.instruction_accounts(&ix, Some(&fx.loaded)),
            )
            .unwrap()
            {
                Classified::Pump(PumpInstruction::Create(cr)) => Some(cr),
                _ => None,
            }
        })
        .unwrap();
    assert!(SolV3Keys::from_create(
        &create,
        &pump_codec::instructions::UserKeys::derive(Pubkey::new_unique()),
        c::BUYBACK_FEE_RECIPIENTS[0]
    )
    .is_none());
}
