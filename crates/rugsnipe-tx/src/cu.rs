//! Per-mint compute-unit limit of the snipe (docs/research/gap-5.md
//! section 1, docs/03 section 6).
//!
//! The snipe's compute cost is deterministic per mint. pump and the ATA
//! program re-derive four PDAs on chain with `find_program_address`, which
//! costs 1,500 CU for every bump below 255:
//!
//! `S = Σ (255 - bump)` over `bonding_curve`, the curve's base ATA, the
//! curve's wSOL ATA and the user's base ATA ([`pump_codec::pda::SnipeBumps`]).
//!
//! * gap-5 envelope (v1 + nonce, `buy_v3`, 361 mainnet simulations):
//!   `62,310 + 1,500·S` (`- 409` on mayhem coins), residual −263/+38 CU;
//! * our template (v1 + nonce, `buy_exact_quote_in_v3`, UVA present, per the
//!   hot-path verifier): about `62,310 + 1,500·S + 1,185`, so the expected
//!   consumption is [`SNIPE_CU_BASE`]` + 1,500·S` with `SNIPE_CU_BASE =
//!   63,500` (rounded up);
//! * the limit adds a [`SNIPE_CU_MARGIN`] of 1,500: `65,000 + 1,500·S`,
//!   clamped to `[floor, 1,400,000]` where the floor is `tx.cu_limit`.
//! * **wallet term** W ([`wallet_missing_bumps`]): pump also re-derives the
//!   buyer's `user_volume_accumulator` on every buy, 1,500 CU per missing
//!   bump. gap-5's wallet (`EW4dq…`) has UVA bump 255 (W = 0), so its
//!   constant hides the term; a wallet with UVA bump 254 costs 1,500 more on
//!   every mint, 252 costs 4,500 more (simulated 2026-10-14 on live mints:
//!   `LfEca…` +1,496..+1,499, `C9fus…` +4,498; the wallet's wSOL-ATA bump
//!   has no effect: `sssss…` with bump 251 fit exactly). The hot path adds
//!   the wallet's W (constant per process) to the mint's S. Grinding the hot
//!   wallet's key to a UVA bump of 255 (one key in two) makes W = 0.
//!
//! S is roughly NegBin(4, ½): median mint ~ S = 4 (71,000), S = 14 was the
//! max in 361 simulations. In v1 the priority fee is a fixed total, and on
//! legacy the CU price is derived from the same total and the per-mint
//! limit, so a tighter limit never changes what we pay, only the
//! scheduler's priority rank (`reward / (cost + 1)`).

use pump_codec::Pubkey;

/// gap-5 `buy_v3` envelope base with a nonce advance (v1, ATA create, tip).
pub const BUY_V3_CU_BASE_NONCE: u32 = 62_310;
/// The same without a nonce advance (`AdvanceNonceAccount` costs 150).
pub const BUY_V3_CU_BASE: u32 = 62_160;
/// CU per missing bump (one extra `create_program_address` round on chain).
pub const CU_PER_MISSING_BUMP: u32 = 1_500;
/// Mayhem coins cost ~409 CU less (gap-5 least squares).
pub const CU_MAYHEM_DISCOUNT: u32 = 409;
/// Expected cost of our buy template at S = 0 (v1 + nonce,
/// `buy_exact_quote_in_v3`, UVA present): 62,310 + 1,185 measured, rounded up.
pub const SNIPE_CU_BASE: u32 = 63_500;
/// Safety margin on top of the expected cost.
pub const SNIPE_CU_MARGIN: u32 = 1_500;
/// Solana's per-transaction maximum.
pub const MAX_CU_LIMIT: u32 = 1_400_000;
/// Recommended `tx.cu_limit` floor: 65,000, the per-mint formula at S = 0,
/// so the floor never binds and every buy carries its own
/// `65,000 + 1,500·S` limit (an 88,000 floor, the earlier recommendation,
/// wasted ~22% of priority per CU on priority-ordered leaders at the median
/// S = 4; the formula already covers real creates, `tests/cu_gap5.rs`).
pub const RECOMMENDED_CU_FLOOR: u32 = 65_000;
/// Static compute-unit limit of the sell templates (they carry no per-mint
/// limit): gap-5 section 8 puts the exit at `44,300 + 1,500·S3` plus 1,464
/// for `CloseAccount` and 300 for tip and nonce, i.e. 67,064 at S3 = 14 (the
/// largest seen) before the wallet term, so sells keep the 88,000 static
/// limit whatever the buy floor ([`sell_cu_limit`]).
pub const SELL_CU_FLOOR: u32 = 88_000;

/// The sells' static limit: `tx.cu_limit`, at least [`SELL_CU_FLOOR`].
pub fn sell_cu_limit(floor: u32) -> u32 {
    floor.clamp(SELL_CU_FLOOR, MAX_CU_LIMIT)
}

/// The gap-5 `buy_v3` model: `62,310 + 1,500·S` with a nonce (`62,160`
/// without), minus 409 on mayhem coins.
pub fn buy_v3_cu_model(s: u32, nonce: bool, mayhem: bool) -> u32 {
    let base = if nonce {
        BUY_V3_CU_BASE_NONCE
    } else {
        BUY_V3_CU_BASE
    };
    (base + CU_PER_MISSING_BUMP.saturating_mul(s)).saturating_sub(if mayhem {
        CU_MAYHEM_DISCOUNT
    } else {
        0
    })
}

