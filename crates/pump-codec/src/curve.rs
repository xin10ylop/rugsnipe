//! Bonding-curve accounts (`Global`, `BondingCurve`, Pump Fees `FeeConfig`)
//! and the curve's integer math, reproduced exactly as the program does it.
//!
//! Notation: `V` = virtual SOL (quote) reserves in lamports, `T` = virtual
//! token reserves in base units (6 decimals), `S` = total supply (1e15).
//!
//! | operation | program formula | fn |
//! |---|---|---|
//! | tokens for `x` lamports (no fees) | `floor(T*x / (V+x))` | [`CurveState::tokens_for_sol`] |
//! | cost of exactly `n` tokens | `floor(n*V / (T-n)) + 1` (the program's `buy_quote`; equals `ceil` unless the division is exact) | [`CurveState::buy_cost`] |
//! | sell `n` tokens | `floor(n*V / (T+n))` | [`CurveState::sell_sol_out`] |
//! | fee | `ceil(amount*bps / 10_000)` per component (protocol, creator) | [`fee_ceil`] |
//! | exact-SOL-in buy | `net = floor(in*1e4/(1e4+Σbps))`, fees `ceil(net*bps/1e4)`, shave overshoot off `net`, `tokens = floor(T*(net-1)/(V+net-1))`, charged `net` | [`CurveState::quote_buy_exact_sol_in`] |
//! | buyback part of the protocol fee | `floor(fee * buyback_bps / 1e4)` | [`buyback_fee`] |
//!
//! Each formula is checked against real `TradeEvent`s in the tests
//! (`tests/curve_events.rs`), including the three worked examples of
//! `docs/research/pumpfun-mechanics-2026.md` section 4.

use crate::bytes::Reader;
use crate::constants::account as acc_disc;
use crate::error::Result;
use solana_pubkey::Pubkey;

// --------------------------------------------------------------- constants
/// `Global.initial_virtual_token_reserves`: 1.073e9 tokens.
pub const INITIAL_VIRTUAL_TOKEN_RESERVES: u64 = 1_073_000_000_000_000;
/// `Global.initial_virtual_sol_reserves`: 30 SOL.
pub const INITIAL_VIRTUAL_SOL_RESERVES: u64 = 30_000_000_000;
/// `Global.initial_real_token_reserves`: 793.1M tokens sellable on the curve.
pub const INITIAL_REAL_TOKEN_RESERVES: u64 = 793_100_000_000_000;
/// `Global.token_total_supply`: 1e9 tokens x 1e6 (mayhem mints have 2e9 minted,
/// but their curve still stores 1e15).
pub const TOKEN_TOTAL_SUPPLY: u64 = 1_000_000_000_000_000;
/// Token decimals of every pump mint.
pub const TOKEN_DECIMALS: u8 = 6;
/// Lamports per SOL.
pub const LAMPORTS_PER_SOL: u64 = 1_000_000_000;
/// Virtual token reserves at completion (`INITIAL_VIRTUAL - INITIAL_REAL`): 279.9M tokens.
pub const COMPLETE_VIRTUAL_TOKEN_RESERVES: u64 =
    INITIAL_VIRTUAL_TOKEN_RESERVES - INITIAL_REAL_TOKEN_RESERVES;
/// Curve invariant `k = V0 * T0` (lamports x base units).
pub const K_INITIAL: u128 =
    INITIAL_VIRTUAL_SOL_RESERVES as u128 * INITIAL_VIRTUAL_TOKEN_RESERVES as u128;
/// Market cap of a fresh curve: `floor(V0 * S / T0)` = 27,958,993,476 lamports (27.959 SOL).
pub const FLOOR_MARKET_CAP_LAMPORTS: u64 = (INITIAL_VIRTUAL_SOL_RESERVES as u128
    * TOKEN_TOTAL_SUPPLY as u128
    / INITIAL_VIRTUAL_TOKEN_RESERVES as u128) as u64;
/// SOL the curve charges to sell its whole remaining supply in one buy from a
/// fresh state: `buy_quote(793.1e12)` = 85,005,359,057 lamports. Real curves
/// complete a few lamports higher because of per-trade rounding (the API shows
/// completed curves at `real_sol_reserves` 85,005,359,061).
pub const COMPLETION_REAL_SOL_FROM_FRESH: u64 = 85_005_359_057;
/// Default protocol fee (`FeeConfig` flat tier on 2026-10-14): 95 bps.
pub const DEFAULT_PROTOCOL_FEE_BPS: u64 = 95;
/// Default creator fee (`FeeConfig`): 30 bps.
pub const DEFAULT_CREATOR_FEE_BPS: u64 = 30;
/// `Global.buyback_basis_points`: half of the protocol fee is the buyback fee.
pub const DEFAULT_BUYBACK_BPS: u64 = 5_000;
/// `Global.pool_migration_fee` (lamports).
pub const POOL_MIGRATION_FEE: u64 = 15_000_001;

