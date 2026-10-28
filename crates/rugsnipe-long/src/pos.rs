//! Positions: from the trigger to the close, the exit rule, the paper fill
//! and the file that keeps them across restarts.
//!
//! **The exit rule** (docs/20): the curve price against the price its buy
//! left, without our own tokens (`(vt_ref / (vt + ours))^2`): take profit
//! at `take_profit`, stop at `stop`, and `max_hold_s` after its buy.
//! A paper position's tokens are not out of the live curve, so nothing is
//! added back.

use pump_codec::multihop::{Hop, Route, Venue};
use pump_codec::Pubkey;
use serde::{Deserialize, Serialize};

/// Where a position is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// The buy is out (paper: waiting for the landing slot to end).
    Buying,
    Open,
    /// A sell is out.
    Selling,
    Closed,
    /// The buy did not land, or failed.
    Failed,
}

/// Why it sold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reason {
    Tp,
    Stop,
    Time,
    /// The curve completed (the coin graduates): out at once.
    Complete,
    /// It sold the coin before any rule of ours fired: out behind it (the
    /// backtest's price path ends at its sale, so its copies sell there).
    #[serde(rename = "its_sell")]
    ItsSell,
    /// By hand (`rugsnipe-long sell`).
    Manual,
}

impl Reason {
    pub fn name(self) -> &'static str {
        match self {
            Self::Tp => "take_profit",
            Self::Stop => "stop",
            Self::Time => "time",
            Self::Complete => "complete",
            Self::ItsSell => "its_sell",
            Self::Manual => "manual",
        }
    }
}

/// A hop, stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HopRec {
    pub curve: bool,
    pub accounts: [String; 5],
}

/// See the module docs. Keys are base58 strings (the file is read by
/// `check.sh` too).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub id: u64,
    pub paper: bool,
    pub mint: String,
    pub quote_mint: String,
    #[serde(default)]
    pub quote_program: String,
    pub coin_program: String,
    pub anchor: String,
    pub anchor_program: String,
    pub route_input: String,
    pub route: Vec<HopRec>,
    pub how: String,
    /// Its buy.
    pub its_sig: String,
    pub its_slot: u64,
    pub its_lamports: u64,
    pub its_quote_in: u64,
    pub its_tokens: u64,
    /// Unix ms we saw its buy (the hold clock starts there).
    pub seen_ms: u64,
    /// The curve after its buy: the exits' reference.
    pub vt_ref: u64,
    pub vq_ref: u64,
    /// Our side: the anchor we put in, its SOL value, the tokens we got.
    pub amount_in: u64,
    pub stake_lamports: u64,
    pub tokens: u64,
    pub state: State,
    pub buy_sig: Option<String>,
    pub buy_sent_ms: u64,
    pub sell_sigs: Vec<String>,
    pub sell_sent_ms: u64,
    pub sell_tries: u32,
    pub reason: Option<Reason>,
    /// The latest curve state seen and when.
    pub vt: u64,
    pub vq: u64,
    pub last_slot: u64,
    pub peak: f64,
    /// Out: anchor received, its SOL value, the result after every fee and
    /// tip we paid (lamports, signed).
    pub out_anchor: u64,
    pub out_lamports: u64,
    pub net_lamports: i64,
    pub closed_ms: u64,
    /// Lamports paid in fees on failed transactions of this position (live).
    pub costs: u64,
    /// Quote our curve trade paid (live: from our fill; paper: estimated):
    /// with `amount_in`, the rate of the route's other hops.
    #[serde(default)]
    pub quote_in: u64,
    /// Live: no sell is sent before this (unix ms): failed sells back off
    /// ([`sell_backoff_ms`]).
    #[serde(default)]
    pub sell_next_ms: u64,
    /// Live: the buy was given up unseen (not landed in time). If it lands
    /// after all, the position reopens on its tokens.
    #[serde(default)]
    pub dropped: bool,
    /// The coin's curve completed (seen on a trade of it, in any state): its
    /// sells go to the canonical PumpSwap pool.
    #[serde(default)]
    pub graduated: bool,
    /// Paper: the slot whose end state the buy fills on (0 on a record from
    /// before it: its slot + 1).
    #[serde(default)]
    pub land_slot: u64,
}

/// How long to wait before the next sell of a position after `tries` failed
/// ones that cost `costs` lamports: the first three go at once (the exit
/// matters and the floor drops with each), then 5 s, 30 s and 2 min; past
/// `cap` lamports of failed fees, every 10 min. Never given up: the coins
/// stay sold-for eventually, but a sell that keeps failing (2026-10-27, the
/// hot bot's 2RR6sfbi: 405 failed sells, 0.09 SOL) can no longer spend fast.
pub fn sell_backoff_ms(tries: u32, costs: u64, cap: u64) -> u64 {
    if cap > 0 && costs >= cap {
        return 600_000;
    }
    match tries {
        0..=2 => 0,
        3..=5 => 5_000,
        6..=9 => 30_000,
        _ => 120_000,
    }
}

