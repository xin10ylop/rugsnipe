//! PumpSwap `multi_hop_swap` (2026-10 `pump_amm` IDL): one instruction that
//! routes a trade through PumpSwap pools and pump bonding curves, for coins
//! priced in another token (a curve quoted in a graduated coin whose pool is
//! quoted in WBTC: WBTC -> pool -> curve -> coin).
//!
//! **Accounts.** 16 fixed ([`FIXED_ACCOUNTS`]), then 5 per hop in path order
//! (input to output): `base_mint, quote_mint, market, base_vault,
//! quote_vault`. The market is the pool or the bonding curve; its vaults are
//! its ATAs. A hop buys its base when the route enters it through its quote
//! and sells it otherwise; the program reads the direction from the mints.
//! The route's intermediate tokens move vault to vault: the user passes only
//! the input and output token accounts.
//!
//! **Fees** are paid in the route's outer asset (the input of a buy route,
//! the output of a sell route) to that asset's ATA of a
//! [`c::BUYBACK_FEE_RECIPIENTS`] wallet (account 6).
//!
//! **Data**: `2b644913e9f66f94 || amount_in u64 || min_amount_out u64`.
//!
//! Checked against H9kuqGKN's transactions of 2026-10-26 (a WBTC -> pool ->
//! curve buy and its sell, `tests/multihop_fixtures.rs`).

use crate::constants as c;
use crate::pda;
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;

/// `multi_hop_swap` discriminator.
pub const MULTI_HOP_SWAP_DISC: [u8; 8] = [0x2b, 0x64, 0x49, 0x13, 0xe9, 0xf6, 0x6f, 0x94];
/// Fixed accounts before the hops.
pub const FIXED_ACCOUNTS: usize = 16;
/// Accounts per hop.
pub const HOP_ACCOUNTS: usize = 5;

/// Where a hop trades.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Venue {
    /// A pump bonding curve (`market` = `bonding_curve(base_mint)`).
    Curve,
    /// A PumpSwap pool.
    Pool,
}

/// One market of a route, as the instruction lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hop {
    pub venue: Venue,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub market: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
}

impl Hop {
    /// The bonding curve of `base_mint` (quoted in `quote_mint`); the vaults
    /// are the curve's ATAs under each mint's token program.
    pub fn curve(
        base_mint: &Pubkey,
        base_token_program: &Pubkey,
        quote_mint: &Pubkey,
        quote_token_program: &Pubkey,
    ) -> Self {
        let market = pda::bonding_curve(base_mint);
        Self {
            venue: Venue::Curve,
            base_mint: *base_mint,
            quote_mint: *quote_mint,
            market,
            base_vault: pda::associated_token_address(&market, base_mint, base_token_program),
            quote_vault: pda::associated_token_address(&market, quote_mint, quote_token_program),
        }
    }

    /// The PumpSwap pool `pool` of `base_mint` / `quote_mint`; the vaults are
    /// the pool's ATAs.
    pub fn pool(
        pool: &Pubkey,
        base_mint: &Pubkey,
        base_token_program: &Pubkey,
        quote_mint: &Pubkey,
        quote_token_program: &Pubkey,
    ) -> Self {
        Self {
            venue: Venue::Pool,
            base_mint: *base_mint,
            quote_mint: *quote_mint,
            market: *pool,
            base_vault: pda::associated_token_address(pool, base_mint, base_token_program),
            quote_vault: pda::associated_token_address(pool, quote_mint, quote_token_program),
        }
    }

    /// A hop from its 5 instruction accounts. The venue is read from the
    /// market: the bonding curve PDA of the base mint is a curve, anything
    /// else a pool.
    pub fn from_accounts(a: [Pubkey; HOP_ACCOUNTS]) -> Self {
        let venue = if a[2] == pda::bonding_curve(&a[0]) {
            Venue::Curve
        } else {
            Venue::Pool
        };
        Self {
            venue,
            base_mint: a[0],
            quote_mint: a[1],
            market: a[2],
            base_vault: a[3],
            quote_vault: a[4],
        }
    }

