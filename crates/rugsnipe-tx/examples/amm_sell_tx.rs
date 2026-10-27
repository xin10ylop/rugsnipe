//! The PumpSwap canonical-pool sell the hot loop sends after a coin
//! graduates, unsigned, as base64 (for `simulateTransaction` with
//! `sigVerify: false`, `replaceRecentBlockhash: true`):
//!
//!     cargo run -p rugsnipe-tx --example amm_sell_tx -- USER MINT TOKENS MIN_OUT [TOKEN_PROGRAM]
use base64::Engine as _;
use pump_codec::constants as c;
use pump_codec::Pubkey;
use rugsnipe_core::config::TxFormat;
use rugsnipe_tx::template::*;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let pk = |s: &str| s.parse::<Pubkey>().expect("pubkey");
    let (user, mint) = (pk(&a[0]), pk(&a[1]));
    let (tokens, min_out): (u64, u64) = (a[2].parse().unwrap(), a[3].parse().unwrap());
    let base_tp = a.get(4).map_or(c::TOKEN_2022_PROGRAM_ID, |s| pk(s));
    let p = AmmSellParams {
        format: TxFormat::Legacy,
        user,
        nonce: None,
        cu_limit: 150_000,
        lads_bytes: 16_000_000,
        tip_account: None,
        buyback_wallet: c::BUYBACK_FEE_RECIPIENTS[0],
        base_token_program: base_tp,
    };
    let mut t = Template::amm_sell(&p).expect("template");
    t.patch(&Patch {
        keys: &amm_mint_keys(&mint, &user, &base_tp),
        lifetime: &[1u8; 32],
        amount: tokens,
        limit: min_out,
        priority_lamports: 10_000,
        tip_lamports: 0,
        cu_limit: None,
    });
    println!(
        "{}",
        base64::engine::general_purpose::STANDARD.encode(t.bytes())
    );
}
