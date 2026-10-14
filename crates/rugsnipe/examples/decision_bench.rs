//! Cost of the decision-path pieces, back to back and with idle gaps
//! (cold caches): `cargo run --release -p rugsnipe --example decision_bench`.

use pump_codec::constants::WSOL_MINT;
use pump_codec::{pda, Pubkey};
use rugsnipe_feed::clock::mono_ns;
use std::time::Duration;

fn q(v: &mut [u64]) -> (u64, u64, u64) {
    v.sort_unstable();
    (v[v.len() / 2], v[v.len() * 9 / 10], v[v.len() * 99 / 100])
}

fn main() {
    let signer = rugsnipe::setup::throwaway_signer().unwrap();
    let owner = signer.pubkey();
    let msg = [7u8; 700];
    if let Some(c) = core_affinity::get_core_ids().and_then(|i| i.last().copied()) {
        core_affinity::set_for_current(c);
    }
    for (gap_us, spin) in [
        (0u64, false),
        (200, false),
        (2_000, false),
        (200, true),
        (2_000, true),
    ] {
        let (mut pda_ns, mut sign_ns) = (vec![], vec![]);
        for _ in 0..300 {
            let mint = Pubkey::new_unique();
            let bc = pda::bonding_curve(&mint);
            if gap_us > 0 && !spin {
                std::thread::sleep(Duration::from_micros(gap_us));
            } else if gap_us > 0 {
                let until = mono_ns() + gap_us * 1_000;
                while mono_ns() < until {
                    std::hint::spin_loop();
                }
            }
            let t0 = mono_ns();
            let a = pda::ata_spl(&bc, &WSOL_MINT);
            let b = pda::ata_token_2022(&owner, &mint);
            let t1 = mono_ns();
            let s = signer.sign(&msg);
            let t2 = mono_ns();
            std::hint::black_box((a, b, s));
            pda_ns.push(t1 - t0);
            sign_ns.push(t2 - t1);
        }
        println!(
            "gap {gap_us:>5} us spin={spin}: 2 ATA PDAs p50/p90/p99 {:?} ns, ed25519 sign {:?} ns",
            q(&mut pda_ns),
            q(&mut sign_ns)
        );
    }
}