    /// The 5 instruction accounts.
    pub fn accounts(&self) -> [Pubkey; HOP_ACCOUNTS] {
        [
            self.base_mint,
            self.quote_mint,
            self.market,
            self.base_vault,
            self.quote_vault,
        ]
    }

    /// `true` when `mint` is one of the hop's two mints.
    pub fn has(&self, mint: &Pubkey) -> bool {
        self.base_mint == *mint || self.quote_mint == *mint
    }

    /// The hop's other mint (`None` when `mint` is not one of its mints).
    pub fn other(&self, mint: &Pubkey) -> Option<Pubkey> {
        if self.base_mint == *mint {
            Some(self.quote_mint)
        } else if self.quote_mint == *mint {
            Some(self.base_mint)
        } else {
            None
        }
    }
}

/// Why a route does not chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteError {
    /// No hops.
    Empty,
    /// Hop `n` does not trade the mint the route reaches it with.
    Broken(usize),
    /// More hops than one instruction is built for.
    TooLong(usize),
}

/// The longest route built (4 hops: 36 accounts, inside a legacy message
/// with the usual tip / budget / ATA instructions).
pub const MAX_HOPS: usize = 4;

/// A route in path order: `input` enters the first hop; each hop hands its
/// other mint to the next.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Route {
    pub input: Pubkey,
    pub hops: Vec<Hop>,
}

impl Route {
    /// Checks that the hops chain from `input`.
    pub fn new(input: Pubkey, hops: Vec<Hop>) -> Result<Self, RouteError> {
        if hops.is_empty() {
            return Err(RouteError::Empty);
        }
        if hops.len() > MAX_HOPS {
            return Err(RouteError::TooLong(hops.len()));
        }
        let mut m = input;
        for (n, h) in hops.iter().enumerate() {
            m = h.other(&m).ok_or(RouteError::Broken(n))?;
        }
        Ok(Self { input, hops })
    }

    /// The mints along the path, input first, output last.
    pub fn mints(&self) -> Vec<Pubkey> {
        let mut v = Vec::with_capacity(self.hops.len() + 1);
        let mut m = self.input;
        v.push(m);
        for h in &self.hops {
            m = h.other(&m).expect("a Route chains (checked in new)");
            v.push(m);
        }
        v
    }

    /// The mint the route ends in.
    pub fn output(&self) -> Pubkey {
        *self.mints().last().expect("a route has at least one hop")
    }

    /// `true` per hop when the hop buys its base (the route enters through
    /// its quote).
    pub fn buys(&self) -> Vec<bool> {
        let ms = self.mints();
        self.hops
            .iter()
            .zip(&ms)
            .map(|(h, m)| h.quote_mint == *m)
            .collect()
    }

    /// The same markets walked back: output becomes input.
    pub fn reversed(&self) -> Self {
        let mut hops = self.hops.clone();
        hops.reverse();
        Self {
            input: self.output(),
            hops,
        }
    }
}

/// `multi_hop_swap` instruction data.
pub fn multi_hop_swap_data(amount_in: u64, min_amount_out: u64) -> Vec<u8> {
    let mut d = Vec::with_capacity(24);
    d.extend_from_slice(&MULTI_HOP_SWAP_DISC);
    d.extend_from_slice(&amount_in.to_le_bytes());
    d.extend_from_slice(&min_amount_out.to_le_bytes());
    d
}

/// The outer asset's buyback fee account: the ATA of buyback recipient
/// `index` (mod 8) for `outer_mint`.
pub fn buyback_fee_account(
    index: usize,
    outer_mint: &Pubkey,
    outer_token_program: &Pubkey,
) -> Pubkey {
    let owner = c::BUYBACK_FEE_RECIPIENTS[index % c::BUYBACK_FEE_RECIPIENTS.len()];
    pda::associated_token_address(&owner, outer_mint, outer_token_program)
}

