//! Rough timings, not a benchmark harness. Run with
//! `cargo test -p pump-codec --release --test perf_smoke -- --ignored --nocapture`.

mod common;

use common::*;
use pump_codec::constants as c;
use pump_codec::events::events_from_inner_instructions;
use pump_codec::instructions::{self as ixs, classify, Classified, PumpInstruction, SolV3Keys};
use pump_codec::tx::TxView;
use std::hint::black_box;
use std::time::Instant;

fn time<F: FnMut()>(label: &str, n: u32, mut f: F) {
    for _ in 0..n / 10 {
        f();
    }
    let t = Instant::now();
    for _ in 0..n {
        f();
    }
    eprintln!(
        "{label:<55} {:>9.0} ns/iter",
        t.elapsed().as_nanos() as f64 / n as f64
    );
}

#[test]
#[ignore]
fn perf_smoke() {
    let fx = TxFixture::load("v1_create_v2_buy");
    let raw = fx.raw.clone();
    time("TxView::parse (v1 create+2 buys, 1945 B)", 200_000, || {
        black_box(TxView::parse(black_box(&raw)).unwrap());
    });
    time("parse + find create_v2 (mint, user, args)", 200_000, || {
        let tx = TxView::parse(black_box(&raw)).unwrap();
        for ix in tx.instructions() {
            if *tx.program_id(&ix) != c::PUMP_PROGRAM_ID {
                continue;
            }
            let a = tx.instruction_accounts(&ix, None);
            if let Ok(Classified::Pump(PumpInstruction::Create(cr))) =
                classify(tx.program_id(&ix), ix.data, &a)
            {
                black_box(cr);
                break;
            }
        }
    });
    let keys = fx.full_keys();
    time("events_from_inner_instructions (3 events)", 200_000, || {
        black_box(events_from_inner_instructions(&keys, fx.inner_instructions(), true).unwrap());
    });
    let mint = keys[1];
    let user = keys[0];
    time("SolV3Keys::derive (6 PDA searches)", 5_000, || {
        black_box(SolV3Keys::derive(
            black_box(mint),
            user,
            c::TOKEN_2022_PROGRAM_ID,
            c::BUYBACK_FEE_RECIPIENTS[0],
        ));
    });
    let uk = ixs::UserKeys::derive(user);
    let tx = TxView::parse(&raw).unwrap();
    let create = tx
        .instructions()
        .find_map(|ix| {
            match classify(
                tx.program_id(&ix),
                ix.data,
                &tx.instruction_accounts(&ix, None),
            ) {
                Ok(Classified::Pump(PumpInstruction::Create(cr))) => Some(cr),
                _ => None,
            }
        })
        .unwrap();
    time("SolV3Keys::from_create (2 PDA searches)", 5_000, || {
        black_box(
            SolV3Keys::from_create(black_box(&create), &uk, c::BUYBACK_FEE_RECIPIENTS[0]).unwrap(),
        );
    });
    let k = SolV3Keys::derive(
        create.mint.unwrap(),
        user,
        c::TOKEN_2022_PROGRAM_ID,
        c::BUYBACK_FEE_RECIPIENTS[0],
    );
    assert_eq!(
        SolV3Keys::from_create(&create, &uk, c::BUYBACK_FEE_RECIPIENTS[0]).unwrap(),
        k
    );
    time("buy_v3 Instruction from cached keys", 500_000, || {
        black_box(ixs::buy_v3(
            black_box(&k),
            1_000_000,
            10_000_000,
            Some(true),
        ));
    });
    let mut buf = [0u8; 25];
    time(
        "encode_u64_u64_opt_bool (buy_v3 data in place)",
        2_000_000,
        || {
            black_box(ixs::encode_u64_u64_opt_bool(
                &mut buf,
                c::ix::BUY_V3,
                black_box(1),
                2,
                Some(true),
            ));
        },
    );
}
