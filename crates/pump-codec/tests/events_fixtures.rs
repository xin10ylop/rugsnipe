//! Event decoding on real transactions: CPI inner instructions vs
//! `Program data:` logs, field completeness, and cross-links between events.

mod common;

use common::*;
use pump_codec::constants as c;
use pump_codec::events::{
    decode_event_payload, events_from_inner_instructions, events_from_log_scan,
    scan_program_data_logs, EventSource, PumpEvent,
};
use pump_codec::pda;

fn cpi_events(fx: &TxFixture) -> Vec<pump_codec::events::EventRecord<'_>> {
    let keys = fx.full_keys();
    let strict = events_from_inner_instructions(&keys, fx.inner_instructions(), true).unwrap();
    let loose = events_from_inner_instructions(&keys, fx.inner_instructions(), false).unwrap();
    assert_eq!(
        strict, loose,
        "{}: event-authority filter changed the result",
        fx.name
    );
    strict
}

#[test]
fn events_per_fixture() {
    // (fixture, expected event names in order)
    let expected: &[(&str, &[&str])] = &[
        (
            "v1_create_v2_buy",
            &["CreateEvent", "TradeEvent", "TradeEvent"],
        ),
        ("v0_create_v2_buy_v3", &["CreateEvent", "TradeEvent"]),
        ("worked_dev_buy_v2", &["CreateEvent", "TradeEvent"]),
        ("worked_buy", &["TradeEvent"]),
        ("worked_sell", &["TradeEvent"]),
        ("legacy_create_spl", &["CreateEvent", "TradeEvent"]),
        (
            "create_buy_complete",
            &["CreateEvent", "TradeEvent", "CompleteEvent"],
        ),
        ("v1_router_inner_trade", &["TradeEvent"]),
        ("pumpswap_buy", &["BuyEvent"]),
        ("pumpswap_sell", &["SellEvent"]),
    ];
    for (name, want) in expected {
        let fx = TxFixture::load(name);
        let got: Vec<&str> = cpi_events(&fx)
            .iter()
            .filter(|r| !matches!(r.event, PumpEvent::Other { .. }))
            .map(|r| r.event.name())
            .collect();
        assert_eq!(&got, want, "{name}");
    }
    // migration: CreatePoolEvent (PumpSwap) then CompletePumpAmmMigrationEvent (pump),
    // with an undecoded PumpSwap event in between.
    let fx = TxFixture::load("migrate");
    let ev = cpi_events(&fx);
    let names: Vec<_> = ev.iter().map(|r| (r.source, r.event.name())).collect();
    assert!(
        names.contains(&(EventSource::PumpSwap, "CreatePoolEvent")),
        "{names:?}"
    );
    assert!(
        names.contains(&(EventSource::Pump, "CompletePumpAmmMigrationEvent")),
        "{names:?}"
    );
}

