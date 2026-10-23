//! Exit policy: the integer form used by [`crate::decide`] and the TOML form of
//! `[exit.default]` (docs/06 section 6) with per-operator overrides.
//!
//! Every ratio is in basis points (1.0 = 10_000) so the decision function is pure
//! integer arithmetic and can be mirrored bit-for-bit in Python.

use serde::{Deserialize, Serialize};
use std::fmt;

/// 1.0 in basis points.
pub const BPS: u64 = 10_000;
/// Maximum number of take-profit levels.
pub const MAX_TP_LEVELS: usize = 5;

/// What a take-profit level's multiple is measured on (`tp_basis`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TpBasis {
    /// The price multiple versus the fill, `(v / v_fill)^2` (docs/06 section 8).
    #[default]
    Price,
    /// The realised value of the stake: `(net sell quote of the balance +
    /// proceeds so far) / cost`, after fees and our own price impact
    /// (docs/04 section 2: "realised value >= +15% of the stake").
    Value,
}

/// When the normal triggers are evaluated (`fire_on`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FireOn {
    /// Once per slot, on the slot's closing state (docs/06 section 8).
    #[default]
    SlotClose,
    /// Also after every trade by someone else, on the state it leaves: the
    /// exit is sent on the first event that shows the trigger (docs/04
    /// section 2, the tape simulator's rule). Slot closes still evaluate.
    Trade,
}

/// Whose sells fire the BundleSell dump trigger (`insider_sell`). The engine
/// only checks [`crate::BundleSet`] membership; the caller fills the set
/// accordingly ([`InsiderSell::creator_set`], [`InsiderSell::bundle_wallets`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsiderSell {
    /// Any sell by a bundle wallet or the creator set (creator, create
    /// signers, fee payer): docs/04 section 6 `insider_sell = "bundle"`.
    #[default]
    Bundle,
    /// Bundle wallets outside the creator set only (the tape simulator's
    /// `ins_bundle` component).
    BundleOnly,
    /// The creator set only.
    Creator,
    /// No insider-sell trigger.
    Off,
}

impl InsiderSell {
    /// The creator set belongs in the bundle set.
    pub fn creator_set(self) -> bool {
        matches!(self, Self::Bundle | Self::Creator)
    }
    /// Bundle wallets outside the creator set belong in the bundle set.
    pub fn bundle_wallets(self) -> bool {
        matches!(self, Self::Bundle | Self::BundleOnly)
    }
}

/// Reference of the phantom stop (`phantom_sl.from`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhantomFrom {
    /// Our fill price, marked on the curve as it is (docs/06 section 8).
    #[default]
    Fill,
    /// The state just before our buy (the post-bundle price), marked on the
    /// curve without our own tokens: measures the others' selling only
    /// (docs/04 section 2, `drop` measured from the post-bundle price).
    Entry,
}

/// One take-profit level: sell `frac_bps` of the *remaining* balance once the
/// price multiple reaches `mult_bps`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TpLevel {
    pub mult_bps: u32,
    pub frac_bps: u32,
}

/// Up to [`MAX_TP_LEVELS`] levels, strictly increasing in `mult_bps`.
/// Serialized as a list of `[mult_bps, frac_bps]` pairs.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct TpLadder {
    levels: [TpLevel; MAX_TP_LEVELS],
    len: u8,
}

impl TpLadder {
    /// Build a ladder; fails on more than 5 levels, non-increasing multiples,
    /// a multiple <= 1.0 or a fraction outside (0, 1].
    pub fn new(levels: &[TpLevel]) -> Result<Self, PolicyError> {
        if levels.len() > MAX_TP_LEVELS {
            return Err(PolicyError::new("tp: more than 5 levels"));
        }
        let mut out = TpLadder::default();
        let mut last = BPS as u32;
        for (i, l) in levels.iter().enumerate() {
            if l.mult_bps <= last {
                return Err(PolicyError::new(
                    "tp: multiples must be > 1.0 and strictly increasing",
                ));
            }
            if l.frac_bps == 0 || u64::from(l.frac_bps) > BPS {
                return Err(PolicyError::new("tp: fraction must be in (0, 1]"));
            }
            last = l.mult_bps;
            out.levels[i] = *l;
        }
        out.len = levels.len() as u8;
        Ok(out)
    }