impl Position {
    /// Paper: the slot whose end state the buy fills on.
    pub fn fill_slot(&self) -> u64 {
        if self.land_slot > 0 {
            self.land_slot
        } else {
            self.its_slot + 1
        }
    }
    pub fn mint_pk(&self) -> Pubkey {
        self.mint.parse().unwrap_or_default()
    }
    pub fn pk(s: &str) -> Pubkey {
        s.parse().unwrap_or_default()
    }

    /// The buy route, rebuilt.
    pub fn buy_route(&self) -> Option<Route> {
        let hops = self
            .route
            .iter()
            .map(|h| {
                let mut a = [Pubkey::default(); 5];
                for (k, s) in h.accounts.iter().enumerate() {
                    a[k] = s.parse().ok()?;
                }
                let mut hop = Hop::from_accounts(a);
                hop.venue = if h.curve { Venue::Curve } else { Venue::Pool };
                Some(hop)
            })
            .collect::<Option<Vec<_>>>()?;
        Route::new(self.route_input.parse().ok()?, hops).ok()
    }

    pub fn store_route(r: &Route) -> (String, Vec<HopRec>) {
        (
            r.input.to_string(),
            r.hops
                .iter()
                .map(|h| HopRec {
                    curve: h.venue == Venue::Curve,
                    accounts: h.accounts().map(|k| k.to_string()),
                })
                .collect(),
        )
    }

    /// Counts against `max_open`.
    pub fn live(&self) -> bool {
        matches!(self.state, State::Buying | State::Open | State::Selling)
    }

    /// The price ratio now (without our tokens on a real position).
    pub fn price(&self) -> f64 {
        let ours = if self.paper { 0 } else { self.tokens };
        pump_codec::multihop::curve_price_ratio(self.vt_ref, self.vt, ours)
    }
}

/// The exit due now, if any.
pub fn exit_due(
    p: &Position,
    now_ms: u64,
    take_profit: f64,
    stop: f64,
    max_hold_s: u64,
) -> Option<Reason> {
    if p.state != State::Open {
        return None;
    }
    let px = p.price();
    if px >= take_profit {
        Some(Reason::Tp)
    } else if px <= stop && px > 0.0 {
        Some(Reason::Stop)
    } else if now_ms.saturating_sub(p.seen_ms) >= max_hold_s * 1000 {
        Some(Reason::Time)
    } else {
        None
    }
}

/// Curve buy with `quote_in` (fee off the input) at `(vt, vq)`: tokens out.
pub fn curve_buy(vt: u64, vq: u64, quote_in: u64, fee_bps: u32) -> u64 {
    pump_codec::multihop::cp_out(vq, vt, quote_in, fee_bps)
}

/// Selling `tokens` into a live curve at `(vt, vq)` that already has them
/// out (a real position: our buy is in the curve's state): the quote that
/// comes back, the fee off the output.
pub fn curve_sell_live(vt: u64, vq: u64, tokens: u64, fee_bps: u32) -> u64 {
    if tokens == 0 || vt == 0 {
        return 0;
    }
    let k = vt as u128 * vq as u128;
    let gross = (vq as u128).saturating_sub(k / (vt as u128 + tokens as u128));
    (gross * (10_000 - fee_bps.min(10_000) as u128) / 10_000) as u64
}

/// Selling `tokens` that the live curve at `(vt, vq)` never saw (a paper
/// position): the curve as it would be with them out is `vt - tokens`, and
/// selling brings it back to `vt`.
pub fn curve_sell_paper(vt: u64, vq: u64, tokens: u64, fee_bps: u32) -> u64 {
    if tokens == 0 || vt == 0 {
        return 0;
    }
    let k = vt as u128 * vq as u128;
    let vt_with = (vt as u128).saturating_sub(tokens as u128).max(1);
    let gross = (k / vt_with).saturating_sub(vq as u128);
    (gross * (10_000 - fee_bps.min(10_000) as u128) / 10_000) as u64
}

/// The positions file (`positions.json`, rewritten whole on each change).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Book {
    pub next_id: u64,
    pub positions: Vec<Position>,
}

impl Book {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Ok(s) => serde_json::from_str(&s).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    pub fn live(&self) -> impl Iterator<Item = &Position> {
        self.positions.iter().filter(|p| p.live())
    }

    /// Closed in the UTC day of `now_ms`: the sum of results (lamports).
    pub fn realized_today(&self, now_ms: u64, paper: bool) -> i64 {
        let day = now_ms / 86_400_000;
        self.positions
            .iter()
            .filter(|p| {
                p.paper == paper && p.state == State::Closed && p.closed_ms / 86_400_000 == day
            })
            .map(|p| p.net_lamports)
            .sum()
    }

