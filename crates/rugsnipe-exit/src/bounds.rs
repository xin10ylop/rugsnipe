//! Price multiple and sell bounds (docs/06 section 8), integer only.

use crate::policy::{ExitPolicy, BPS};
use crate::types::Venue;
use pump_codec::curve::{CurveState, FeeBps, INITIAL_VIRTUAL_SOL_RESERVES, K_INITIAL};
use pump_codec::Pubkey;

/// Base units left in the pool after migration (206.9M tokens reserved).
pub const POOL_BASE_AT_MIGRATION: u64 = 206_900_000_000_000;

/// Price multiple versus the fill, in basis points.
///
/// * Curve: `mult = (v / v_fill)^2` (price is proportional to V^2 on a constant-product curve).
/// * PumpSwap: spot price ratio `(q / b) / (v_fill / t_fill)`.
///
/// Returns 0 when the fill is unknown.
pub fn mult_bps(venue: Venue, v: u64, t: u64, v_fill: u64, t_fill: u64) -> u64 {
    if v_fill == 0 {
        return 0;
    }
    let r = match venue {
        Venue::Curve => {
            let num = (v as u128) * (v as u128) * BPS as u128;
            num / ((v_fill as u128) * (v_fill as u128))
        }
        Venue::PumpSwap => {
            if t == 0 || t_fill == 0 {
                return 0;
            }
            // (v * t_fill * BPS) / (t * v_fill); v, t_fill < 2^64 so split to stay in u128.
            let num = (v as u128) * (t_fill as u128);
            let den = (t as u128) * (v_fill as u128);
            mul_div(num, BPS as u128, den)
        }
    };
    u64::try_from(r).unwrap_or(u64::MAX)
}

/// `a * b / d` without overflowing u128 for `b <= 2^32` (floor).
fn mul_div(a: u128, b: u128, d: u128) -> u128 {
    if d == 0 {
        return 0;
    }
    let q = a / d;
    let r = a % d;
    q * b + (r * b) / d
}

/// `x * keep_bps / 10_000`, floor.
#[inline]
pub fn keep_bps(x: u64, keep: u64) -> u64 {
    ((x as u128) * (keep as u128) / BPS as u128) as u64
}

/// Net lamports for selling `tokens` into a curve with virtual reserves `(v, t)`.
pub fn curve_sell_net(tokens: u64, v: u64, t: u64, creator: Pubkey, fees: FeeBps) -> u64 {
    let s = CurveState {
        virtual_sol: v,
        virtual_token: t,
        real_sol: 0,
        real_token: 0,
        complete: false,
        creator,
    };
    s.quote_sell(tokens, fees).map(|q| q.net).unwrap_or(0)
}

/// Net lamports for selling `tokens` into a constant-product pool `(q, b)` with
/// a total fee of `fee_bps` taken from the output (rounded up). Local helper:
/// `pump-codec` has no PumpSwap quote math yet.
pub fn amm_sell_net(tokens: u64, q: u64, b: u64, fee_bps: u32) -> u64 {
    let n = tokens as u128;
    let d = b as u128 + n;
    if d == 0 || tokens == 0 {
        return 0;
    }
    let out = n * q as u128 / d;
    let fee = (out * fee_bps as u128).div_ceil(BPS as u128);
    out.saturating_sub(fee) as u64
}

/// Market state the bounds are computed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Market {
    pub venue: Venue,
    /// Curve virtual SOL or pool quote reserve.
    pub v: u64,
    /// Curve virtual token or pool base reserve.
    pub t: u64,
    pub creator: Pubkey,
    pub fees: FeeBps,
}

impl Market {
    /// Net proceeds of selling `tokens` at this state.
    pub fn sell_net(&self, tokens: u64, amm_fee_bps: u32) -> u64 {
        match self.venue {
            Venue::Curve => curve_sell_net(tokens, self.v, self.t, self.creator, self.fees),
            Venue::PumpSwap => amm_sell_net(tokens, self.v, self.t, amm_fee_bps),
        }
    }
}

/// Normal bound: `quote_sell(tokens, V_trigger) x (1 - tp_slip)`.
pub fn normal_min_out(p: &ExitPolicy, m: &Market, tokens: u64) -> u64 {
    keep_bps(
        m.sell_net(tokens, p.amm_fee_bps),
        BPS - u64::from(p.tp_slip_bps),
    )
}