    /// The configured levels.
    pub fn levels(&self) -> &[TpLevel] {
        &self.levels[..self.len as usize]
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl fmt::Debug for TpLadder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.levels()).finish()
    }
}

impl Serialize for TpLadder {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let v: Vec<[u32; 2]> = self
            .levels()
            .iter()
            .map(|l| [l.mult_bps, l.frac_bps])
            .collect();
        v.serialize(s)
    }
}

impl<'de> Deserialize<'de> for TpLadder {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v: Vec<[u32; 2]> = Vec::deserialize(d)?;
        let levels: Vec<TpLevel> = v
            .iter()
            .map(|p| TpLevel {
                mult_bps: p[0],
                frac_bps: p[1],
            })
            .collect();
        TpLadder::new(&levels).map_err(serde::de::Error::custom)
    }
}

/// Integer exit policy consumed by [`crate::decide`]. A value of 0 disables
/// the corresponding optional trigger (trail, phantom SL, no-activity, max-hold,
/// consolidation, big-buy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExitPolicy {
    /// Take-profit ladder (`tp`).
    pub tp: TpLadder,
    /// Slippage below the trigger quote for normal sells (`tp_slip_bps`).
    pub tp_slip_bps: u32,
    /// Trailing stop arms once the peak multiple reaches this (`trail.activate`).
    pub trail_activate_bps: u32,
    /// Trailing stop fires at `price <= peak * (1 - retrace)` (`trail.retrace_bps`).
    pub trail_retrace_bps: u32,
    /// Phantom stop loss is armed `after_ms` after the open (`phantom_sl.after_ms`).
    pub phantom_after_ms: u64,
    /// Phantom stop loss fires at `mult <= 1 - bps` (`phantom_sl.bps`).
    pub phantom_bps: u32,
    /// No trade on the mint for this long (`no_activity_ms`).
    pub no_activity_ms: u64,
    /// Maximum holding time (`max_hold_ms`).
    pub max_hold_ms: u64,
    /// A bundle wallet selling at least this much SOL is a dump (`bundle_sell_min_lamports`).
    pub bundle_sell_min_lamports: u64,
    /// Transfers into one owner within the window reaching this % of supply (`consolidation_pct`).
    pub consolidation_pct: u32,
    /// Consolidation window in slots (2 per docs/06 section 8).
    pub consolidation_window_slots: u64,
    /// A single buy within `[lo, hi]` is a dump signal (`big_buy_lamports`; lo = 0 disables, hi = 0 = no upper bound).
    pub big_buy_lo_lamports: u64,
    pub big_buy_hi_lamports: u64,
    /// Sell retry cadence (`sell_retry.every_slots`).
    pub retry_every_slots: u64,
    /// Maximum attempts per order (`sell_retry.max_tries`, <= 15).
    pub retry_max_tries: u32,
    /// Tip multiplier per retry (`sell_retry.tip_mult`, 2.0 = 20_000).
    pub retry_tip_mult_bps: u32,
    /// First-attempt tip of a normal sell.
    pub sell_tip_lamports: u64,
    /// First-attempt tip of a dump sell.
    pub dump_tip_lamports: u64,
    /// Per-position cap on the sum of sell tips (`risk.max_sell_spend_lamports`).
    pub max_sell_spend_lamports: u64,
    /// A normal order switches to dump bounds after this many failed attempts.
    pub escalate_after_tries: u32,
    /// Virtual SOL of the floor state used for dump bounds (30 SOL).
    pub dump_floor_vsol: u64,
    /// Dump bound = floor quote x (1 - dump_slip) (0.98 -> 200).
    pub dump_slip_bps: u32,
    /// PumpSwap total fee assumed by the AMM quote (1.25% below 420 SOL market cap).
    pub amm_fee_bps: u32,
    /// After migration there is no floor: dump bound = pool quote x (1 - amm_dump_slip).
    pub amm_dump_slip_bps: u32,
    /// A remainder below this many base units is sold with the rest.
    pub dust_tokens: u64,
    /// Take-profit multiples measure the price or the realised value (`tp_basis`).
    #[serde(default)]
    pub tp_basis: TpBasis,
    /// Normal triggers on slot close only, or also after every trade (`fire_on`).
    #[serde(default)]
    pub fire_on: FireOn,
    /// Whose sells are a BundleSell (`insider_sell`; `off` disables the trigger).
    #[serde(default)]
    pub insider_sell: InsiderSell,
    /// Phantom stop reference (`phantom_sl.from`).
    #[serde(default)]
    pub phantom_from: PhantomFrom,
    /// Hard exit once the slot reaches `opened_slot + max_hold_slots` (0 = off).
    #[serde(default)]
    pub max_hold_slots: u64,
    /// Sell everything once the slot reaches the launch's create slot + this
    /// (`hold_to_create_slots`; `None` = off). Counted from the create, not
    /// from the landing: the operator-wave rule (docs/09) holds each
    /// operator's launches to a fixed slot after their create. The hot loop
    /// overrides it per position from the operator table
    /// (`entry.operators_file`). The order runs at dump bounds (it sells at
    /// that slot whatever the price, like the copied wallet's
    /// `min_sol_output = 0` sells) without being a dump signal.
    #[serde(default)]
    pub hold_to_create_slot: Option<u16>,
}