const BPS_DENOM: u128 = 10_000;

// -------------------------------------------------------------------- fees
/// Protocol + creator fee rates of a curve trade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeeBps {
    pub protocol: u64,
    pub creator: u64,
}

impl FeeBps {
    /// 95 + 30 bps, the flat schedule in `FeeConfig` on 2026-10-14. Read
    /// `FeeConfig` at startup (and on `SetParams`) instead of trusting this.
    pub const DEFAULT: FeeBps = FeeBps {
        protocol: DEFAULT_PROTOCOL_FEE_BPS,
        creator: DEFAULT_CREATOR_FEE_BPS,
    };
    pub const ZERO: FeeBps = FeeBps {
        protocol: 0,
        creator: 0,
    };
    #[inline]
    pub const fn total(&self) -> u64 {
        self.protocol.saturating_add(self.creator)
    }
}

impl Default for FeeBps {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// `ceil(amount * bps / 10_000)`, the program's fee rounding (always up).
#[inline]
pub fn fee_ceil(amount: u64, bps: u64) -> u64 {
    u64::try_from((amount as u128 * bps as u128).div_ceil(BPS_DENOM)).unwrap_or(u64::MAX)
}

/// Creator fee: `fee_ceil`, or 0 on a curve without a creator (`Pubkey::default()`).
#[inline]
pub fn creator_fee(amount: u64, bps: u64, creator: &Pubkey) -> u64 {
    if *creator == Pubkey::default() {
        0
    } else {
        fee_ceil(amount, bps)
    }
}

/// Buyback slice of the protocol fee: `floor(protocol_fee * buyback_bps / 10_000)`
/// (verified on odd fees in the fixtures; mayhem coins report `buyback_bps = 0`).
#[inline]
pub fn buyback_fee(protocol_fee: u64, buyback_bps: u64) -> u64 {
    u64::try_from(protocol_fee as u128 * buyback_bps as u128 / BPS_DENOM).unwrap_or(u64::MAX)
}

/// Split an exact-quote-in budget like the program (`exact_quote_in_fees`):
/// `net = floor(in*1e4/(1e4+Σbps))`, each fee `ceil(net*bps/1e4)` on the
/// unshaved net, overshoot shaved off `net`. Returns `(net, protocol_fee,
/// creator_fee)`; `None` when nothing is left to swap.
pub fn exact_in_split(amount_in: u64, fees: FeeBps) -> Option<(u64, u64, u64)> {
    let input = amount_in as u128;
    let mut net = input * BPS_DENOM / (BPS_DENOM + fees.total() as u128);
    if net == 0 {
        return None;
    }
    let pf = (net * fees.protocol as u128).div_ceil(BPS_DENOM);
    let cf = (net * fees.creator as u128).div_ceil(BPS_DENOM);
    let cost = net + pf + cf;
    if cost > input {
        net = net.checked_sub(cost - input).filter(|n| *n > 0)?;
    }
    // net <= input fits; the fees fit whenever net survived the shave.
    Some((net as u64, u64::try_from(pf).ok()?, u64::try_from(cf).ok()?))
}

// ------------------------------------------------------------------ quotes
/// Result of a buy quote. `sol_cost` is what the curve receives (= the
/// `TradeEvent.sol_amount`); `total` is what leaves the user's wallet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuyQuote {
    pub tokens: u64,
    pub sol_cost: u64,
    pub protocol_fee: u64,
    pub creator_fee: u64,
    pub total: u64,
}

/// Result of a sell quote. `sol_out` is what leaves the curve (= the
/// `TradeEvent.sol_amount`); `net` is what the user receives after fees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SellQuote {
    pub tokens: u64,
    pub sol_out: u64,
    pub protocol_fee: u64,
    pub creator_fee: u64,
    pub net: u64,
}

/// The part of a bonding curve the math needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurveState {
    /// Virtual SOL (quote) reserves, lamports.
    pub virtual_sol: u64,
    /// Virtual token reserves, base units.
    pub virtual_token: u64,
    /// Real SOL (quote) held by the curve, lamports.
    pub real_sol: u64,
    /// Real tokens still sellable on the curve, base units.
    pub real_token: u64,
    /// `true` once the curve is sold out (waiting for / after migration).
    pub complete: bool,
    /// Coin creator (creator fee recipient; zero key = no creator fee).
    pub creator: Pubkey,
}