    /// Drop closed positions older than `keep_ms` (the journal keeps them).
    pub fn prune(&mut self, now_ms: u64, keep_ms: u64) {
        self.positions
            .retain(|p| p.live() || now_ms.saturating_sub(p.closed_ms.max(p.seen_ms)) < keep_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_sells_back_off_and_the_cap_slows_them_to_ten_minutes() {
        assert_eq!(sell_backoff_ms(0, 0, 10_000_000), 0);
        assert_eq!(sell_backoff_ms(2, 600_000, 10_000_000), 0);
        assert_eq!(sell_backoff_ms(3, 900_000, 10_000_000), 5_000);
        assert_eq!(sell_backoff_ms(7, 2_100_000, 10_000_000), 30_000);
        assert_eq!(sell_backoff_ms(12, 3_600_000, 10_000_000), 120_000);
        assert_eq!(sell_backoff_ms(40, 10_000_000, 10_000_000), 600_000);
        // no cap: never slower than 2 min
        assert_eq!(sell_backoff_ms(1_000, u64::MAX, 0), 120_000);
        // the worst hour after the cap: 6 failed sells (about 0.002 SOL)
        assert_eq!(3_600_000 / sell_backoff_ms(50, 20_000_000, 10_000_000), 6);
    }

    fn pos() -> Position {
        Position {
            id: 1,
            paper: false,
            mint: String::new(),
            quote_mint: String::new(),
            quote_program: String::new(),
            coin_program: String::new(),
            anchor: String::new(),
            anchor_program: String::new(),
            route_input: String::new(),
            route: vec![],
            how: String::new(),
            its_sig: String::new(),
            its_slot: 0,
            its_lamports: 0,
            its_quote_in: 0,
            its_tokens: 0,
            seen_ms: 1_000,
            vt_ref: 1_000_000,
            vq_ref: 1_000,
            amount_in: 0,
            stake_lamports: 0,
            tokens: 0,
            state: State::Open,
            buy_sig: None,
            buy_sent_ms: 0,
            sell_sigs: vec![],
            sell_sent_ms: 0,
            sell_tries: 0,
            reason: None,
            vt: 1_000_000,
            vq: 1_000,
            last_slot: 0,
            peak: 1.0,
            out_anchor: 0,
            out_lamports: 0,
            net_lamports: 0,
            closed_ms: 0,
            costs: 0,
            quote_in: 0,
            sell_next_ms: 0,
            dropped: false,
            graduated: false,
            land_slot: 0,
        }
    }

    #[test]
    fn exits_on_the_price_without_our_tokens() {
        let mut p = pos();
        assert_eq!(exit_due(&p, 2_000, 2.0, 0.8, 1790), None);
        // the curve at 2x: vt down by 1/sqrt(2)
        p.vt = (1_000_000.0 / 2f64.sqrt()) as u64 - 10;
        assert_eq!(exit_due(&p, 2_000, 2.0, 0.8, 1790), Some(Reason::Tp));
        // our own tokens out of the curve do not count: the same vt with
        // 50k of ours held is under 2x
        p.tokens = 50_000;
        assert_eq!(exit_due(&p, 2_000, 2.0, 0.8, 1790), None);
        // a paper position's tokens are not out of the curve
        p.paper = true;
        assert_eq!(exit_due(&p, 2_000, 2.0, 0.8, 1790), Some(Reason::Tp));
        p.paper = false;
        p.tokens = 0;
        p.vt = (1_000_000.0 / 0.8f64.sqrt()) as u64 + 10;
        assert_eq!(exit_due(&p, 2_000, 2.0, 0.8, 1790), Some(Reason::Stop));
        p.vt = 1_000_000;
        assert_eq!(
            exit_due(&p, 1_000 + 1_790_000, 2.0, 0.8, 1790),
            Some(Reason::Time)
        );
        p.state = State::Selling;
        assert_eq!(exit_due(&p, 1_000 + 1_790_000, 2.0, 0.8, 1790), None);
    }

    #[test]
    fn a_buy_and_sell_on_an_unmoved_curve_returns_less_the_fees() {
        let (vt, vq) = (1_000_000_000_000u64, 30_000_000_000u64);
        let t = curve_buy(vt, vq, 1_000_000_000, 100);
        // live: our buy is in the curve, which is now at vt - t
        let back = curve_sell_live(vt - t, vq + 990_000_000, t, 100);
        assert!(back < 1_000_000_000 && back > 970_000_000, "{back}");
        // paper: the curve never saw our buy
        let back = curve_sell_paper(vt, vq, t, 100);
        assert!(back < 1_000_000_000 && back > 970_000_000, "{back}");
    }

    #[test]
    fn the_book_saves_and_loads_and_counts_today() {
        let dir = std::env::temp_dir().join(format!("long-book-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("positions.json");
        let mut b = Book::load(&path).unwrap();
        let mut p = pos();
        p.state = State::Closed;
        p.net_lamports = -5;
        p.closed_ms = 86_400_000 * 3 + 5;
        b.positions.push(p.clone());
        b.save(&path).unwrap();
        let b2 = Book::load(&path).unwrap();
        assert_eq!(b2.positions, vec![p]);
        assert_eq!(b2.realized_today(86_400_000 * 3 + 100, false), -5);
        assert_eq!(b2.realized_today(86_400_000 * 4, false), 0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