/// Inputs of [`multi_hop_swap`].
#[derive(Debug, Clone)]
pub struct MultiHop<'a> {
    pub user: Pubkey,
    /// The user's token account of `route.input`.
    pub user_in: Pubkey,
    /// The user's token account of `route.output()`.
    pub user_out: Pubkey,
    pub route: &'a Route,
    /// The asset the fee is paid in: the route's input on a buy route, its
    /// output on a sell route.
    pub outer_mint: Pubkey,
    pub outer_token_program: Pubkey,
    /// Which buyback recipient takes the fee.
    pub buyback_index: usize,
    pub amount_in: u64,
    pub min_amount_out: u64,
}

/// The `multi_hop_swap` instruction.
pub fn multi_hop_swap(m: &MultiHop<'_>) -> Instruction {
    let ro = AccountMeta::new_readonly;
    let w = |k| AccountMeta::new(k, false);
    let mut accounts = Vec::with_capacity(FIXED_ACCOUNTS + HOP_ACCOUNTS * m.route.hops.len());
    accounts.extend([
        AccountMeta::new(m.user, true),
        w(m.user_in),
        w(m.user_out),
        ro(c::PUMP_SWAP_GLOBAL_CONFIG, false),
        ro(c::PUMP_SWAP_FEE_CONFIG, false),
        w(pda::pump_swap_user_volume_accumulator(&m.user)),
        w(buyback_fee_account(
            m.buyback_index,
            &m.outer_mint,
            &m.outer_token_program,
        )),
        ro(c::SPL_TOKEN_PROGRAM_ID, false),
        ro(c::TOKEN_2022_PROGRAM_ID, false),
        ro(c::SYSTEM_PROGRAM_ID, false),
        ro(c::PUMP_SWAP_EVENT_AUTHORITY, false),
        ro(c::PUMP_SWAP_PROGRAM_ID, false),
        ro(c::PUMP_PROGRAM_ID, false),
        ro(c::GLOBAL, false),
        ro(c::FEE_CONFIG, false),
        ro(c::EVENT_AUTHORITY, false),
    ]);
    for h in &m.route.hops {
        accounts.extend([
            ro(h.base_mint, false),
            ro(h.quote_mint, false),
            w(h.market),
            w(h.base_vault),
            w(h.quote_vault),
        ]);
    }
    Instruction {
        program_id: c::PUMP_SWAP_PROGRAM_ID,
        accounts,
        data: multi_hop_swap_data(m.amount_in, m.min_amount_out),
    }
}

/// A parsed `multi_hop_swap` (accounts by instruction position).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMultiHop {
    pub user: Pubkey,
    pub user_in: Pubkey,
    pub user_out: Pubkey,
    pub buyback_fee_account: Pubkey,
    pub amount_in: u64,
    pub min_amount_out: u64,
    pub hops: Vec<Hop>,
}

/// Parses a `multi_hop_swap` from its accounts and data (`None` when the
/// data is not one, or the accounts are short or unresolved).
pub fn parse_multi_hop_swap<A: crate::instructions::AccountLookup + ?Sized>(
    accounts: &A,
    data: &[u8],
) -> Option<ParsedMultiHop> {
    if data.len() < 24 || data[..8] != MULTI_HOP_SWAP_DISC {
        return None;
    }
    let n = accounts.account_count();
    if n < FIXED_ACCOUNTS + HOP_ACCOUNTS || !(n - FIXED_ACCOUNTS).is_multiple_of(HOP_ACCOUNTS) {
        return None;
    }
    let mut hops = Vec::with_capacity((n - FIXED_ACCOUNTS) / HOP_ACCOUNTS);
    for h in 0..(n - FIXED_ACCOUNTS) / HOP_ACCOUNTS {
        let o = FIXED_ACCOUNTS + h * HOP_ACCOUNTS;
        let mut a = [Pubkey::default(); HOP_ACCOUNTS];
        for (k, slot) in a.iter_mut().enumerate() {
            *slot = accounts.account(o + k)?;
        }
        hops.push(Hop::from_accounts(a));
    }
    Some(ParsedMultiHop {
        user: accounts.account(0)?,
        user_in: accounts.account(1)?,
        user_out: accounts.account(2)?,
        buyback_fee_account: accounts.account(6)?,
        amount_in: u64::from_le_bytes(data[8..16].try_into().ok()?),
        min_amount_out: u64::from_le_bytes(data[16..24].try_into().ok()?),
        hops,
    })
}