impl CurveState {
    /// A freshly created SOL curve at the mainnet `Global` constants.
    /// Mayhem coins also start here (verified on two mayhem `create_v2` +
    /// dev-buy transactions), but `set_mayhem_virtual_params` can later move
    /// their virtual reserves away from `real + 30 SOL` (fixtures show mayhem
    /// curves at ~9.5 SOL / 1.07e15 with 0.036 SOL real). After the create
    /// slot, take a mayhem curve's state from `TradeEvent` / the account.
    pub const fn fresh(creator: Pubkey) -> Self {
        Self {
            virtual_sol: INITIAL_VIRTUAL_SOL_RESERVES,
            virtual_token: INITIAL_VIRTUAL_TOKEN_RESERVES,
            real_sol: 0,
            real_token: INITIAL_REAL_TOKEN_RESERVES,
            complete: false,
            creator,
        }
    }

    /// From a decoded `BondingCurve` account.
    pub fn from_account(bc: &BondingCurve) -> Self {
        Self {
            virtual_sol: bc.virtual_quote_reserves,
            virtual_token: bc.virtual_token_reserves,
            real_sol: bc.real_quote_reserves,
            real_token: bc.real_token_reserves,
            complete: bc.complete,
            creator: bc.creator,
        }
    }

    /// Spot price in lamports per whole token (`V/T * 1e6`).
    pub fn price_lamports_per_token(&self) -> f64 {
        self.virtual_sol as f64 / self.virtual_token as f64 * 1e6
    }

    /// Market cap in lamports: `floor(V * supply / T)` (the SDK's
    /// `bondingCurveMarketCap`; the API's `market_cap` uses supply 1e15).
    pub fn market_cap_lamports(&self, supply: u64) -> u128 {
        if self.virtual_token == 0 {
            return 0;
        }
        self.virtual_sol as u128 * supply as u128 / self.virtual_token as u128
    }

    /// Market cap in SOL at the standard 1e15 supply.
    pub fn market_cap_sol(&self) -> f64 {
        self.market_cap_lamports(TOKEN_TOTAL_SUPPLY) as f64 / LAMPORTS_PER_SOL as f64
    }

    /// Tokens out for `net_sol` lamports going into the curve:
    /// `floor(T*x/(V+x))`, capped at `real_token`.
    pub fn tokens_for_sol(&self, net_sol: u64) -> u64 {
        let v = self.virtual_sol as u128;
        let t = self.virtual_token as u128;
        let x = net_sol as u128;
        if v + x == 0 {
            return 0;
        }
        ((t * x / (v + x)) as u64).min(self.real_token)
    }

    /// The program's `buy_quote`: lamports (before fees) to buy exactly
    /// `tokens`: `floor(n*V/(T-n)) + 1`. `None` for 0 tokens or `n >= T`.
    pub fn buy_cost(&self, tokens: u64) -> Option<u64> {
        if tokens == 0 || tokens >= self.virtual_token {
            return None;
        }
        let n = tokens as u128;
        let cost = n * self.virtual_sol as u128 / (self.virtual_token as u128 - n) + 1;
        u64::try_from(cost).ok()
    }

    /// Lamports out of the curve (before fees) for selling `tokens`: `floor(n*V/(T+n))`.
    pub fn sell_sol_out(&self, tokens: u64) -> u64 {
        let n = tokens as u128;
        let d = self.virtual_token as u128 + n;
        if d == 0 {
            return 0;
        }
        (n * self.virtual_sol as u128 / d) as u64
    }

    /// Exact-tokens-out buy (`buy`, `buy_v2`, `buy_v3`) of `tokens`, capped at
    /// the curve's remaining `real_token` (partial fill). Fees on top, ceil.
    pub fn quote_buy_exact_tokens(&self, tokens: u64, fees: FeeBps) -> Option<BuyQuote> {
        let tokens = tokens.min(self.real_token);
        let sol_cost = self.buy_cost(tokens)?;
        let protocol_fee = fee_ceil(sol_cost, fees.protocol);
        let creator_fee = creator_fee(sol_cost, fees.creator, &self.creator);
        Some(BuyQuote {
            tokens,
            sol_cost,
            protocol_fee,
            creator_fee,
            total: sol_cost
                .checked_add(protocol_fee)?
                .checked_add(creator_fee)?,
        })
    }

    /// Exact-SOL-in buy (`buy_exact_sol_in`, `buy_exact_quote_in_v2/_v3`)
    /// spending `spendable` lamports including fees. `None` if fees eat it all
    /// or the result is 0 tokens. Within the curve's remaining supply only
    /// (no synthetic-migration leg).
    pub fn quote_buy_exact_sol_in(&self, spendable: u64, fees: FeeBps) -> Option<BuyQuote> {
        let fees = if self.creator == Pubkey::default() {
            FeeBps { creator: 0, ..fees }
        } else {
            fees
        };
        let (net, protocol_fee, creator_fee) = exact_in_split(spendable, fees)?;
        let x = net as u128 - 1;
        let v = self.virtual_sol as u128;
        let t = self.virtual_token as u128;
        if v + x == 0 {
            return None;
        }
        let tokens = (t * x / (v + x)) as u64;
        if tokens == 0 || tokens > self.real_token {
            return None;
        }
        Some(BuyQuote {
            tokens,
            sol_cost: net,
            protocol_fee,
            creator_fee,
            total: net + protocol_fee + creator_fee,
        })
    }