/// Dump bound. Curve: `quote_sell(tokens, V = dump_floor_vsol) x (1 - dump_slip)`
/// on the constant-product curve through the initial reserves (`T = k / V`), so it
/// never fails on price while the curve trades. PumpSwap has no floor: the pool
/// quote x (1 - amm_dump_slip).
pub fn dump_min_out(p: &ExitPolicy, m: &Market, tokens: u64) -> u64 {
    match m.venue {
        Venue::Curve => {
            let v = p.dump_floor_vsol;
            let t = if v == INITIAL_VIRTUAL_SOL_RESERVES {
                pump_codec::curve::INITIAL_VIRTUAL_TOKEN_RESERVES
            } else {
                u64::try_from(K_INITIAL / v.max(1) as u128).unwrap_or(u64::MAX)
            };
            keep_bps(
                curve_sell_net(tokens, v, t, m.creator, m.fees),
                BPS - u64::from(p.dump_slip_bps),
            )
        }
        Venue::PumpSwap => keep_bps(
            amm_sell_net(tokens, m.v, m.t, p.amm_fee_bps),
            BPS - u64::from(p.amm_dump_slip_bps),
        ),
    }
}

/// Pool reserves estimated from the curve at completion when no pool event has
/// been seen yet: quote = SOL raised (`v - 30 SOL`) minus the migration fee,
/// base = the 206.9M reserved tokens.
pub fn pool_from_curve(v: u64) -> (u64, u64) {
    let q = v
        .saturating_sub(INITIAL_VIRTUAL_SOL_RESERVES)
        .saturating_sub(pump_codec::curve::POOL_MIGRATION_FEE);
    (q, POOL_BASE_AT_MIGRATION)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pump_codec::curve::INITIAL_VIRTUAL_TOKEN_RESERVES as T0;

    #[test]
    fn mult_is_v_squared() {
        let v0 = 30_000_000_000;
        assert_eq!(mult_bps(Venue::Curve, v0, T0, v0, T0), 10_000);
        // 10 SOL into a fresh curve: (40/30)^2 = 1.777..
        assert_eq!(mult_bps(Venue::Curve, 40_000_000_000, 0, v0, T0), 17_777);
        // doubling V quadruples the price
        assert_eq!(mult_bps(Venue::Curve, 2 * v0, 0, v0, T0), 40_000);
        assert_eq!(mult_bps(Venue::Curve, v0, T0, 0, 0), 0);
    }

    #[test]
    fn pool_mult_is_price_ratio_and_continuous_at_migration() {
        // Curve at completion: V = 115.005 SOL, T = 279.9M tokens; pool ~ (85 SOL, 206.9M).
        let v_fill = 30_000_000_000;
        let t_fill = T0;
        let v_end = 115_005_359_057u64;
        let t_end = (K_INITIAL / v_end as u128) as u64;
        let curve = mult_bps(Venue::Curve, v_end, t_end, v_fill, t_fill);
        let (q, b) = pool_from_curve(v_end);
        let pool = mult_bps(Venue::PumpSwap, q, b, v_fill, t_fill);
        assert!((14_000..15_000).contains(&(curve / 10)), "{curve}");
        let rel = (pool as f64 / curve as f64 - 1.0).abs();
        assert!(rel < 0.02, "pool {pool} vs curve {curve}");
    }

    #[test]
    fn dump_bound_is_floor_quote_times_098() {
        let p = ExitPolicy::default();
        let tokens = 3_000_000_000_000; // 3M tokens
        let m = Market {
            venue: Venue::Curve,
            v: 60_000_000_000,
            t: (K_INITIAL / 60_000_000_000u128) as u64,
            creator: Pubkey::new_from_array([7; 32]),
            fees: FeeBps::DEFAULT,
        };
        let floor = CurveState::fresh(m.creator)
            .quote_sell(tokens, FeeBps::DEFAULT)
            .unwrap()
            .net;
        assert_eq!(dump_min_out(&p, &m, tokens), floor * 98 / 100);
        // the dump bound is always below the current quote on a curve above the floor
        assert!(dump_min_out(&p, &m, tokens) < m.sell_net(tokens, p.amm_fee_bps));
        // the normal bound is 85% of the current quote
        let q = m.sell_net(tokens, p.amm_fee_bps);
        assert_eq!(normal_min_out(&p, &m, tokens), q * 85 / 100);
    }

    #[test]
    fn amm_quote() {
        // 1% of the base into a 100 SOL / 1e15 pool, 1.25% fee
        let out = amm_sell_net(10_000_000_000_000, 100_000_000_000, 1_000_000_000_000_000, 125);
        let gross = 10_000_000_000_000u128 * 100_000_000_000 / 1_010_000_000_000_000;
        let fee = (gross * 125).div_ceil(10_000);
        assert_eq!(out as u128, gross - fee);
        assert_eq!(amm_sell_net(0, 1, 1, 125), 0);
    }
}