impl Default for ExitPolicy {
    /// docs/06 section 6 `[exit.default]` with `max_sell_spend_lamports = 20_000_000`.
    fn default() -> Self {
        ExitConfig::default()
            .to_policy(20_000_000)
            .expect("default exit config is valid")
    }
}

impl ExitPolicy {
    /// Apply per-operator overrides (from the ArmSet / backtest) on top of this policy.
    pub fn with_overrides(&self, o: &ExitOverrides) -> Result<ExitPolicy, PolicyError> {
        let base = ExitConfig::from_policy(self);
        base.merged(o).to_policy(self.max_sell_spend_lamports)
    }

    /// Basic sanity checks (also run by [`ExitConfig::to_policy`]).
    pub fn validate(&self) -> Result<(), PolicyError> {
        if u64::from(self.tp_slip_bps) >= BPS {
            return Err(PolicyError::new("tp_slip_bps must be < 10000"));
        }
        if u64::from(self.trail_retrace_bps) >= BPS || u64::from(self.phantom_bps) >= BPS {
            return Err(PolicyError::new("retrace/phantom bps must be < 10000"));
        }
        if self.trail_activate_bps != 0 && u64::from(self.trail_activate_bps) <= BPS {
            return Err(PolicyError::new("trail.activate must be > 1.0"));
        }
        if self.consolidation_pct > 100 {
            return Err(PolicyError::new("consolidation_pct must be <= 100"));
        }
        if self.big_buy_hi_lamports != 0 && self.big_buy_hi_lamports < self.big_buy_lo_lamports {
            return Err(PolicyError::new("big_buy_lamports: hi < lo"));
        }
        if self.retry_every_slots == 0 || self.retry_max_tries == 0 || self.retry_max_tries > 15 {
            return Err(PolicyError::new(
                "sell_retry: every_slots >= 1 and 1 <= max_tries <= 15",
            ));
        }
        if u64::from(self.retry_tip_mult_bps) < BPS {
            return Err(PolicyError::new("sell_retry.tip_mult must be >= 1.0"));
        }
        if self.max_sell_spend_lamports == 0 {
            return Err(PolicyError::new("max_sell_spend_lamports must be set"));
        }
        if self.hold_to_create_slot == Some(0) {
            return Err(PolicyError::new(
                "hold_to_create_slot must be >= 1 (None = off)",
            ));
        }
        if self.dump_floor_vsol == 0
            || u64::from(self.dump_slip_bps) >= BPS
            || u64::from(self.amm_dump_slip_bps) >= BPS
            || u64::from(self.amm_fee_bps) >= BPS
        {
            return Err(PolicyError::new("dump bounds out of range"));
        }
        Ok(())
    }
}