    /// Sell `tokens`; fees deducted from the gross, ceil each.
    pub fn quote_sell(&self, tokens: u64, fees: FeeBps) -> Option<SellQuote> {
        if tokens == 0 {
            return None;
        }
        let sol_out = self.sell_sol_out(tokens);
        let protocol_fee = fee_ceil(sol_out, fees.protocol);
        let creator_fee = creator_fee(sol_out, fees.creator, &self.creator);
        let net = sol_out.checked_sub(protocol_fee.checked_add(creator_fee)?)?;
        Some(SellQuote {
            tokens,
            sol_out,
            protocol_fee,
            creator_fee,
            net,
        })
    }

    /// Apply a buy (tokens out, `sol_cost` in) to the reserves (saturating;
    /// feed it amounts from a quote of this state).
    pub fn apply_buy(&mut self, tokens: u64, sol_cost: u64) {
        self.virtual_sol = self.virtual_sol.saturating_add(sol_cost);
        self.virtual_token = self.virtual_token.saturating_sub(tokens);
        self.real_sol = self.real_sol.saturating_add(sol_cost);
        self.real_token = self.real_token.saturating_sub(tokens);
        if self.real_token == 0 {
            self.complete = true;
        }
    }

    /// Apply a sell (tokens in, `sol_out` out) to the reserves (saturating).
    pub fn apply_sell(&mut self, tokens: u64, sol_out: u64) {
        self.virtual_sol = self.virtual_sol.saturating_sub(sol_out);
        self.virtual_token = self.virtual_token.saturating_add(tokens);
        self.real_sol = self.real_sol.saturating_sub(sol_out);
        self.real_token = self.real_token.saturating_add(tokens);
    }

    /// The state before a trade, rebuilt from the trade's post-trade reserves
    /// (`TradeEvent` carries post-trade reserves). `None` if the amounts are
    /// inconsistent with the reserves (would under/overflow).
    pub fn pre_trade(
        post: &CurveState,
        is_buy: bool,
        sol_amount: u64,
        token_amount: u64,
    ) -> Option<CurveState> {
        let mut s = *post;
        if is_buy {
            s.virtual_sol = s.virtual_sol.checked_sub(sol_amount)?;
            s.virtual_token = s.virtual_token.checked_add(token_amount)?;
            s.real_sol = s.real_sol.checked_sub(sol_amount)?;
            s.real_token = s.real_token.checked_add(token_amount)?;
        } else {
            s.virtual_sol = s.virtual_sol.checked_add(sol_amount)?;
            s.virtual_token = s.virtual_token.checked_sub(token_amount)?;
            s.real_sol = s.real_sol.checked_add(sol_amount)?;
            s.real_token = s.real_token.checked_sub(token_amount)?;
        }
        s.complete = false;
        Some(s)
    }

    /// Price impact of putting `net_sol` lamports into the curve:
    /// `(V'/T') / (V/T) - 1` (spot after vs spot before). From a fresh curve,
    /// 10 SOL gives `(40/30)^2 - 1 = 0.778`.
    pub fn buy_price_impact(&self, net_sol: u64) -> f64 {
        let tokens = self.tokens_for_sol(net_sol);
        let v1 = self.virtual_sol.saturating_add(net_sol) as f64;
        let t1 = self.virtual_token.saturating_sub(tokens) as f64;
        (v1 / t1) / (self.virtual_sol as f64 / self.virtual_token as f64) - 1.0
    }

    /// Lamports (before fees) needed to buy out the rest of the curve.
    pub fn sol_to_complete(&self) -> Option<u64> {
        self.buy_cost(self.real_token)
    }

    /// Fraction of the sellable supply already sold, in basis points.
    pub fn progress_bps(&self) -> u64 {
        let sold = INITIAL_REAL_TOKEN_RESERVES.saturating_sub(self.real_token) as u128;
        (sold * 10_000 / INITIAL_REAL_TOKEN_RESERVES as u128) as u64
    }

    /// Loss at the floor for an entry at this state: `1 - (V0/V)^2` (ignores fees).
    pub fn floor_loss_fraction(&self) -> f64 {
        let r = INITIAL_VIRTUAL_SOL_RESERVES as f64 / self.virtual_sol as f64;
        1.0 - r * r
    }

