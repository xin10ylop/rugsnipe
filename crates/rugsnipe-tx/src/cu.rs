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
//!
//! S is roughly NegBin(4, ½): median mint ~ S = 4 (71,000), S = 14 was the
//! max in 361 simulations. In v1 the priority fee is a fixed total, and on
//! legacy the CU price is derived from the same total and the per-mint
//! limit, so a tighter limit never changes what we pay, only the
//! scheduler's priority rank (`reward / (cost + 1)`).

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
/// Recommended `tx.cu_limit` floor: a static 88,000 covers S <= 15
/// (0.075% of mints fail a static 88,000; the per-mint limit covers them).
pub const RECOMMENDED_CU_FLOOR: u32 = 88_000;

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

/// The per-mint limit: `65,000 + 1,500·S` clamped to `[floor, 1,400,000]`
/// (`floor` = `tx.cu_limit`; a floor above the maximum is capped).
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
        assert_eq!(snipe_cu_limit(4, RECOMMENDED_CU_FLOOR), 88_000);
        assert_eq!(snipe_cu_limit(16, RECOMMENDED_CU_FLOOR), 89_000);
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
}
