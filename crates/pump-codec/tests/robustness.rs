//! Decoders must never panic: mutate every fixture and feed random bytes to
//! every entry point (debug build, so integer overflow would panic too).

mod common;

use common::*;
use pump_codec::constants as c;
use pump_codec::curve::{BondingCurve, CurveState, FeeBps, FeeConfig, Global};
use pump_codec::events::{
    decode_cpi_event, decode_event_payload, scan_program_data_logs, EventSource,
};
use pump_codec::instructions::classify;
use pump_codec::tx::TxView;
use pump_codec::Pubkey;

/// Iterations per fixture; raise with `PUMP_CODEC_FUZZ_ITERS=20000` for a deeper run.
fn iters(default: usize) -> usize {
    std::env::var("PUMP_CODEC_FUZZ_ITERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Seed; vary with `PUMP_CODEC_FUZZ_SEED`.
fn seed(default: u64) -> u64 {
    std::env::var("PUMP_CODEC_FUZZ_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
        | 1
}

/// xorshift64*: deterministic, dependency-free.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn exercise_tx(raw: &[u8]) {
    if let Ok(tx) = TxView::parse(raw) {
        let _ = tx.compute_budget();
        let _ = tx.address_table_lookups().count();
        let _ = tx.transaction_id();
        for ix in tx.instructions() {
            let a = tx.instruction_accounts(&ix, None);
            let _ = classify(tx.program_id(&ix), ix.data, &a);
            let _ = classify(&c::PUMP_PROGRAM_ID, ix.data, &a);
            let _ = classify(&c::PUMP_SWAP_PROGRAM_ID, ix.data, &a);
            let _ = decode_cpi_event(&c::PUMP_PROGRAM_ID, ix.data);
        }
        for i in 0..tx.total_account_count() + 2 {
            let _ = (tx.is_writable(i), tx.is_signer(i));
        }
    }
}

#[test]
fn mutated_transactions_never_panic() {
    let mut rng = Rng(seed(0x9E37_79B9_7F4A_7C15));
    for name in tx_names() {
        let fx = TxFixture::load(&name);
        exercise_tx(&fx.raw);
        for _ in 0..iters(300) {
            let mut m = fx.raw.clone();
            for _ in 0..1 + rng.below(4) {
                let i = rng.below(m.len());
                m[i] = rng.next() as u8;
            }
            if rng.below(4) == 0 {
                let cut = rng.below(m.len());
                m.truncate(cut);
            }
            exercise_tx(&m);
        }
    }
}

#[test]
fn mutated_instruction_and_event_data_never_panic() {
    let mut rng = Rng(seed(42));
    let keys: Vec<Pubkey> = (0..40).map(|_| Pubkey::new_unique()).collect();
    for name in tx_names() {
        let fx = TxFixture::load(&name);
        let tx = fx.view();
        let mut blobs: Vec<Vec<u8>> = tx.instructions().map(|i| i.data.to_vec()).collect();
        blobs.extend(fx.inner.iter().map(|(_, _, _, d)| d.clone()));
        for b in blobs {
            for _ in 0..iters(300) / 8 {
                let mut m = b.clone();
                if !m.is_empty() {
                    let i = rng.below(m.len());
                    m[i] = rng.next() as u8;
                    if rng.below(3) == 0 {
                        m.truncate(rng.below(m.len()));
                    }
                }
                let n = rng.below(keys.len());
                let _ = classify(&c::PUMP_PROGRAM_ID, &m, &keys[..n]);
                let _ = classify(&c::PUMP_SWAP_PROGRAM_ID, &m, &keys[..n]);
                let _ = decode_cpi_event(&c::PUMP_PROGRAM_ID, &m);
                let _ = decode_cpi_event(&c::PUMP_SWAP_PROGRAM_ID, &m);
                if m.len() > 8 {
                    let _ = decode_event_payload(EventSource::Pump, &m[8..]);
                    let _ = decode_event_payload(EventSource::PumpSwap, &m[8..]);
                    if let Ok(pump_codec::events::PumpEvent::Trade(t)) =
                        decode_event_payload(EventSource::Pump, &m[8..])
                    {
                        let _ = (t.pre_state(), t.post_state());
                    }
                }
            }
        }
        let _ = scan_program_data_logs(&fx.logs);
        let garbled: Vec<String> = fx.logs.iter().map(|l| l.replace('A', "!")).collect();
        let _ = scan_program_data_logs(&garbled);
    }
}

#[test]
fn random_account_data_and_math_never_panic() {
    let mut rng = Rng(seed(7));
    for len in [0usize, 7, 8, 9, 40, 49, 81, 150, 300, 800] {
        for _ in 0..200 {
            let mut d: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
            let _ = Global::decode(&d);
            let _ = BondingCurve::decode(&d);
            let _ = FeeConfig::decode(&d);
            if d.len() >= 8 {
                d[..8].copy_from_slice(&c::account::BONDING_CURVE);
                let _ = BondingCurve::decode(&d);
                d[..8].copy_from_slice(&c::account::FEE_CONFIG);
                let _ = FeeConfig::decode(&d);
                d[..8].copy_from_slice(&c::account::GLOBAL);
                let _ = Global::decode(&d);
            }
        }
    }
    for _ in 0..iters(300) * 60 {
        let s = CurveState {
            virtual_sol: rng.next() >> rng.below(64),
            virtual_token: rng.next() >> rng.below(64),
            real_sol: rng.next() >> rng.below(64),
            real_token: rng.next() >> rng.below(64),
            complete: false,
            creator: Pubkey::new_from_array([1; 32]),
        };
        let fees = FeeBps {
            protocol: rng.next() >> rng.below(64),
            creator: rng.next() >> rng.below(64),
        };
        let x = rng.next() >> rng.below(64);
        let _ = s.tokens_for_sol(x);
        let _ = s.buy_cost(x);
        let _ = s.sell_sol_out(x);
        let _ = s.quote_buy_exact_tokens(x, fees);
        let _ = s.quote_buy_exact_sol_in(x, fees);
        let _ = s.quote_sell(x, fees);
        let _ = s.market_cap_lamports(x);
        let _ = s.buy_price_impact(x);
        let _ = s.sol_to_complete();
        let _ = s.progress_bps();
        let _ = CurveState::pre_trade(&s, rng.below(2) == 0, x, rng.next());
        let mut m = s;
        m.apply_buy(x, rng.next());
        m.apply_sell(x, rng.next());
        let _ = s.simulate_buys(&[x, x / 2], FeeBps::DEFAULT);
    }
}