    /// "Post-N-buys" simulation: apply a sequence of exact-SOL-in buys
    /// (each `spendable` includes fees) and return each quote with the state
    /// after it. Stops early (shorter output) when a buy would not fill
    /// within the curve's remaining supply.
    pub fn simulate_buys(&self, spendables: &[u64], fees: FeeBps) -> Vec<(BuyQuote, CurveState)> {
        let mut s = *self;
        let mut out = Vec::with_capacity(spendables.len());
        for &amt in spendables {
            let Some(q) = s.quote_buy_exact_sol_in(amt, fees) else {
                break;
            };
            s.apply_buy(q.tokens, q.sol_cost);
            out.push((q, s));
        }
        out
    }
}

// ----------------------------------------------------------------- accounts
/// pump `Global` account (IDL order, 2026-10-14 layout).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Global {
    pub initialized: bool,
    pub authority: Pubkey,
    pub fee_recipient: Pubkey,
    pub initial_virtual_token_reserves: u64,
    pub initial_virtual_sol_reserves: u64,
    pub initial_real_token_reserves: u64,
    pub token_total_supply: u64,
    pub fee_basis_points: u64,
    pub withdraw_authority: Pubkey,
    pub enable_migrate: bool,
    pub pool_migration_fee: u64,
    pub creator_fee_basis_points: u64,
    pub fee_recipients: [Pubkey; 7],
    pub set_creator_authority: Pubkey,
    pub admin_set_creator_authority: Pubkey,
    pub create_v2_enabled: bool,
    pub whitelist_pda: Pubkey,
    pub reserved_fee_recipient: Pubkey,
    pub mayhem_mode_enabled: bool,
    pub reserved_fee_recipients: [Pubkey; 7],
    pub is_cashback_enabled: bool,
    pub buyback_fee_recipients: [Pubkey; 8],
    pub buyback_basis_points: u64,
    pub initial_virtual_quote_reserves: u64,
    pub whitelisted_quote_mints: [Pubkey; 1],
    pub creator_fee_configurable: bool,
    pub max_configurable_creator_fee_bps: u64,
    pub holder_reward_claim_authority: Pubkey,
    pub is_holder_reward_enabled: bool,
    pub max_curve_depth: u8,
}

fn pubkeys<const N: usize>(r: &mut Reader<'_>) -> Result<[Pubkey; N]> {
    let mut out = [Pubkey::default(); N];
    for k in out.iter_mut() {
        *k = r.pubkey()?;
    }
    Ok(out)
}

impl Global {
    /// Decode account data (discriminator checked). Extra trailing bytes are ignored.
    pub fn decode(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        r.expect_discriminator(&acc_disc::GLOBAL)?;
        Ok(Self {
            initialized: r.bool()?,
            authority: r.pubkey()?,
            fee_recipient: r.pubkey()?,
            initial_virtual_token_reserves: r.u64()?,
            initial_virtual_sol_reserves: r.u64()?,
            initial_real_token_reserves: r.u64()?,
            token_total_supply: r.u64()?,
            fee_basis_points: r.u64()?,
            withdraw_authority: r.pubkey()?,
            enable_migrate: r.bool()?,
            pool_migration_fee: r.u64()?,
            creator_fee_basis_points: r.u64()?,
            fee_recipients: pubkeys(&mut r)?,
            set_creator_authority: r.pubkey()?,
            admin_set_creator_authority: r.pubkey()?,
            create_v2_enabled: r.bool()?,
            whitelist_pda: r.pubkey()?,
            reserved_fee_recipient: r.pubkey()?,
            mayhem_mode_enabled: r.bool()?,
            reserved_fee_recipients: pubkeys(&mut r)?,
            is_cashback_enabled: r.bool()?,
            buyback_fee_recipients: pubkeys(&mut r)?,
            buyback_basis_points: r.u64()?,
            initial_virtual_quote_reserves: r.u64()?,
            whitelisted_quote_mints: pubkeys(&mut r)?,
            creator_fee_configurable: r.bool()?,
            max_configurable_creator_fee_bps: r.u64()?,
            holder_reward_claim_authority: r.pubkey()?,
            is_holder_reward_enabled: r.bool()?,
            max_curve_depth: r.u8()?,
        })
    }

    /// `fee_recipient` followed by `fee_recipients`: the 8 normal recipients.
    pub fn all_fee_recipients(&self) -> [Pubkey; 8] {
        let mut out = [self.fee_recipient; 8];
        out[1..].copy_from_slice(&self.fee_recipients);
        out
    }

    /// `reserved_fee_recipient` followed by `reserved_fee_recipients` (mayhem coins).
    pub fn all_reserved_fee_recipients(&self) -> [Pubkey; 8] {
        let mut out = [self.reserved_fee_recipient; 8];
        out[1..].copy_from_slice(&self.reserved_fee_recipients);
        out
    }

