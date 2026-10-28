//! `/etc/rugsnipe/long.toml`: the long copy's own settings. The feed
//! endpoint, its token and the senders come from the hot unit's config and
//! key files (`hot_config`, `api_keys`), so both processes use the same
//! providers.

use serde::Deserialize;

/// What the process does with a trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Nothing (the process only reports).
    Off,
    /// Paper: a position on the live curve, journaled, nothing sent.
    #[default]
    Log,
    /// Real buys and sells from the long wallet.
    On,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LongCfg {
    pub mode: Mode,
    /// The wallet copied (H9kuqGKN).
    pub copy_wallet: String,
    /// The long wallet's keypair (JSON array, root-only).
    pub wallet_file: String,
    /// Its buy, valued in SOL, of at least this much: its 1.5-SOL tier
    /// (docs/20: 1.5 in the backtest; its tier reads 1.45-1.51 by the
    /// route's valuation).
    pub min_sol: f64,
    /// Our buy, valued in SOL.
    pub stake_sol: f64,
    /// Sell when the curve price (without our own tokens) reaches this
    /// multiple of the price its buy left.
    pub take_profit: f64,
    /// ... or falls to this multiple.
    pub stop: f64,
    /// ... or this long after its buy (seconds; it sells at 1800).
    pub max_hold_s: u64,
    /// Open positions at once (each `stake_sol`).
    pub max_open: usize,
    /// Our buy is refused above this premium over its average price
    /// (`min_out` from its own fill).
    pub max_premium: f64,
    /// No new buys once the UTC day's realized result is at or below
    /// `-daily_stop_sol` (exits continue).
    pub daily_stop_sol: f64,
    /// Mints held as floats besides wrapped SOL (WBTC ...): routes may start
    /// and end in them.
    pub floats: Vec<String>,
    /// Chain reads (blockhash, accounts, statuses) and simulation.
    pub rpc_url: String,
    /// The hot unit's config (its `[feeds] grpc` and `[senders]`).
    pub hot_config: String,
    /// The hot unit's API keys (gRPC token, sender keys).
    pub api_keys: String,
    /// Sender labels for buys: one transaction goes to each, so keep one
    /// (two landing would double the position).
    pub buy_senders: Vec<String>,
    /// Sender labels for sells (empty = every enabled sender; a second sell
    /// of the same position fails harmlessly).
    pub sell_senders: Vec<String>,
    /// Also send every transaction through `rpc_url` (`sendTransaction`).
    pub rpc_send: bool,
    pub buy_tip_lamports: u64,
    pub sell_tip_lamports: u64,
    /// Priority fee, micro-lamports per compute unit.
    pub buy_cu_price: u64,
    pub sell_cu_price: u64,
    pub cu_limit: u32,
    /// Sell `min_out` slack under the expected output: take profit, the
    /// others (stop, time, retries).
    pub sell_slack_tp: f64,
    pub sell_slack_exit: f64,
    /// Failed-sell fees (SOL) on one position past which its sells retry
    /// only every 10 min (they never stop).
    pub max_sell_fail_sol: f64,
    /// Positions, journal-only state, the kill file.
    pub state_dir: String,
    /// Float prices (Jupiter Price API v3; with a key: `https://api.jup.ag/price/v3`
    /// and `cred:` is not supported here, so the free endpoint by default).
    pub price_api: String,
}

impl Default for LongCfg {
    fn default() -> Self {
        Self {
            mode: Mode::Log,
            copy_wallet: "H9kuqGKNYgPFYCA17uUTkaqXGum7MK8y1GwbS7cxsjDg".into(),
            wallet_file: "/etc/rugsnipe/keys/long-wallet.json".into(),
            min_sol: 1.45,
            stake_sol: 1.0,
            take_profit: 2.0,
            stop: 0.8,
            max_hold_s: 1790,
            max_open: 4,
            max_premium: 0.25,
            daily_stop_sol: 3.0,
            floats: vec!["3NZ9JMVBmGAqocybic2c7LQCJScmgsAZ6vQqTDzcqmJh".into()],
            rpc_url: String::new(),
            hot_config: "/etc/rugsnipe/rugsnipe.toml".into(),
            api_keys: "/etc/rugsnipe/keys/api-keys.toml".into(),
            buy_senders: vec!["nozomi".into()],
            sell_senders: vec![],
            rpc_send: true,
            buy_tip_lamports: 1_000_000,
            sell_tip_lamports: 1_000_000,
            buy_cu_price: 2_000_000,
            sell_cu_price: 1_000_000,
            cu_limit: 300_000,
            sell_slack_tp: 0.30,
            sell_slack_exit: 0.50,
            max_sell_fail_sol: 0.01,
            state_dir: "/var/lib/rugsnipe-long".into(),
            price_api: crate::oracle::API.into(),
        }
    }
}

impl LongCfg {
    pub fn from_toml_str(s: &str) -> Result<Self, String> {
        let c: Self = toml::from_str(s).map_err(|e| e.to_string())?;
        c.validate()?;
        Ok(c)
    }

    pub fn validate(&self) -> Result<(), String> {
        let bad = |w: &str| Err(format!("long.toml: {w}"));
        if self.copy_wallet.parse::<pump_codec::Pubkey>().is_err() {
            return bad("copy_wallet is not a pubkey");
        }
        if !(0.01..=100.0).contains(&self.stake_sol) {
            return bad("stake_sol must be in [0.01, 100]");
        }
        if !(self.take_profit > 1.0 && self.take_profit < 100.0) {
            return bad("take_profit must be above 1");
        }
        if !(self.stop > 0.0 && self.stop < 1.0) {
            return bad("stop must be in (0, 1)");
        }
        if !(0.0..=1.0).contains(&self.max_premium) {
            return bad("max_premium must be in [0, 1]");
        }
        if !(0.0..1.0).contains(&self.sell_slack_tp) || !(0.0..1.0).contains(&self.sell_slack_exit)
        {
            return bad("sell slacks must be in [0, 1)");
        }
        if self.max_open == 0 || self.max_open > 50 {
            return bad("max_open must be in 1..=50");
        }
        if self.max_hold_s == 0 {
            return bad("max_hold_s must be positive");
        }
        for f in &self.floats {
            if f.parse::<pump_codec::Pubkey>().is_err() {
                return bad("floats must be mints");
            }
        }
        if self.mode == Mode::On && self.buy_senders.len() > 1 {
            return bad("one buy sender: two buys landing would double the position");
        }
        Ok(())
    }

    pub fn copy_pk(&self) -> pump_codec::Pubkey {
        self.copy_wallet.parse().expect("validated")
    }

    pub fn float_pks(&self) -> Vec<pump_codec::Pubkey> {
        self.floats.iter().filter_map(|f| f.parse().ok()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_validate_and_parse() {
        LongCfg::default().validate().unwrap();
        let c = LongCfg::from_toml_str("mode = \"on\"\nstake_sol = 0.5\nmax_open = 2\n").unwrap();
        assert_eq!(c.mode, Mode::On);
        assert_eq!(c.max_open, 2);
        assert!(LongCfg::from_toml_str("stop = 1.2").is_err());
        assert!(LongCfg::from_toml_str("mode = \"on\"\nbuy_senders = [\"a\", \"b\"]").is_err());
        assert!(LongCfg::from_toml_str("nope = 1").is_err());
    }
}