// ============================================================== pricing
/// Constant-product output for `amount_in` against reserves `r_in` / `r_out`,
/// the fee (basis points) taken off the input. Saturates instead of
/// overflowing; 0 on empty reserves.
pub fn cp_out(r_in: u64, r_out: u64, amount_in: u64, fee_bps: u32) -> u64 {
    if r_in == 0 || r_out == 0 {
        return 0;
    }
    let net = amount_in as u128 * (10_000 - fee_bps.min(10_000)) as u128 / 10_000;
    (r_out as u128 * net / (r_in as u128 + net)) as u64
}

/// The curve price against a reference, from virtual token reserves alone:
/// on a constant-product curve the price is `k / vt^2`, so the ratio of two
/// states is `(vt_ref / vt)^2` (whatever the quote). `extra_out` is base
/// held out of the curve that the price should not count (our own
/// position: `vt + extra_out` is the curve without us).
pub fn curve_price_ratio(vt_ref: u64, vt: u64, extra_out: u64) -> f64 {
    let v = vt as f64 + extra_out as f64;
    if v <= 0.0 {
        return 0.0;
    }
    (vt_ref as f64 / v).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(n: u8) -> Pubkey {
        Pubkey::new_from_array([n; 32])
    }

    #[test]
    fn route_chains_and_reverses() {
        let (a, b, x) = (k(1), k(2), k(3));
        let pool = Hop::pool(
            &k(9),
            &b,
            &c::TOKEN_2022_PROGRAM_ID,
            &a,
            &c::SPL_TOKEN_PROGRAM_ID,
        );
        let curve = Hop::curve(&x, &c::TOKEN_2022_PROGRAM_ID, &b, &c::TOKEN_2022_PROGRAM_ID);
        let r = Route::new(a, vec![pool, curve]).unwrap();
        assert_eq!(r.mints(), vec![a, b, x]);
        assert_eq!(r.buys(), vec![true, true]);
        let s = r.reversed();
        assert_eq!(s.input, x);
        assert_eq!(s.mints(), vec![x, b, a]);
        assert_eq!(s.buys(), vec![false, false]);
        assert_eq!(Route::new(x, vec![pool, curve]), Err(RouteError::Broken(0)));
        assert_eq!(Route::new(a, vec![]), Err(RouteError::Empty));
        assert_eq!(curve.venue, Venue::Curve);
        assert_eq!(Hop::from_accounts(curve.accounts()), curve);
        assert_eq!(Hop::from_accounts(pool.accounts()).venue, Venue::Pool);
    }

    #[test]
    fn data_layout() {
        let d = multi_hop_swap_data(188_751, 1);
        assert_eq!(hex(&d), "2b644913e9f66f944fe10200000000000100000000000000");
    }

    #[test]
    fn cp_out_and_price_ratio() {
        assert_eq!(cp_out(100, 100, 100, 0), 50);
        assert_eq!(cp_out(0, 100, 5, 0), 0);
        assert!(cp_out(1_000_000, 1_000_000, 1000, 100) < cp_out(1_000_000, 1_000_000, 1000, 0));
        assert!((curve_price_ratio(100, 50, 0) - 4.0).abs() < 1e-12);
        assert!((curve_price_ratio(100, 40, 10) - 4.0).abs() < 1e-12);
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }
}