    /// A fresh curve state from this `Global`'s initial reserves.
    pub fn fresh_curve(&self, creator: Pubkey) -> CurveState {
        CurveState {
            virtual_sol: self.initial_virtual_sol_reserves,
            virtual_token: self.initial_virtual_token_reserves,
            real_sol: 0,
            real_token: self.initial_real_token_reserves,
            complete: false,
            creator,
        }
    }
}

/// pump `BondingCurve` account. Older curves have a shorter layout: fields
/// past the end of the data decode to their default and `decoded_fields`
/// says how many IDL fields were present.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BondingCurve {
    pub virtual_token_reserves: u64,
    pub virtual_quote_reserves: u64,
    pub real_token_reserves: u64,
    pub real_quote_reserves: u64,
    pub token_total_supply: u64,
    pub complete: bool,
    pub creator: Pubkey,
    pub is_mayhem_mode: bool,
    pub is_cashback_coin: bool,
    /// `Pubkey::default()` for SOL-paired coins.
    pub quote_mint: Pubkey,
    pub creator_fee_bps: u64,
    pub can_edit_creator_fee: bool,
    pub is_holder_reward: bool,
    /// v3 creator fee kept on the curve (lamports or quote units).
    pub creator_fee: u64,
    /// v3 protocol fee kept on the curve.
    pub protocol_fees: u64,
    pub depth: u8,
    pub initial_virtual_quote_reserves: u64,
    pub post_complete_base_out: u64,
    pub post_complete_quote_in: u64,
    /// Number of IDL fields present in the account data (19 for the current layout).
    pub decoded_fields: u8,
}

impl BondingCurve {
    /// Number of fields in the 2026-10 IDL layout.
    pub const FIELDS: u8 = 19;

    /// Decode account data (discriminator checked). The first 6 fields are
    /// required; later fields are read while data remains.
    pub fn decode(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        r.expect_discriminator(&acc_disc::BONDING_CURVE)?;
        let mut bc = BondingCurve {
            virtual_token_reserves: r.u64()?,
            virtual_quote_reserves: r.u64()?,
            real_token_reserves: r.u64()?,
            real_quote_reserves: r.u64()?,
            token_total_supply: r.u64()?,
            complete: r.bool()?,
            decoded_fields: 6,
            ..Default::default()
        };
        macro_rules! opt {
            ($field:ident, $read:ident) => {
                match r.$read()? {
                    Some(v) => {
                        bc.$field = v;
                        bc.decoded_fields += 1;
                    }
                    None => return Ok(bc),
                }
            };
        }
        opt!(creator, opt_pubkey);
        opt!(is_mayhem_mode, opt_bool);
        opt!(is_cashback_coin, opt_bool);
        opt!(quote_mint, opt_pubkey);
        opt!(creator_fee_bps, opt_u64);
        opt!(can_edit_creator_fee, opt_bool);
        opt!(is_holder_reward, opt_bool);
        opt!(creator_fee, opt_u64);
        opt!(protocol_fees, opt_u64);
        opt!(depth, opt_u8);
        opt!(initial_virtual_quote_reserves, opt_u64);
        opt!(post_complete_base_out, opt_u64);
        opt!(post_complete_quote_in, opt_u64);
        Ok(bc)
    }

    /// The math view of this curve.
    pub fn state(&self) -> CurveState {
        CurveState::from_account(self)
    }

    /// `true` for a SOL-paired curve.
    pub fn is_sol_quoted(&self) -> bool {
        crate::constants::is_sol_like_quote_mint(&self.quote_mint)
    }
}

/// Pump Fees `Fees` triple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fees {
    pub lp_fee_bps: u64,
    pub protocol_fee_bps: u64,
    pub creator_fee_bps: u64,
}

/// One market-cap tier of a fee schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeeTier {
    pub market_cap_lamports_threshold: u128,
    pub fees: Fees,
}

/// Pump Fees `FeeConfig` account (`["fee_config", program]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeeConfig {
    pub bump: u8,
    pub admin: Pubkey,
    pub flat_fees: Fees,
    pub fee_tiers: Vec<FeeTier>,
    pub stable_fee_tiers: Vec<FeeTier>,
    pub exotic_flat_fees: Fees,
}

fn read_fees(r: &mut Reader<'_>) -> Result<Fees> {
    Ok(Fees {
        lp_fee_bps: r.u64()?,
        protocol_fee_bps: r.u64()?,
        creator_fee_bps: r.u64()?,
    })
}

fn read_tiers(r: &mut Reader<'_>) -> Result<Vec<FeeTier>> {
    let n = r.u32()? as usize;
    if n > 1024 {
        return Err(crate::error::DecodeError::InvalidLength {
            offset: r.pos(),
            value: n,
        });
    }
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        v.push(FeeTier {
            market_cap_lamports_threshold: r.u128()?,
            fees: read_fees(r)?,
        });
    }
    Ok(v)
}