/// Policy validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyError(pub String);

impl PolicyError {
    fn new(s: &str) -> Self {
        PolicyError(s.to_string())
    }
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid exit policy: {}", self.0)
    }
}

impl std::error::Error for PolicyError {}

// ------------------------------------------------------------- TOML form
/// `trail = { activate, retrace_bps }`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TrailCfg {
    pub activate: f64,
    pub retrace_bps: u32,
}

/// `phantom_sl = { after_ms, bps }` or `{ after_ms, drop }` (a fraction;
/// `drop` defaults the reference to `from = "entry"`, the post-bundle price;
/// `bps` to `from = "fill"`). `bps = 0` without `drop` disables the stop.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhantomCfg {
    pub after_ms: u64,
    #[serde(default)]
    pub bps: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drop: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<PhantomFrom>,
}

impl PhantomCfg {
    /// `(bps, reference)` after resolving `drop` / `from`.
    pub fn resolve(&self) -> Result<(u32, PhantomFrom), PolicyError> {
        match self.drop {
            Some(d) => {
                if self.bps != 0 {
                    return Err(PolicyError::new("phantom_sl: give bps or drop, not both"));
                }
                if !d.is_finite() || !(0.0..1.0).contains(&d) {
                    return Err(PolicyError::new("phantom_sl.drop must be in [0, 1)"));
                }
                Ok((
                    (d * BPS as f64).round() as u32,
                    self.from.unwrap_or(PhantomFrom::Entry),
                ))
            }
            None => Ok((self.bps, self.from.unwrap_or(PhantomFrom::Fill))),
        }
    }
}

/// `sell_retry = { every_slots, max_tries, tip_mult }`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RetryCfg {
    pub every_slots: u64,
    pub max_tries: u32,
    pub tip_mult: f64,
}

/// `[exit.default]` exactly as written in `config/rugsnipe.example.toml`
/// (multiples and fractions as floats). Extra knobs not in the spec table
/// have defaults and may be set too.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExitConfig {
    pub tp: Vec<[f64; 2]>,
    pub tp_slip_bps: u32,
    pub trail: TrailCfg,
    pub phantom_sl: PhantomCfg,
    pub no_activity_ms: u64,
    pub max_hold_ms: u64,
    pub bundle_sell_min_lamports: u64,
    pub consolidation_pct: u32,
    pub big_buy_lamports: [u64; 2],
    pub sell_retry: RetryCfg,
    #[serde(default = "d_window")]
    pub consolidation_window_slots: u64,
    #[serde(default = "d_sell_tip")]
    pub sell_tip_lamports: u64,
    #[serde(default = "d_dump_tip")]
    pub dump_tip_lamports: u64,
    #[serde(default = "d_escalate")]
    pub escalate_after_tries: u32,
    #[serde(default = "d_floor")]
    pub dump_floor_vsol: u64,
    #[serde(default = "d_dump_slip")]
    pub dump_slip_bps: u32,
    #[serde(default = "d_amm_fee")]
    pub amm_fee_bps: u32,
    #[serde(default = "d_amm_slip")]
    pub amm_dump_slip_bps: u32,
    #[serde(default = "d_dust")]
    pub dust_tokens: u64,
    /// `"price"` (default) or `"value"` (realised value of the stake).
    #[serde(default)]
    pub tp_basis: TpBasis,
    /// `"slot_close"` (default) or `"trade"`.
    #[serde(default)]
    pub fire_on: FireOn,
    /// `"bundle"` (default), `"bundle_only"`, `"creator"` or `"off"`.
    #[serde(default)]
    pub insider_sell: InsiderSell,
    /// Hard exit at `landing slot + max_hold_slots` (0 = off; docs/04 section 2 uses 5).
    #[serde(default)]
    pub max_hold_slots: u64,
    /// Sell everything at `create slot + hold_to_create_slots` (0 = off; at
    /// most 65,535). The default for launches whose create signer is not in
    /// the operator table (`entry.operators_file`); a listed signer's
    /// `hold_slots` replaces it per position (docs/09).
    #[serde(default)]
    pub hold_to_create_slots: u64,
}

