//! `create_hit` (wire), `from_logs` (logsSubscribe), `pump_buys` owners and
//! `transfers` on real mainnet fixtures. Oracle: the fixtures' own
//! `TradeEvent`s / `CreateEvent` (self-CPI inner instructions in meta).

mod common;

use common::*;
use pump_codec::create_hit::{create_hit, from_logs, pump_buys, HitError, HitSource};
use pump_codec::curve::FeeBps;
use pump_codec::events::{events_from_inner_instructions, PumpEvent};
use pump_codec::launchers::{Launcher, KNOWN_LAUNCHERS};
use pump_codec::transfers::{transfers, TokenProgram};
use pump_codec::Pubkey;

/// (sum sol, sum tokens, n, buyers) of buy TradeEvents of `mint` in meta.
fn event_buys(fx: &TxFixture, mint: &Pubkey) -> (u64, u64, u8, Vec<Pubkey>) {
    let recs =
        events_from_inner_instructions(&fx.full_keys(), fx.inner_instructions(), true).unwrap();
    let (mut s, mut t, mut n, mut u) = (0u64, 0u64, 0u8, vec![]);
    for r in recs {
        if let PumpEvent::Trade(ev) = r.event {
            if ev.mint == *mint && ev.is_buy {
                s += ev.sol_amount;
                t += ev.token_amount;
                n += 1;
                u.push(ev.user);
            }
        }
    }
    (s, t, n, u)
}

fn create_event_mint(fx: &TxFixture) -> Option<Pubkey> {
    let recs =
        events_from_inner_instructions(&fx.full_keys(), fx.inner_instructions(), true).ok()?;
    recs.into_iter().find_map(|r| match r.event {
        PumpEvent::Create(c) => Some(c.mint),
        _ => None,
    })
}

fn reserves(s: Option<pump_codec::curve::CurveState>) -> Option<(u64, u64, u64, u64)> {
    s.map(|s| (s.virtual_sol, s.virtual_token, s.real_sol, s.real_token))
}

fn ok(fx: &TxFixture) -> bool {
    fx.result["meta"]["err"].is_null()
}

#[test]
fn three_signer_create_sums_both_buys_and_lists_second_buyer() {
    let fx = TxFixture::load("v1_create_v2_buy");
    let tx = fx.view();
    assert_eq!(tx.num_signatures(), 3);
    let wire = create_hit(&tx, None, &[], FeeBps::DEFAULT)
        .unwrap()
        .unwrap();
    let (s, t, n, users) = event_buys(&fx, &wire.mint);
    assert_eq!(n, 2, "dev legacy buy + v3 buy");
    assert_eq!(wire.intx_buys, 2);
    // wire-only simulation reproduces the events to the lamport
    assert!(wire.intx_exact);
    assert_eq!((wire.intx_sol_in, wire.intx_tokens_out), (s, t));
    let second = wire
        .intx_buyers
        .as_slice()
        .iter()
        .find(|k| k.to_string().starts_with("GGd2xV"))
        .copied()
        .expect("second buyer GGd2xV… listed");
    assert!(users.contains(&second));
    assert!(wire.signers.contains(&second));
    assert_eq!(wire.signers.len(), 3);
    assert_eq!(wire.fee_payer, Some(*tx.fee_payer()));
    assert_eq!(wire.launcher, Launcher::Direct);
    assert!(
        wire.aqbc_hint.is_some(),
        "the v3 buy names the curve quote ATA"
    );
    assert_eq!(
        wire.aqbc_hint.unwrap(),
        pump_codec::pda::ata_spl(&wire.bc, &pump_codec::constants::WSOL_MINT)
    );
    // with meta: events are the truth, same numbers
    let full = create_hit(
        &tx,
        Some(&fx.loaded),
        &fx.inner_instructions(),
        FeeBps::DEFAULT,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        (full.intx_sol_in, full.intx_tokens_out, full.intx_buys),
        (s, t, 2)
    );
    assert_eq!(reserves(full.curve_after), reserves(wire.curve_after));
    // this fixture's logs are truncated before the trades: the logs path says so
    let lh = from_logs(&fx.signature, &fx.logs).unwrap().unwrap();
    assert!(lh.logs_truncated && !lh.intx_exact);
    assert_eq!(lh.mint, wire.mint);
}