impl FeeConfig {
    /// Decode account data (discriminator checked).
    pub fn decode(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        r.expect_discriminator(&acc_disc::FEE_CONFIG)?;
        let bump = r.u8()?;
        let admin = r.pubkey()?;
        let flat_fees = read_fees(&mut r)?;
        let fee_tiers = read_tiers(&mut r)?;
        let stable_fee_tiers = if r.is_empty() {
            Vec::new()
        } else {
            read_tiers(&mut r)?
        };
        let exotic_flat_fees = if r.is_empty() {
            Fees::default()
        } else {
            read_fees(&mut r)?
        };
        Ok(Self {
            bump,
            admin,
            flat_fees,
            fee_tiers,
            stable_fee_tiers,
            exotic_flat_fees,
        })
    }

    /// Highest tier whose threshold is `<= market_cap`, else the first tier.
    pub fn tier_for(tiers: &[FeeTier], market_cap_lamports: u128) -> Option<Fees> {
        let first = tiers.first()?;
        Some(
            tiers
                .iter()
                .rev()
                .find(|t| market_cap_lamports >= t.market_cap_lamports_threshold)
                .unwrap_or(first)
                .fees,
        )
    }

    /// Curve fee bps for a SOL-paired coin at `market_cap_lamports`
    /// (`fee_tiers`; USDC uses `stable_fee_tiers`, other quotes the exotic/flat fees).
    /// Schedule only: use [`Self::curve_fees_for_coin`] to apply a coin's own
    /// creator rate (custom pairs).
    pub fn curve_fees(&self, quote_mint: &Pubkey, market_cap_lamports: u128) -> Option<FeeBps> {
        let fees = if crate::constants::is_sol_like_quote_mint(quote_mint) {
            Self::tier_for(&self.fee_tiers, market_cap_lamports)?
        } else if *quote_mint == crate::constants::USDC_MINT {
            Self::tier_for(&self.stable_fee_tiers, market_cap_lamports)?
        } else if self.exotic_flat_fees != Fees::default() {
            self.exotic_flat_fees
        } else {
            self.flat_fees
        };
        Some(FeeBps {
            protocol: fees.protocol_fee_bps,
            creator: fees.creator_fee_bps,
        })
    }