fn d_window() -> u64 {
    2
}
fn d_sell_tip() -> u64 {
    50_000
}
fn d_dump_tip() -> u64 {
    200_000
}
fn d_escalate() -> u32 {
    3
}
fn d_floor() -> u64 {
    30_000_000_000
}
fn d_dump_slip() -> u32 {
    200
}
fn d_amm_fee() -> u32 {
    125
}
fn d_amm_slip() -> u32 {
    5_000
}
fn d_dust() -> u64 {
    1_000_000
}

impl Default for ExitConfig {
    /// docs/06 section 6 `[exit.default]`.
    fn default() -> Self {
        ExitConfig {
            tp: vec![[2.0, 0.5], [3.0, 1.0]],
            tp_slip_bps: 1500,
            trail: TrailCfg {
                activate: 1.5,
                retrace_bps: 2500,
            },
            phantom_sl: PhantomCfg {
                after_ms: 10_000,
                bps: 4000,
                drop: None,
                from: None,
            },
            no_activity_ms: 150_000,
            max_hold_ms: 1_800_000,
            bundle_sell_min_lamports: 100_000_000,
            consolidation_pct: 20,
            big_buy_lamports: [0, 0],
            sell_retry: RetryCfg {
                every_slots: 2,
                max_tries: 15,
                tip_mult: 2.0,
            },
            consolidation_window_slots: d_window(),
            sell_tip_lamports: d_sell_tip(),
            dump_tip_lamports: d_dump_tip(),
            escalate_after_tries: d_escalate(),
            dump_floor_vsol: d_floor(),
            dump_slip_bps: d_dump_slip(),
            amm_fee_bps: d_amm_fee(),
            amm_dump_slip_bps: d_amm_slip(),
            dust_tokens: d_dust(),
            tp_basis: TpBasis::Price,
            fire_on: FireOn::SlotClose,
            insider_sell: InsiderSell::Bundle,
            max_hold_slots: 0,
            hold_to_create_slots: 0,
        }
    }
}

/// Float -> basis points, rounded half away from zero (deterministic).
fn to_bps(x: f64, what: &str) -> Result<u32, PolicyError> {
    if !x.is_finite() || !(0.0..=1_000.0).contains(&x) {
        return Err(PolicyError(format!("{what}: {x} out of range")));
    }
    Ok((x * BPS as f64).round() as u32)
}

impl ExitConfig {
    /// Convert to the integer policy. `max_sell_spend_lamports` comes from `[risk]`.
    pub fn to_policy(&self, max_sell_spend_lamports: u64) -> Result<ExitPolicy, PolicyError> {
        let mut levels = Vec::with_capacity(self.tp.len());
        for p in &self.tp {
            levels.push(TpLevel {
                mult_bps: to_bps(p[0], "tp multiple")?,
                frac_bps: to_bps(p[1], "tp fraction")?,
            });
        }
        let (phantom_bps, phantom_from) = self.phantom_sl.resolve()?;
        let hold_to_create_slot = match self.hold_to_create_slots {
            0 => None,
            h => Some(
                u16::try_from(h)
                    .map_err(|_| PolicyError::new("hold_to_create_slots must be <= 65535"))?,
            ),
        };
        let p = ExitPolicy {
            tp: TpLadder::new(&levels)?,
            tp_slip_bps: self.tp_slip_bps,
            trail_activate_bps: to_bps(self.trail.activate, "trail.activate")?,
            trail_retrace_bps: self.trail.retrace_bps,
            phantom_after_ms: self.phantom_sl.after_ms,
            phantom_bps,
            no_activity_ms: self.no_activity_ms,
            max_hold_ms: self.max_hold_ms,
            bundle_sell_min_lamports: self.bundle_sell_min_lamports,
            consolidation_pct: self.consolidation_pct,
            consolidation_window_slots: self.consolidation_window_slots.max(1),
            big_buy_lo_lamports: self.big_buy_lamports[0],
            big_buy_hi_lamports: self.big_buy_lamports[1],
            retry_every_slots: self.sell_retry.every_slots,
            retry_max_tries: self.sell_retry.max_tries,
            retry_tip_mult_bps: to_bps(self.sell_retry.tip_mult, "sell_retry.tip_mult")?,
            sell_tip_lamports: self.sell_tip_lamports,
            dump_tip_lamports: self.dump_tip_lamports,
            max_sell_spend_lamports,
            escalate_after_tries: self.escalate_after_tries,
            dump_floor_vsol: self.dump_floor_vsol,
            dump_slip_bps: self.dump_slip_bps,
            amm_fee_bps: self.amm_fee_bps,
            amm_dump_slip_bps: self.amm_dump_slip_bps,
            dust_tokens: self.dust_tokens,
            tp_basis: self.tp_basis,
            fire_on: self.fire_on,
            insider_sell: self.insider_sell,
            phantom_from,
            max_hold_slots: self.max_hold_slots,
            hold_to_create_slot,
        };
        p.validate()?;
        Ok(p)
    }