#[test]
fn v3_buy_credits_a_third_party_owner() {
    let fx = TxFixture::load("v1_buy_exact_quote_in_v3");
    let tx = fx.view();
    let buys = pump_buys(&tx, Some(&fx.loaded), &[]).unwrap();
    assert_eq!(buys.len(), 1);
    let b = buys[0];
    assert!(
        b.user.unwrap().to_string().starts_with("8Z2tbj"),
        "{:?}",
        b.user
    );
    let owner = b.owner.expect("owner from the ATA create");
    assert!(owner.to_string().starts_with("5aR7Xt"), "{owner}");
    assert_ne!(Some(owner), b.user);
    assert_eq!(
        b.token_account.unwrap(),
        pump_codec::pda::ata_token_2022(&owner, &b.mint.unwrap())
    );
    // not a create
    assert!(create_hit(&tx, None, &[], FeeBps::DEFAULT)
        .unwrap()
        .is_none());
}

#[test]
fn every_create_fixture_matches_its_events() {
    let mut creates = 0;
    let mut exact_wire = 0;
    for name in tx_names() {
        let fx = TxFixture::load(&name);
        if !ok(&fx) {
            continue;
        }
        let Some(ev_mint) = create_event_mint(&fx) else {
            continue;
        };
        creates += 1;
        let tx = fx.view();
        let inner = fx.inner_instructions();
        let full = create_hit(&tx, Some(&fx.loaded), &inner, FeeBps::DEFAULT)
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .unwrap_or_else(|| panic!("{name}: no hit"));
        assert_eq!(full.mint, ev_mint, "{name}");
        assert_eq!(full.source, HitSource::Wire);
        let (s, t, n, users) = event_buys(&fx, &ev_mint);
        assert_eq!(
            (full.intx_sol_in, full.intx_tokens_out),
            (s, t),
            "{name}: meta totals"
        );
        assert_eq!(full.intx_buys, n, "{name}: buy count");
        for u in users.iter().take(2) {
            assert!(
                full.intx_buyers.contains(u) || full.intx_buyers.overflowed(),
                "{name}"
            );
        }
        // wire-only: either a CPI create (needs meta) or a hit whose
        // simulation, when it claims exactness, equals the events.
        match create_hit(&tx, Some(&fx.loaded), &[], FeeBps::DEFAULT) {
            Ok(Some(w)) => {
                assert_eq!(w.mint, ev_mint, "{name}");
                if w.intx_exact {
                    exact_wire += 1;
                    assert_eq!(
                        (w.intx_sol_in, w.intx_tokens_out),
                        (s, t),
                        "{name}: simulation"
                    );
                    assert_eq!(w.intx_buys, n, "{name}");
                }
                if w.mayhem || !w.is_sol_quote() {
                    assert!(!w.intx_exact, "{name}");
                }
            }
            Err(HitError::CpiCreateNeedsMeta { launcher }) => {
                assert!(!full.launcher.is_direct(), "{name}");
                assert_eq!(launcher, full.launcher, "{name}");
            }
            other => panic!("{name}: {other:?}"),
        }
    }
    assert!(creates >= 15, "only {creates} create fixtures");
    assert!(exact_wire >= 8, "only {exact_wire} exact wire simulations");
}

#[test]
fn launcher_cpi_create_needs_meta_then_resolves() {
    let fx = TxFixture::load("fresh_v1_cpi_create_v2_bexq_v2");
    let tx = fx.view();
    let six = Launcher::from_program(&pk("6Vo3245eszAb5wuqEMw8mGdbfRUdKbHhDHP5LcaGuTAB"));
    assert!(matches!(six, Launcher::Known(_)));
    assert_eq!(KNOWN_LAUNCHERS.len(), 6);
    match create_hit(&tx, Some(&fx.loaded), &[], FeeBps::DEFAULT) {
        Err(HitError::CpiCreateNeedsMeta { launcher }) => assert_eq!(launcher, six),
        other => panic!("{other:?}"),
    }
    let h = create_hit(
        &tx,
        Some(&fx.loaded),
        &fx.inner_instructions(),
        FeeBps::DEFAULT,
    )
    .unwrap()
    .unwrap();
    assert_eq!(h.launcher, six);
    assert_eq!(h.launcher.name(), "6Vo3245");
    assert!(h.intx_buys >= 1);
    // logs path sees the same launcher (create logged at depth 2)
    let l = from_logs(&fx.signature, &fx.logs).unwrap().unwrap();
    assert_eq!(l.launcher, six);
    assert_eq!(l.mint, h.mint);
}

