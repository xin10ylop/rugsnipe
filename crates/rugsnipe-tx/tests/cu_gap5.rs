//! The per-mint CU formula against the 361 successful SOL-quoted mainnet
//! simulations of docs/research/gap-5.md section 1
//! (`analysis/scripts/gap5/sweep_nonce.json`: v1 `[AdvanceNonce,
//! CreateIdempotent(T22/SPL), buy_v3, tip]`, user `EW4dq…Tk2`).
//!
//! For every record: our hash-only bump recovery and the full PDA search
//! agree with the recorded `bc`/`abc`/`uata` bumps; the curve's wSOL ATA
//! bump (`aqbc`, not recorded) is derived here; the gap-5 model
//! `62,310 + 1,500·S − 409·mayhem` reproduces `unitsConsumed` within
//! ±300 CU; and the per-mint limit `65,000 + 1,500·S` covers the measured
//! cost plus the template's +1,185 (buy_exact_quote_in_v3) with room left.

use pump_codec::constants as c;
use pump_codec::{pda, Pubkey};
use rugsnipe_tx::cu::{buy_v3_cu_model, snipe_cu_expected, snipe_cu_limit};
use rugsnipe_tx::template::MintKeys;
use serde_json::Value;

const USER: &str = "EW4dqBdFKBjjfTkdX1XeR9BnfrL1sUivVNaTJVJ18Tk2";

fn records() -> Vec<Value> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../analysis/scripts/gap5/sweep_nonce.json"
    );
    let text = std::fs::read_to_string(path).expect("gap-5 sweep data");
    serde_json::from_str::<Vec<Value>>(&text).unwrap()
}

#[test]
fn formula_reproduces_361_gap5_simulations() {
    let user: Pubkey = USER.parse().unwrap();
    // the gap-5 wallet's UVA bump is 255: no wallet term in this fit
    assert_eq!(rugsnipe_tx::cu::wallet_missing_bumps(&user), 0);
    let mut n = 0;
    let (mut lo, mut hi) = (i64::MAX, i64::MIN);
    let mut max_s = 0;
    let mut min_headroom = i64::MAX;
    for r in records() {
        if !r["err"].is_null() {
            continue;
        }
        let Some(units) = r["units"].as_u64() else {
            continue;
        };
        let mint: Pubkey = r["mint"].as_str().unwrap().parse().unwrap();
        let tp: Pubkey = r["curve"]["tp"].as_str().unwrap().parse().unwrap();
        assert!(tp == c::TOKEN_2022_PROGRAM_ID || tp == c::SPL_TOKEN_PROGRAM_ID);
        let mayhem = r["curve"]["mayhem"].as_u64().unwrap() == 1;
        let b = &r["bumps"];
        let rec = |k: &str| b[k].as_u64().unwrap() as u8;
        // full PDA search == recorded bumps
        let full = pda::SnipeBumps::derive(&mint, &user, &tp);
        assert_eq!(
            (full.bc, full.abc, full.user_ata),
            (rec("bc"), rec("abc"), rec("uata")),
            "{mint}"
        );
        // hot path: hash-only recovery from the keys
        let bc = pda::bonding_curve(&mint);
        let keys = MintKeys::curve(
            mint,
            bc,
            pda::associated_token_address(&bc, &mint, &tp),
            pda::ata_spl(&bc, &c::WSOL_MINT),
            pda::associated_token_address(&user, &mint, &tp),
        );
        let hot = keys.snipe_bumps(&user, &tp).unwrap();
        assert_eq!(hot, full, "{mint}");
        let s = hot.missing();
        max_s = max_s.max(s);
        let resid = units as i64 - buy_v3_cu_model(s, true, mayhem) as i64;
        lo = lo.min(resid);
        hi = hi.max(resid);
        assert!(
            resid.abs() <= 300,
            "{mint}: units {units} S {s} mayhem {mayhem} residual {resid}"
        );
        // the limit covers this mint's cost on our template (+1,185)
        let limit = snipe_cu_limit(s, 0);
        assert_eq!(limit, 65_000 + 1_500 * s);
        assert_eq!(keys.snipe_cu_limit(&user, &tp, 0, 0), Some(limit));
        assert!(snipe_cu_expected(s) as u64 >= units + 1_185);
        min_headroom = min_headroom.min(limit as i64 - (units as i64 + 1_185));
        n += 1;
    }
    assert_eq!(n, 361, "successful SOL-quoted simulations");
    assert_eq!(max_s, 14, "gap-5: max S = 14");
    assert!(lo >= -300 && hi <= 300, "residual range {lo}..{hi}");
    assert!(min_headroom >= 1_500, "headroom {min_headroom}");
    eprintln!(
        "gap-5 fit: n={n} residual {lo}..{hi} CU, max S {max_s}, min headroom {min_headroom} CU"
    );
}