    /// The float form of an integer policy (inverse of [`Self::to_policy`] up to rounding).
    pub fn from_policy(p: &ExitPolicy) -> Self {
        let b = BPS as f64;
        ExitConfig {
            tp: p
                .tp
                .levels()
                .iter()
                .map(|l| [l.mult_bps as f64 / b, l.frac_bps as f64 / b])
                .collect(),
            tp_slip_bps: p.tp_slip_bps,
            trail: TrailCfg {
                activate: p.trail_activate_bps as f64 / b,
                retrace_bps: p.trail_retrace_bps,
            },
            phantom_sl: PhantomCfg {
                after_ms: p.phantom_after_ms,
                bps: p.phantom_bps,
                drop: None,
                from: Some(p.phantom_from),
            },
            no_activity_ms: p.no_activity_ms,
            max_hold_ms: p.max_hold_ms,
            bundle_sell_min_lamports: p.bundle_sell_min_lamports,
            consolidation_pct: p.consolidation_pct,
            big_buy_lamports: [p.big_buy_lo_lamports, p.big_buy_hi_lamports],
            sell_retry: RetryCfg {
                every_slots: p.retry_every_slots,
                max_tries: p.retry_max_tries,
                tip_mult: p.retry_tip_mult_bps as f64 / b,
            },
            consolidation_window_slots: p.consolidation_window_slots,
            sell_tip_lamports: p.sell_tip_lamports,
            dump_tip_lamports: p.dump_tip_lamports,
            escalate_after_tries: p.escalate_after_tries,
            dump_floor_vsol: p.dump_floor_vsol,
            dump_slip_bps: p.dump_slip_bps,
            amm_fee_bps: p.amm_fee_bps,
            amm_dump_slip_bps: p.amm_dump_slip_bps,
            dust_tokens: p.dust_tokens,
            tp_basis: p.tp_basis,
            fire_on: p.fire_on,
            insider_sell: p.insider_sell,
            max_hold_slots: p.max_hold_slots,
            hold_to_create_slots: p.hold_to_create_slot.map_or(0, u64::from),
        }
    }

    /// This config with `o`'s set fields replacing the defaults.
    pub fn merged(&self, o: &ExitOverrides) -> ExitConfig {
        let mut c = self.clone();
        macro_rules! take {
            ($($f:ident),*) => { $( if let Some(v) = &o.$f { c.$f = v.clone(); } )* };
        }
        take!(
            tp,
            tp_slip_bps,
            trail,
            phantom_sl,
            no_activity_ms,
            max_hold_ms,
            bundle_sell_min_lamports,
            consolidation_pct,
            big_buy_lamports,
            sell_retry,
            sell_tip_lamports,
            dump_tip_lamports,
            escalate_after_tries,
            tp_basis,
            fire_on,
            insider_sell,
            max_hold_slots,
            hold_to_create_slots
        );
        c
    }
}