#[test]
fn logs_path_equals_wire_path() {
    let mut compared = 0;
    for name in tx_names() {
        let fx = TxFixture::load(&name);
        if !ok(&fx) || create_event_mint(&fx).is_none() {
            continue;
        }
        let Some(l) = from_logs(&fx.signature, &fx.logs).unwrap() else {
            panic!("{name}: logs path found no create");
        };
        let w = create_hit(
            &fx.view(),
            Some(&fx.loaded),
            &fx.inner_instructions(),
            FeeBps::DEFAULT,
        )
        .unwrap()
        .unwrap();
        assert_eq!(l.source, HitSource::Logs);
        assert_eq!(l.sig, w.sig, "{name}");
        assert_eq!(
            (l.mint, l.bc, l.abc, l.user, l.token_prog, l.holder_reward),
            (w.mint, w.bc, w.abc, w.user, w.token_prog, w.holder_reward),
            "{name}"
        );
        if !w.holder_reward {
            assert_eq!(l.creator_arg, w.creator_arg, "{name}");
        }
        assert_eq!(
            (l.mayhem, l.cashback, l.launcher),
            (w.mayhem, w.cashback, w.launcher),
            "{name}"
        );
        if w.quote.is_some() {
            assert_eq!(l.quote, w.quote, "{name}");
        }
        assert_eq!(l.fee_payer, None);
        if l.logs_truncated {
            continue;
        }
        compared += 1;
        assert_eq!(
            (l.intx_sol_in, l.intx_tokens_out, l.intx_buys),
            (w.intx_sol_in, w.intx_tokens_out, w.intx_buys),
            "{name}"
        );
        if w.intx_exact && l.intx_exact {
            assert_eq!(reserves(l.curve_after), reserves(w.curve_after), "{name}");
        }
        // every logs buyer (a buy signer) is a wire buyer
        for b in l.intx_buyers.as_slice() {
            assert!(
                w.intx_buyers.contains(b) || w.intx_buyers.overflowed(),
                "{name}"
            );
        }
    }
    assert!(compared >= 8, "only {compared} untruncated create fixtures");
}

#[test]
fn transfers_in_a_buy_fixture() {
    let fx = TxFixture::load("v0_create_v2_buy_v3");
    let tx = fx.view();
    let ts = transfers(&tx, Some(&fx.loaded), &fx.inner_instructions());
    let mint = create_event_mint(&fx).unwrap();
    // the buy moves tokens curve -> buyer with Token-2022 TransferChecked (CPI)
    let t = ts
        .iter()
        .find(|t| t.checked && t.mint == Some(mint))
        .expect("TransferChecked of the new mint");
    assert_eq!(t.program, TokenProgram::Token2022);
    assert!(t.inner_index.is_some());
    assert_eq!(t.decimals, Some(6));
    let (_, tokens, _, _) = event_buys(&fx, &mint);
    let total: u64 = ts
        .iter()
        .filter(|t| t.mint == Some(mint))
        .map(|t| t.amount)
        .sum();
    assert_eq!(total, tokens);
    // pre-execution bytes carry no transfers (all CPIs)
    assert!(transfers(&tx, Some(&fx.loaded), &[])
        .iter()
        .all(|t| t.inner_index.is_none()));
}

#[test]
fn a_dev_buy_by_cpi_from_the_launch_tool_is_hidden_from_the_wire_and_shown_by_the_meta() {
    // m.rapidlaunch.io (2026-10-25): a top-level create_v2, the dev buy by
    // CPI from the tool's own program. The shreds copy (no meta) finds no
    // buy and cannot be exact; the executed copy has the TradeEvent.
    let fx = TxFixture::load("rapidlaunch_create_v2_cpi_dev_buy");
    let tx = fx.view();
    let wire = create_hit(&tx, None, &[], FeeBps::DEFAULT)
        .unwrap()
        .unwrap();
    assert_eq!(wire.launcher, Launcher::Direct);
    assert_eq!((wire.intx_sol_in, wire.intx_exact), (0, false));
    assert_eq!(wire.tx_shape.uri_host(), Some("m.rapidlaunch.io"));
    let (s, _, n, _) = event_buys(&fx, &wire.mint);
    assert_eq!((s, n), (2_962_962_962, 1), "the dev's executed buy");
}