/// Expected consumption of our buy template: `63,500 + 1,500·S`.
pub fn snipe_cu_expected(s: u32) -> u32 {
    SNIPE_CU_BASE.saturating_add(CU_PER_MISSING_BUMP.saturating_mul(s))
}

/// The wallet's own missing bumps W: `255 -` the bump of the buyer's pump
/// `user_volume_accumulator`, which pump re-derives on every buy (1,500 CU
/// per missing bump). Constant per wallet: compute it once at startup (one
/// full PDA search) and add it to the mint's S.
pub fn wallet_missing_bumps(user: &Pubkey) -> u32 {
    255 - u32::from(pump_codec::pda::user_volume_accumulator_with_bump(user).1)
}

/// The per-mint limit: `65,000 + 1,500·S` clamped to `[floor, 1,400,000]`
/// (`floor` = `tx.cu_limit`; a floor above the maximum is capped). `s` is
/// the mint's four bumps plus the wallet term ([`wallet_missing_bumps`]).
pub fn snipe_cu_limit(s: u32, floor: u32) -> u32 {
    snipe_cu_expected(s)
        .saturating_add(SNIPE_CU_MARGIN)
        .clamp(floor.min(MAX_CU_LIMIT), MAX_CU_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formula_and_clamp() {
        assert_eq!(snipe_cu_limit(0, 0), 65_000);
        assert_eq!(snipe_cu_limit(4, 0), 71_000);
        assert_eq!(snipe_cu_limit(14, 0), 86_000);
        // the recommended floor never binds: every mint gets its own limit
        assert_eq!(snipe_cu_limit(0, RECOMMENDED_CU_FLOOR), 65_000);
        assert_eq!(snipe_cu_limit(4, RECOMMENDED_CU_FLOOR), 71_000);
        assert_eq!(snipe_cu_limit(16, RECOMMENDED_CU_FLOOR), 89_000);
        assert_eq!(snipe_cu_limit(4, 88_000), 88_000);
        // sells keep a static 88,000 whatever the buy floor
        assert_eq!(sell_cu_limit(RECOMMENDED_CU_FLOOR), 88_000);
        assert_eq!(sell_cu_limit(0), 88_000);
        assert_eq!(sell_cu_limit(100_000), 100_000);
        assert_eq!(snipe_cu_limit(1_020, 0), MAX_CU_LIMIT);
        assert_eq!(snipe_cu_limit(u32::MAX, 0), MAX_CU_LIMIT);
        assert_eq!(snipe_cu_limit(0, 2_000_000), MAX_CU_LIMIT);
        assert_eq!(buy_v3_cu_model(14, true, false), 83_310);
        assert_eq!(buy_v3_cu_model(3, true, true), 66_401);
        assert_eq!(buy_v3_cu_model(0, false, false), 62_160);
        // the template's expected cost sits 1,190 above the gap-5 envelope
        assert_eq!(
            snipe_cu_expected(7) - buy_v3_cu_model(7, true, false),
            1_190
        );
    }

    /// Regression (F3, 2026-10-14): live `simulateTransaction` of the hot
    /// path's v1 + nonce buy template at the per-mint limit. Wallets whose
    /// UVA bump is below 255 consumed `1,500·W` more than `63,500 + 1,500·S`
    /// and failed a limit without the wallet term (legacy:
    /// `ComputationalBudgetExceeded` / `ProgramFailedToComplete`).
    #[test]
    fn wallet_term_from_uva_bump() {
        let w = |k: &str| wallet_missing_bumps(&k.parse().unwrap());
        // gap-5's wallet: the 361-record fit has W = 0
        assert_eq!(w("EW4dqBdFKBjjfTkdX1XeR9BnfrL1sUivVNaTJVJ18Tk2"), 0);
        // wSOL-ATA bump 251, UVA bump 255: measured exactly on the formula
        assert_eq!(w("ssssswdk4RR8HqkE3uwUWzDbd6mXFTTPjcXBKNzQ57E"), 0);
        // (wallet, W, S, measured unitsConsumed on v1 + nonce)
        for (k, wt, s, units) in [
            (
                "LfEcaUf77iEhnz6gFpLqYgDb5Uk6Ekc5n69wu7Qa9Uw",
                1,
                0,
                64_998u32,
            ),
            ("LfEcaUf77iEhnz6gFpLqYgDb5Uk6Ekc5n69wu7Qa9Uw", 1, 2, 67_999),
            ("C9fusuRFwYkH2kBsq4k2Sf9FfZHtvP7WEcaJ3AEoM8Z9", 3, 5, 75_498),
            (
                "C9fusuRFwYkH2kBsq4k2Sf9FfZHtvP7WEcaJ3AEoM8Z9",
                3,
                13,
                87_499,
            ),
        ] {
            assert_eq!(w(k), wt, "{k}");
            // without W the v1 limit leaves at most 2 CU (legacy, +450, fails)
            assert!(snipe_cu_limit(s, 0) < units + 3, "{k} S {s}");
            // with W: covered, with the 1,500 margin (legacy adds 450)
            let lim = snipe_cu_limit(s + wt, 0);
            assert!(lim >= units + 450 + 1_000, "{k} S {s}: {lim} vs {units}");
            assert!(lim - units <= 3_500);
        }
    }
}