/// Per-operator overrides (same shape as [`ExitConfig`], every field optional),
/// e.g. `[exit.op.123]` or the ArmSet's `exit` object.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExitOverrides {
    pub tp: Option<Vec<[f64; 2]>>,
    pub tp_slip_bps: Option<u32>,
    pub trail: Option<TrailCfg>,
    pub phantom_sl: Option<PhantomCfg>,
    pub no_activity_ms: Option<u64>,
    pub max_hold_ms: Option<u64>,
    pub bundle_sell_min_lamports: Option<u64>,
    pub consolidation_pct: Option<u32>,
    pub big_buy_lamports: Option<[u64; 2]>,
    pub sell_retry: Option<RetryCfg>,
    pub sell_tip_lamports: Option<u64>,
    pub dump_tip_lamports: Option<u64>,
    pub escalate_after_tries: Option<u32>,
    pub tp_basis: Option<TpBasis>,
    pub fire_on: Option<FireOn>,
    pub insider_sell: Option<InsiderSell>,
    pub max_hold_slots: Option<u64>,
    pub hold_to_create_slots: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_spec_table() {
        let p = ExitPolicy::default();
        assert_eq!(
            p.tp.levels(),
            &[
                TpLevel {
                    mult_bps: 20_000,
                    frac_bps: 5_000
                },
                TpLevel {
                    mult_bps: 30_000,
                    frac_bps: 10_000
                }
            ]
        );
        assert_eq!(p.tp_slip_bps, 1500);
        assert_eq!(p.trail_activate_bps, 15_000);
        assert_eq!(p.trail_retrace_bps, 2500);
        assert_eq!((p.phantom_after_ms, p.phantom_bps), (10_000, 4000));
        assert_eq!(p.no_activity_ms, 150_000);
        assert_eq!(p.max_hold_ms, 1_800_000);
        assert_eq!(p.bundle_sell_min_lamports, 100_000_000);
        assert_eq!(p.consolidation_pct, 20);
        assert_eq!((p.big_buy_lo_lamports, p.big_buy_hi_lamports), (0, 0));
        assert_eq!(
            (p.retry_every_slots, p.retry_max_tries, p.retry_tip_mult_bps),
            (2, 15, 20_000)
        );
        assert_eq!(p.max_sell_spend_lamports, 20_000_000);
    }

    #[test]
    fn ladder_validation() {
        let l = |m, f| TpLevel {
            mult_bps: m,
            frac_bps: f,
        };
        assert!(TpLadder::new(&[l(20_000, 5000), l(15_000, 5000)]).is_err());
        assert!(TpLadder::new(&[l(10_000, 5000)]).is_err());
        assert!(TpLadder::new(&[l(20_000, 0)]).is_err());
        assert!(TpLadder::new(&[l(20_000, 10_001)]).is_err());
        assert!(TpLadder::new(&[l(11_000, 1); 6]).is_err());
        let ok = TpLadder::new(&[
            l(12_000, 1000),
            l(15_000, 2000),
            l(20_000, 3000),
            l(30_000, 5000),
            l(50_000, 10_000),
        ])
        .unwrap();
        assert_eq!(ok.len(), 5);
        let js = serde_json::to_string(&ok).unwrap();
        assert_eq!(
            js,
            "[[12000,1000],[15000,2000],[20000,3000],[30000,5000],[50000,10000]]"
        );
        let back: TpLadder = serde_json::from_str(&js).unwrap();
        assert_eq!(back, ok);
        assert!(serde_json::from_str::<TpLadder>("[[9000,1000]]").is_err());
    }

    #[test]
    fn overrides_apply_over_defaults() {
        let o = ExitOverrides {
            tp: Some(vec![[1.5, 0.3], [2.5, 0.5], [4.0, 1.0]]),
            big_buy_lamports: Some([2_000_000_000, 0]),
            ..Default::default()
        };
        let p = ExitPolicy::default().with_overrides(&o).unwrap();
        assert_eq!(p.tp.len(), 3);
        assert_eq!(p.tp.levels()[0].mult_bps, 15_000);
        assert_eq!(p.big_buy_lo_lamports, 2_000_000_000);
        // untouched fields keep the defaults
        assert_eq!(p.no_activity_ms, 150_000);
        assert_eq!(p.max_sell_spend_lamports, 20_000_000);
        let bad = ExitOverrides {
            tp: Some(vec![[0.5, 1.0]]),
            ..Default::default()
        };
        assert!(ExitPolicy::default().with_overrides(&bad).is_err());
    }

    #[test]
    fn missing_spend_cap_is_refused() {
        assert!(ExitConfig::default().to_policy(0).is_err());
    }

    /// The operator-wave exit: every other rule can be switched off from
    /// the TOML alone (`tp = []`, `insider_sell = "off"`, `no_activity_ms =
    /// 0`, `phantom_sl` at 0, `max_hold_slots` 0 or very large, `max_hold_ms`
    /// 0) next to `hold_to_create_slots`.
    #[test]
    fn hold_to_create_with_every_other_rule_off_is_valid() {
        let toml = r#"
tp = []
tp_slip_bps = 2500
trail = { activate = 0.0, retrace_bps = 0 }
phantom_sl = { after_ms = 0, drop = 0.0 }
no_activity_ms = 0
max_hold_ms = 0
max_hold_slots = 1_000_000
bundle_sell_min_lamports = 0
insider_sell = "off"
consolidation_pct = 0
big_buy_lamports = [0, 0]
sell_retry = { every_slots = 2, max_tries = 15, tip_mult = 2.0 }
hold_to_create_slots = 6
"#;
        let c: ExitConfig = toml::from_str(toml).unwrap();
        let p = c.to_policy(20_000_000).unwrap();
        assert!(p.tp.is_empty());
        assert_eq!(p.insider_sell, InsiderSell::Off);
        assert_eq!((p.no_activity_ms, p.max_hold_ms, p.phantom_bps), (0, 0, 0));
        assert_eq!(p.max_hold_slots, 1_000_000);
        assert_eq!(p.hold_to_create_slot, Some(6));
        // the bps form of the stop, and max_hold_slots = 0, are accepted too
        let c2: ExitConfig = toml::from_str(
            &toml
                .replace("drop = 0.0", "bps = 0")
                .replace("max_hold_slots = 1_000_000", "max_hold_slots = 0"),
        )
        .unwrap();
        assert_eq!(c2.to_policy(20_000_000).unwrap().phantom_bps, 0);
        // 0 = off; the float form round-trips; the u16 range is enforced
        let off: ExitConfig =
            toml::from_str(&toml.replace("hold_to_create_slots = 6", "")).unwrap();
        assert_eq!(off.to_policy(20_000_000).unwrap().hold_to_create_slot, None);
        assert_eq!(
            ExitConfig::from_policy(&p).to_policy(20_000_000).unwrap(),
            p
        );
        let big = ExitConfig {
            hold_to_create_slots: 70_000,
            ..c.clone()
        };
        assert!(big.to_policy(20_000_000).is_err());
        let o = ExitOverrides {
            hold_to_create_slots: Some(10),
            ..Default::default()
        };
        assert_eq!(p.with_overrides(&o).unwrap().hold_to_create_slot, Some(10));
        // an old policy_json without the field reads as off
        let mut js: serde_json::Value = serde_json::to_value(p).unwrap();
        js.as_object_mut().unwrap().remove("hold_to_create_slot");
        let back: ExitPolicy = serde_json::from_value(js).unwrap();
        assert_eq!(back.hold_to_create_slot, None);
    }

    #[test]
    fn roundtrip_float_form() {
        let p = ExitPolicy::default();
        let back = ExitConfig::from_policy(&p).to_policy(p.max_sell_spend_lamports);
        assert_eq!(back.unwrap(), p);
    }
}