    /// [`Self::curve_fees`] plus the program's per-coin creator rate: while
    /// `Global.creator_fee_configurable` is on, a nonzero
    /// `BondingCurve.creator_fee_bps` replaces the schedule's creator rate
    /// (custom-pair coins, e.g. 300 bps). Matches `pump-rust-client` 0.4.1
    /// `compute_bonding_curve_fee_bps`. The creator fee is still 0 on a curve
    /// whose `creator` is the zero key (the quote functions apply that).
    pub fn curve_fees_for_coin(
        &self,
        quote_mint: &Pubkey,
        market_cap_lamports: u128,
        coin_creator_fee_bps: u64,
        creator_fee_configurable: bool,
    ) -> Option<FeeBps> {
        let mut fees = self.curve_fees(quote_mint, market_cap_lamports)?;
        if creator_fee_configurable && coin_creator_fee_bps != 0 {
            fees.creator = coin_creator_fee_bps;
        }
        Some(fees)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const V0: u64 = INITIAL_VIRTUAL_SOL_RESERVES;
    const T0: u64 = INITIAL_VIRTUAL_TOKEN_RESERVES;

    #[test]
    fn worked_example_dev_buy_cost() {
        // pumpfun-mechanics-2026.md section 4: dev buy_v2 of 26,170,731,707,316
        // tokens on a fresh curve costs 750,000,000 lamports.
        let s = CurveState::fresh(Pubkey::new_unique());
        assert_eq!(s.buy_cost(26_170_731_707_316), Some(750_000_000));
        let q = s
            .quote_buy_exact_tokens(26_170_731_707_316, FeeBps::DEFAULT)
            .unwrap();
        assert_eq!(q.sol_cost, 750_000_000);
        assert_eq!(q.protocol_fee, 7_125_000);
        assert_eq!(q.creator_fee, 2_250_000);
    }

    #[test]
    fn worked_example_buy_tokens_for_sol() {
        // Next buy: 923,456,789 lamports into the curve after the dev buy.
        let mut s = CurveState::fresh(Pubkey::new_unique());
        s.apply_buy(26_170_731_707_316, 750_000_000);
        assert_eq!(s.tokens_for_sol(923_456_789), 30_520_874_344_997);
        // and the inverse: buying exactly that many tokens costs the same lamports.
        assert_eq!(s.buy_cost(30_520_874_344_997), Some(923_456_789));
    }

    #[test]
    fn worked_example_sell() {
        // Sell (tx 54rjuNFq...): post-trade V=33,464,263,370 T=961,921,667,068,915,
        // 64,688,838,765,391 tokens sold for 2,412,711,917 lamports.
        let post = CurveState {
            virtual_sol: 33_464_263_370,
            virtual_token: 961_921_667_068_915,
            real_sol: 3_464_263_370,
            real_token: 682_021_667_068_915,
            complete: false,
            creator: Pubkey::new_unique(),
        };
        let pre = CurveState::pre_trade(&post, false, 2_412_711_917, 64_688_838_765_391).unwrap();
        let q = pre.quote_sell(64_688_838_765_391, FeeBps::DEFAULT).unwrap();
        assert_eq!(q.sol_out, 2_412_711_917);
        assert_eq!(q.protocol_fee, 22_920_764);
        assert_eq!(q.creator_fee, 7_238_136);
        assert_eq!(buyback_fee(q.protocol_fee, DEFAULT_BUYBACK_BPS), 11_460_382);
    }

    #[test]
    fn floor_and_completion_constants() {
        assert_eq!(FLOOR_MARKET_CAP_LAMPORTS, 27_958_993_476);
        let s = CurveState::fresh(Pubkey::default());
        assert_eq!(
            s.market_cap_lamports(TOKEN_TOTAL_SUPPLY),
            FLOOR_MARKET_CAP_LAMPORTS as u128
        );
        assert_eq!(s.sol_to_complete(), Some(COMPLETION_REAL_SOL_FROM_FRESH));
        let mut done = s;
        done.apply_buy(INITIAL_REAL_TOKEN_RESERVES, COMPLETION_REAL_SOL_FROM_FRESH);
        assert!(done.complete);
        assert_eq!(done.virtual_token, COMPLETE_VIRTUAL_TOKEN_RESERVES);
        // ~410.88 SOL market cap at completion, 14.70x the floor.
        let mcap = done.market_cap_sol();
        assert!((mcap - 410.88).abs() < 0.01, "{mcap}");
        assert!((mcap / s.market_cap_sol() - 14.70).abs() < 0.01);
        assert_eq!(done.progress_bps(), 10_000);
    }

    #[test]
    fn price_impact_and_floor_loss() {
        let s = CurveState::fresh(Pubkey::default());
        // 10 SOL net from a fresh curve: (40/30)^2 = 1.778x.
        let imp = s.buy_price_impact(10 * LAMPORTS_PER_SOL);
        assert!((imp - (16.0 / 9.0 - 1.0)).abs() < 1e-6, "{imp}");
        let mut e = s;
        let tok = e.tokens_for_sol(40 * LAMPORTS_PER_SOL);
        e.apply_buy(tok, 40 * LAMPORTS_PER_SOL);
        assert!((e.floor_loss_fraction() - (1.0 - (30.0f64 / 70.0).powi(2))).abs() < 1e-12);
    }

    #[test]
    fn exact_in_split_matches_known_events() {
        // buy_exact_quote_in_v3 of 789,750,000: net 780,000,000, fees 7,410,000 / 2,340,000.
        assert_eq!(
            exact_in_split(789_750_000, FeeBps::DEFAULT),
            Some((780_000_000, 7_410_000, 2_340_000))
        );
        // buy_exact_sol_in of 25,092,090: net 24,782,311, fees 235,432 / 74,347.
        assert_eq!(
            exact_in_split(25_092_090, FeeBps::DEFAULT),
            Some((24_782_311, 235_432, 74_347))
        );
        assert_eq!(exact_in_split(0, FeeBps::DEFAULT), None);
    }

    #[test]
    fn simulate_buys_chains_states() {
        let s = CurveState::fresh(Pubkey::new_unique());
        let sims = s.simulate_buys(&[LAMPORTS_PER_SOL; 4], FeeBps::DEFAULT);
        assert_eq!(sims.len(), 4);
        let mut manual = s;
        for (q, after) in &sims {
            let q2 = manual
                .quote_buy_exact_sol_in(LAMPORTS_PER_SOL, FeeBps::DEFAULT)
                .unwrap();
            assert_eq!(*q, q2);
            manual.apply_buy(q2.tokens, q2.sol_cost);
            assert_eq!(*after, manual);
            assert_eq!(q.total, LAMPORTS_PER_SOL);
        }
        // Monotone price: later buys get fewer tokens.
        assert!(sims.windows(2).all(|w| w[1].0.tokens < w[0].0.tokens));
        // Too big a buy stops the simulation instead of over-filling.
        assert!(s
            .simulate_buys(&[200 * LAMPORTS_PER_SOL], FeeBps::DEFAULT)
            .is_empty());
        let _ = (V0, T0);
    }
}
