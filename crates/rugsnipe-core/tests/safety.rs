//! A kill file stops buys (and the latch survives the file's removal);
//! every decision has an rslog record.

use rugsnipe_core::config::RiskCfg;
use rugsnipe_core::kill::KillFile;
use rugsnipe_core::risk::*;
use rugsnipe_core::rslog::{decode_frames, Rec};
use rugsnipe_core::{IntentId, Mode, Pubkey};

fn gate() -> RiskGate {
    let g = RiskGate::new(RiskCfg {
        max_snipe_lamports: 250_000_000,
        max_probe_lamports: 20_000_000,
        max_probes_per_day: 20,
        max_open_positions: 4,
        max_daily_loss_lamports: 1_000_000_000,
        max_consec_losses_per_op: 5,
        max_buys_per_op_per_day: 10,
        max_buys_per_hour: 20,
        max_bid_lamports: 25_000_000,
        max_sell_spend_lamports: 20_000_000,
        max_entry_vsol_lamports: 40_000_000_000,
        max_hot_balance_lamports: 5_000_000_000,
        min_hot_balance_lamports: 1_000_000_000,
        ..Default::default()
    });
    g.set_hot_balance(2_000_000_000);
    g
}

fn input() -> DecideInput {
    DecideInput {
        mode: Mode::Live,
        op_slot: 0,
        op_armed: true,
        armset_ok: true,
        frozen: false,
        spend: 50_000_000,
        bid: 1_000_000,
        v_entry: 31_000_000_000,
        now_unix: 1_791_968_762,
    }
}

#[test]
fn kill_file_stops_buys() {
    let path = std::env::temp_dir().join(format!("rugsnipe-KILL-{}", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let kf = KillFile::new(&path);
    let g = gate();
    assert!(!kf.poll(&g));
    assert_eq!(g.decide(&input()), Decision::Fire);
    std::fs::write(&path, b"").unwrap();
    assert!(kf.poll(&g));
    assert_eq!(g.kill_reason(), Some(KillReason::KillFile));
    assert_eq!(g.decide(&input()), Decision::Skip(SkipReason::Killed));
    std::fs::remove_file(&path).unwrap();
    assert!(kf.poll(&g), "removing the file does not unkill");
    assert_eq!(g.decide(&input()), Decision::Skip(SkipReason::Killed));
    g.unkill_local();
    assert_eq!(g.decide(&input()), Decision::Fire);
}

#[test]
fn every_decision_becomes_a_record() {
    let g = gate();
    let mint = Pubkey::new_unique();
    let id = IntentId::new(&mint, &Pubkey::new_unique());
    let mut bytes = vec![];
    for (i, mode) in [Mode::Paper, Mode::Live, Mode::Live]
        .into_iter()
        .enumerate()
    {
        let mut inp = input();
        inp.mode = mode;
        if i == 2 {
            inp.bid = 100_000_000;
        }
        let d = g.decide(&inp);
        let would = match d {
            Decision::Paper {
                would_skip: Some(r),
                ..
            } => r as u8,
            _ => 0,
        };
        Rec::Decision {
            intent: id.0,
            mint,
            mode: mode.as_u8(),
            code: d.code(),
            would_skip: would,
            size: inp.spend,
            v_max: 40_000_000_000,
            min_tokens: 1,
            nonce_idx: 0,
            decided_ns: 0,
        }
        .encode_frame(i as u64, 0, 1, &mut bytes);
    }
    let (frames, _) = decode_frames(&bytes);
    let codes: Vec<u8> = frames
        .iter()
        .map(|(_, r)| {
            if let Rec::Decision { code, .. } = r {
                *code
            } else {
                255
            }
        })
        .collect();
    assert_eq!(
        codes,
        vec![
            0x10 | PaperReason::ModePaper as u8,
            0,
            0x20 | SkipReason::BidCap as u8
        ]
    );
}