#[test]
fn log_events_match_cpi_events_when_logged() {
    let mut with_logs = 0;
    let mut truncated = Vec::new();
    for name in tx_names() {
        let fx = TxFixture::load(&name);
        let scan = scan_program_data_logs(&fx.logs).unwrap();
        let from_logs = events_from_log_scan(&scan).unwrap();
        let cpi = cpi_events(&fx);
        if from_logs.is_empty() {
            continue;
        }
        with_logs += 1;
        // Every logged pump/PumpSwap event also appears as a CPI event, same order.
        let cpi_ev: Vec<_> = cpi.iter().map(|r| (r.source, r.event.clone())).collect();
        let mut it = cpi_ev.iter();
        for le in &from_logs {
            assert!(
                it.any(|ce| ce == le),
                "{name}: log event {} not among CPI events",
                le.1.name()
            );
        }
        // and the decoded (known) events are exactly the same sequence
        let known = |v: &[(EventSource, PumpEvent<'_>)]| -> Vec<String> {
            v.iter()
                .filter(|e| !matches!(e.1, PumpEvent::Other { .. }))
                .map(|e| format!("{:?}", e))
                .collect()
        };
        if scan.truncated {
            // The runtime cut the logs: events are missing from the log path
            // (the reason to prefer inner instructions). Seen on `migrate` and
            // `v1_create_v2_buy`.
            truncated.push(name.clone());
            assert!(known(&from_logs).len() < known(&cpi_ev).len(), "{name}");
        } else {
            assert_eq!(known(&from_logs), known(&cpi_ev), "{name}");
        }
    }
    assert_eq!(with_logs, tx_names().len(), "every fixture logs its events");
    // Truncation is flagged exactly for the fixtures whose logs contain the marker.
    let marked: Vec<String> = tx_names()
        .into_iter()
        .filter(|n| TxFixture::load(n).logs.iter().any(|l| l == "Log truncated"))
        .collect();
    assert!(!marked.is_empty());
    assert_eq!(truncated, marked);
}

#[test]
fn trade_events_are_complete_and_consistent() {
    let mut n = 0;
    for name in tx_names() {
        let fx = TxFixture::load(&name);
        for r in cpi_events(&fx) {
            let PumpEvent::Trade(t) = &r.event else {
                continue;
            };
            n += 1;
            // Every 2026-10 field is present (no trailing truncation on fresh txs).
            assert!(
                t.creator_fee_unclaimed.is_some(),
                "{name}: missing trailing fields"
            );
            assert!(t.ix_name.is_some() && t.shareholders.is_some() && t.quote_mint.is_some());
            assert_eq!(t.fee_basis_points, Some(95), "{name}");
            let cbps = t.creator_fee_basis_points.unwrap();
            assert!(
                cbps == 30 || t.quote_mint != Some(pump_codec::Pubkey::default()),
                "{name}: creator bps {cbps}"
            );
            // v3 trades keep the protocol fee on the curve: zero fee recipient.
            let fr = t.fee_recipient.unwrap();
            if fr != pump_codec::Pubkey::default() {
                assert!(
                    c::FEE_RECIPIENTS.contains(&fr) || c::RESERVED_FEE_RECIPIENTS.contains(&fr),
                    "{name}: unknown fee recipient {fr}"
                );
            }
            // mayhem trades pay the reserved recipients and no buyback fee
            if t.mayhem_mode == Some(true) && fr != pump_codec::Pubkey::default() {
                assert!(c::RESERVED_FEE_RECIPIENTS.contains(&fr), "{name}");
            }
            let post = t.post_state().unwrap();
            assert_eq!(post.virtual_sol, t.virtual_sol_reserves);
            assert!(t.is_creator_trade() == (t.creator == Some(t.user)));
        }
    }
    assert!(n >= 15, "only {n} trade events");
}

#[test]
fn create_event_links_to_create_instruction_and_pdas() {
    use pump_codec::instructions::{classify, Classified, CreateKind, PumpInstruction};
    for name in [
        "v1_create_v2_buy",
        "v1_create_v2_buy_b",
        "v1_create_v2_extend_bexq_v3",
        "v0_create_v2_buy_v3",
        "v0_create_v2_quote_tail",
        "worked_dev_buy_v2",
        "legacy_create_spl",
        "create_buy_complete",
    ] {
        let fx = TxFixture::load(name);
        let tx = fx.view();
        let ev = cpi_events(&fx);
        let ce = ev
            .iter()
            .find_map(|r| match &r.event {
                PumpEvent::Create(c) => Some((r.outer_index, c.clone())),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{name}: no CreateEvent"));
        let (outer, ce) = ce;
        let ix = tx.instruction(outer as usize).unwrap();
        let accts = tx.instruction_accounts(&ix, Some(&fx.loaded));
        let Classified::Pump(PumpInstruction::Create(cr)) =
            classify(tx.program_id(&ix), ix.data, &accts).unwrap()
        else {
            panic!("{name}: outer ix {outer} is not a create");
        };
        assert_eq!(cr.args.name, ce.name, "{name}");
        assert_eq!(cr.args.symbol, ce.symbol);
        assert_eq!(cr.args.uri, ce.uri);
        assert_eq!(cr.args.creator, ce.creator);
        assert_eq!(cr.mint, Some(ce.mint));
        assert_eq!(cr.bonding_curve, Some(ce.bonding_curve));
        assert_eq!(cr.user, Some(ce.user), "{name}: user");
        assert_eq!(cr.token_program, ce.token_program, "{name}");
        assert_eq!(pda::bonding_curve(&ce.mint), ce.bonding_curve);
        assert_eq!(
            cr.associated_bonding_curve,
            Some(pda::associated_token_address(
                &ce.bonding_curve,
                &ce.mint,
                &ce.token_program.unwrap()
            ))
        );
        assert_eq!(
            *tx.fee_payer() == ce.user,
            tx.signer_keys().contains(&ce.user) && *tx.fee_payer() == ce.user
        );
        match cr.kind {
            CreateKind::V2 => {
                assert_eq!(ce.token_program, Some(c::TOKEN_2022_PROGRAM_ID), "{name}");
                assert_eq!(Some(cr.args.is_mayhem_mode), ce.is_mayhem_mode);
                assert_eq!(
                    cr.args.is_holder_reward.unwrap_or(false),
                    ce.is_holder_reward.unwrap()
                );
                // mayhem accounts #12/#13 are PDAs of the mint
                assert_eq!(
                    accts_get(&accts, 12),
                    Some(pda::mayhem_state(&ce.mint)),
                    "{name}"
                );
                assert_eq!(
                    accts_get(&accts, 13),
                    Some(pda::mayhem_token_vault(&ce.mint)),
                    "{name}"
                );
                assert_eq!(accts_get(&accts, 10), Some(c::MAYHEM_GLOBAL_PARAMS));
                assert_eq!(accts_get(&accts, 11), Some(c::MAYHEM_SOL_VAULT));
                let sol = ce.quote_mint.map(|q| c::is_sol_like_quote_mint(&q));
                assert_eq!(cr.is_sol_quote, sol, "{name}: quote classification");
            }
            CreateKind::Legacy => {
                assert_eq!(ce.token_program, Some(c::SPL_TOKEN_PROGRAM_ID), "{name}");
                assert_eq!(cr.num_accounts, 14);
                assert_eq!(accts_get(&accts, 6), Some(pda::metadata(&ce.mint)));
            }
        }
    }
}

fn accts_get(
    a: &impl pump_codec::instructions::AccountLookup,
    i: usize,
) -> Option<pump_codec::Pubkey> {
    a.account(i)
}

#[test]
fn quote_tail_create_is_classified_without_alt_resolution_when_static() {
    use pump_codec::instructions::{classify, Classified, PumpInstruction};
    let fx = TxFixture::load("v0_create_v2_quote_tail");
    let tx = fx.view();
    let (ix, cr) = tx
        .instructions()
        .find_map(|ix| {
            let a = tx.instruction_accounts(&ix, Some(&fx.loaded));
            match classify(tx.program_id(&ix), ix.data, &a).unwrap() {
                Classified::Pump(PumpInstruction::Create(c)) => Some((ix, c)),
                _ => None,
            }
        })
        .unwrap();
    assert_eq!(cr.num_accounts, 20);
    assert_eq!(cr.quote_tail_len, 4);
    assert_eq!(cr.is_sol_quote, Some(false));
    assert_eq!(
        accts_get(&tx.instruction_accounts(&ix, Some(&fx.loaded)), 19),
        Some(pda::quote_control())
    );
    // Without the loaded addresses, signer/static accounts still resolve.
    let a = tx.instruction_accounts(&ix, None);
    let Classified::Pump(PumpInstruction::Create(c2)) =
        classify(tx.program_id(&ix), ix.data, &a).unwrap()
    else {
        panic!()
    };
    assert_eq!(c2.mint, cr.mint);
    assert_eq!(c2.user, cr.user);
    assert_eq!(c2.args, cr.args);
}

#[test]
fn completion_and_migration_events_link_up() {
    let fx = TxFixture::load("create_buy_complete");
    let ev = cpi_events(&fx);
    let create = ev
        .iter()
        .find_map(|r| {
            if let PumpEvent::Create(c) = &r.event {
                Some(c.clone())
            } else {
                None
            }
        })
        .unwrap();
    let trade = ev
        .iter()
        .find_map(|r| {
            if let PumpEvent::Trade(t) = &r.event {
                Some(t.clone())
            } else {
                None
            }
        })
        .unwrap();
    let complete = ev
        .iter()
        .find_map(|r| {
            if let PumpEvent::Complete(c) = &r.event {
                Some(c.clone())
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(complete.mint, create.mint);
    assert_eq!(complete.bonding_curve, create.bonding_curve);
    assert_eq!(trade.real_token_reserves, Some(0));
    assert_eq!(
        trade.virtual_token_reserves,
        pump_codec::curve::COMPLETE_VIRTUAL_TOKEN_RESERVES
    );

    let fx = TxFixture::load("migrate");
    let ev = cpi_events(&fx);
    let mig = ev
        .iter()
        .find_map(|r| {
            if let PumpEvent::CompletePumpAmmMigration(m) = &r.event {
                Some(m.clone())
            } else {
                None
            }
        })
        .unwrap();
    let cp = ev
        .iter()
        .find_map(|r| {
            if let PumpEvent::AmmCreatePool(p) = &r.event {
                Some(p.clone())
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(mig.mint, create.mint);
    assert_eq!(mig.pool, cp.pool);
    assert_eq!(cp.base_mint, create.mint);
    assert_eq!(cp.quote_mint, c::WSOL_MINT);
    assert_eq!(cp.index, 0);
    assert_eq!(cp.creator, pda::pool_authority(&create.mint));
    assert_eq!(
        mig.pool,
        pda::canonical_pump_swap_pool(&create.mint, &c::WSOL_MINT)
    );
    assert_eq!(
        mig.pool_migration_fee,
        pump_codec::curve::POOL_MIGRATION_FEE
    );
    // the API agrees on the pool address
    let coins = read_json("api/coins_completed.json");
    let api = coins["result"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["mint"] == create.mint.to_string())
        .unwrap();
    assert_eq!(api["pool_address"].as_str().unwrap(), mig.pool.to_string());

    // later PumpSwap trades on that pool
    for name in ["pumpswap_buy", "pumpswap_sell"] {
        let fx = TxFixture::load(name);
        for r in cpi_events(&fx) {
            match &r.event {
                PumpEvent::AmmBuy(e) | PumpEvent::AmmSell(e) => {
                    assert!(e.creator_fee_unclaimed.is_some(), "{name}: trailing fields");
                    assert!(e.protocol_fee_basis_points > 0);
                    if name == "pumpswap_buy" {
                        assert_eq!(e.pool, mig.pool);
                        assert!(e.ix_name.is_some());
                    } else {
                        assert!(e.ix_name.is_none());
                    }
                }
                _ => {}
            }
        }
    }
}

#[test]
fn truncated_trade_event_parses_up_to_present_fields() {
    let fx = TxFixture::load("worked_sell");
    let data = fx
        .inner
        .iter()
        .map(|(_, _, _, d)| d)
        .find(|d| d.len() > 16 && d[..8] == c::EVENT_IX_TAG && d[8..16] == c::event::TRADE)
        .unwrap();
    let payload = &data[8..];
    let full = match decode_event_payload(EventSource::Pump, payload).unwrap() {
        PumpEvent::Trade(t) => t,
        _ => panic!(),
    };
    // Original 2024 layout: 8 fields = 8 + 32+8+8+1+32+8+8+8 bytes.
    let core = 8 + 32 + 8 + 8 + 1 + 32 + 8 + 8 + 8;
    let PumpEvent::Trade(t) = decode_event_payload(EventSource::Pump, &payload[..core]).unwrap()
    else {
        panic!()
    };
    assert_eq!(t.sol_amount, full.sol_amount);
    assert_eq!(t.virtual_token_reserves, full.virtual_token_reserves);
    assert!(t.real_sol_reserves.is_none() && t.fee.is_none() && t.ix_name.is_none());
    // May 2025 layout: + real reserves, fee recipient/bps/fee, creator/bps/fee.
    let creator_era = core + 8 + 8 + 32 + 8 + 8 + 32 + 8 + 8;
    let PumpEvent::Trade(t) =
        decode_event_payload(EventSource::Pump, &payload[..creator_era]).unwrap()
    else {
        panic!()
    };
    assert_eq!(t.creator_fee, full.creator_fee);
    assert!(t.track_volume.is_none());
    // a field cut in half is an error, not a silent None
    assert!(decode_event_payload(EventSource::Pump, &payload[..core + 3]).is_err());
    // too short for the core is an error
    assert!(decode_event_payload(EventSource::Pump, &payload[..40]).is_err());
    // the same bytes from a foreign program are ignored
    let foreign = pump_codec::Pubkey::new_unique();
    assert!(pump_codec::events::decode_cpi_event(&foreign, data)
        .unwrap()
        .is_none());
}

#[test]
fn post_complete_buy_event_synthetic_round_trip() {
    // No PostCompleteBuyEvent was found on-chain (searched the last 3
    // bonding-curve transactions of ~50 recently completed coins on
    // 2026-10-14), so this checks the decoder against bytes laid out from the
    // IDL field list.
    let k = |b: u8| [b; 32];
    let mut d = Vec::new();
    d.extend_from_slice(&c::EVENT_IX_TAG);
    d.extend_from_slice(&c::event::POST_COMPLETE_BUY);
    for b in [1u8, 2, 3, 4] {
        d.extend_from_slice(&k(b)); // user, mint, bonding_curve, quote_mint
    }
    d.extend_from_slice(&1_791_968_847i64.to_le_bytes());
    for v in 1u64..=11 {
        d.extend_from_slice(&(v * 1000).to_le_bytes());
    }
    let ev = pump_codec::events::decode_cpi_event(&c::PUMP_PROGRAM_ID, &d)
        .unwrap()
        .unwrap();
    let PumpEvent::PostCompleteBuy(e) = ev else {
        panic!("{ev:?}")
    };
    assert_eq!(e.user.to_bytes(), k(1));
    assert_eq!(e.quote_mint.to_bytes(), k(4));
    assert_eq!(e.timestamp, 1_791_968_847);
    assert_eq!(
        (e.base_out, e.quote_in, e.fee_basis_points),
        (1000, 2000, 3000)
    );
    assert_eq!(e.pool_quote_reserves_after, 11_000);
    assert!(pump_codec::events::decode_cpi_event(&c::PUMP_PROGRAM_ID, &d[..d.len() - 1]).is_err());
    // Same bytes from PumpSwap are not a pump event.
    let other = pump_codec::events::decode_cpi_event(&c::PUMP_SWAP_PROGRAM_ID, &d)
        .unwrap()
        .unwrap();
    assert!(matches!(
        other,
        PumpEvent::Other {
            source: EventSource::PumpSwap,
            ..
        }
    ));
}

#[test]
fn mayhem_trades_pay_no_buyback_and_use_reserved_recipients() {
    let mut mayhem = 0;
    for name in tx_names() {
        let fx = TxFixture::load(&name);
        for r in cpi_events(&fx) {
            let PumpEvent::Trade(t) = &r.event else {
                continue;
            };
            if t.mayhem_mode == Some(true) {
                mayhem += 1;
                assert_eq!(t.buyback_fee_basis_points, Some(0), "{name}");
                assert_eq!(t.buyback_fee, Some(0), "{name}");
                let fr = t.fee_recipient.unwrap();
                assert!(
                    fr == pump_codec::Pubkey::default() || c::RESERVED_FEE_RECIPIENTS.contains(&fr),
                    "{name}"
                );
            } else {
                assert_eq!(
                    t.buyback_fee_basis_points,
                    Some(pump_codec::curve::DEFAULT_BUYBACK_BPS),
                    "{name}"
                );
            }
        }
    }
    // 3 mayhem trades in the fixtures (legacy buy, buy_exact_sol_in, v3 exact-in).
    assert_eq!(mayhem, 3);
}
