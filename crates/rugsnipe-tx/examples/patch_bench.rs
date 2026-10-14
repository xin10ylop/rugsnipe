//! `cargo run --release -p rugsnipe-tx --example patch_bench`: time the hot
//! path from per-mint keys to signed bytes (patch + ed25519 sign + base64),
//! and the two ATA PDA searches that precede it.

use pump_codec::constants as c;
use pump_codec::instructions::SolV3Keys;
use pump_codec::{pda, Pubkey};
use rugsnipe_core::config::TxFormat;
use rugsnipe_tx::signer::HotSigner;
use rugsnipe_tx::template::*;
use std::time::Instant;
use zeroize::Zeroizing;

fn pct(v: &mut [u64], p: f64) -> u64 {
    v.sort_unstable();
    v[((v.len() - 1) as f64 * p) as usize]
}

fn main() {
    let payer = HotSigner::from_seed(&Zeroizing::new([7u8; 32]));
    for format in [TxFormat::Legacy, TxFormat::V1] {
        let p = CurveParams {
            format,
            user: payer.pubkey(),
            nonce: Some(NonceKeys {
                account: Pubkey::new_unique(),
                authority: payer.pubkey(),
            }),
            cu_limit: 72_150,
            lads_bytes: 12_124_160,
            tip_account: Some(c::JITO_TIP_ACCOUNTS[0]),
            buyback_fee_recipient: c::BUYBACK_FEE_RECIPIENTS[0],
            base_token_program: c::TOKEN_2022_PROGRAM_ID,
            partial_fill: Some(false),
        };
        let mut t = Template::buy(&p).unwrap();
        let mints: Vec<Pubkey> = (0..2_000).map(|_| Pubkey::new_unique()).collect();
        let keys: Vec<MintKeys> = mints
            .iter()
            .map(|m| {
                MintKeys::from_v3(&SolV3Keys::derive(
                    *m,
                    p.user,
                    p.base_token_program,
                    p.buyback_fee_recipient,
                ))
            })
            .collect();
        let (mut patch_sign, mut pdas) = (vec![], vec![]);
        let mut b64 = vec![0u8; 2_000];
        for (m, k) in mints.iter().zip(&keys) {
            let t0 = Instant::now();
            let bc = pda::bonding_curve(m);
            let _aqbc = pda::ata_spl(&bc, &c::WSOL_MINT);
            let _ata = pda::ata_token_2022(&payer.pubkey(), m);
            pdas.push(t0.elapsed().as_nanos() as u64);
            let t1 = Instant::now();
            t.patch(&Patch {
                keys: k,
                lifetime: &[3; 32],
                amount: 50_000_000,
                limit: 1,
                priority_lamports: 7_200,
                tip_lamports: 1_000_000,
                cu_limit: None,
            });
            t.sign(&payer, None);
            use base64::Engine as _;
            let n = base64::engine::general_purpose::STANDARD
                .encode_slice(t.bytes(), &mut b64)
                .unwrap();
            std::hint::black_box(n);
            patch_sign.push(t1.elapsed().as_nanos() as u64);
        }
        println!(
            "{format:?}+nonce {} B: patch+sign+base64 p50 {:.1} us p99 {:.1} us | bc+aqbc+ata PDA p50 {:.1} us",
            t.len(),
            pct(&mut patch_sign, 0.5) as f64 / 1e3,
            pct(&mut patch_sign, 0.99) as f64 / 1e3,
            pct(&mut pdas, 0.5) as f64 / 1e3
        );
    }
}
