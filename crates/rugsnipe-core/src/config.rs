//! The rugsnipe TOML (docs/06 section 6; `config/rugsnipe.example.toml`)
//! and its validation.
//!
//! [`Config::validate`] returns **every** refusal reason at once, not the
//! first. Paper mode needs no keys and no ArmSet; probe and live are refused
//! when any of these holds (docs/06 section 6):
//!
//! * a risk cap is 0 or missing, or `tx.cu_limit = 0` / `tx.lads_bytes = 0`;
//! * a key credential is missing, or group- or world-readable;
//! * the clock offset is unknown or above `max_clock_offset_ms`;
//! * no healthy post-execution feed;
//! * the ArmSet is unsigned / bad / stale / wrong regime;
//! * probe: the ArmSet does not carry `probe_allowed = true`; live: it does
//!   not carry `gate_passed = true` (probe does **not** need the gate: the
//!   probe is how the gate's landing inputs get measured);
//! * the `[rpc_budget]` role sum is above 32;
//! * (live only) no `--live` CLI flag or no valid local `ctl arm-live` token.
//!
//! Unknown keys are rejected (`deny_unknown_fields`) so a misspelled cap
//! cannot silently read as "missing"; `[exit]` is passed through raw to the
//! exit engine. The same file also carries the Python-only knobs
//! (`[rpc_budget] per_host / per_method / methods`, `[collector]`), typed
//! and validated here so one file serves both sides. Parsing is pure
//! (`from_toml_str`); the caller reads the file.

use crate::mode::Mode;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Upper bound on the sum of `[rpc_budget]` (requests/s per public-RPC IP).
pub const RPC_BUDGET_MAX: u32 = 32;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    #[serde(rename = "box")]
    pub box_: BoxCfg,
    pub mode: ModeCfg,
    pub secrets: SecretsCfg,
    pub risk: RiskCfg,
    pub feeds: FeedsCfg,
    pub senders: SendersCfg,
    pub rpc_budget: RpcBudgetCfg,
    pub tx: TxCfg,
    pub bid: BidCfg,
    /// Raw `[exit.*]` tables, interpreted by `rugsnipe-exit`.
    pub exit: toml::Table,
    pub guard: GuardCfg,
    pub watch: WatchCfg,
    pub obs: ObsCfg,
    pub ab: AbCfg,
    /// Entry rule of the lead policy (docs/04 sections 2 and 6).
    pub entry: EntryCfg,
    /// Paper (shadow) fill model and cost accounting (docs/04 section 7).
    pub paper: PaperCfg,
    /// Python collector knobs (`analysis/rugsnipe_analysis/collector.py`);
    /// unused by the Rust binaries.
    pub collector: CollectorCfg,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct BoxCfg {
    pub id: String,
    pub role: String,
    pub singleton: bool,
}
impl Default for BoxCfg {
    fn default() -> Self {
        Self {
            id: "local".into(),
            role: "hot".into(),
            singleton: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ModeCfg {
    pub mode: Mode,
    /// Inclusive slot ranges where buys are forced to paper (Alpenglow cutover).
    pub regime_freeze_slots: Vec<[u64; 2]>,
    pub max_age_slots: u64,
}
impl Default for ModeCfg {
    fn default() -> Self {
        Self {
            mode: Mode::Paper,
            regime_freeze_slots: vec![],
            max_age_slots: 2,
        }
    }
}
impl ModeCfg {
    /// `true` if `slot` falls in a freeze window.
    pub fn is_frozen(&self, slot: u64) -> bool {
        self.regime_freeze_slots
            .iter()
            .any(|[a, b]| (*a..=*b).contains(&slot))
    }
}

/// systemd credential **names** (never key material) and two public keys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct SecretsCfg {
    pub hot_keypair: String,
    pub probe_keypair: String,
    pub api_keys: String,
    /// Base58 ed25519 key that signs ArmSets (lives on ohio1 only).
    pub armset_pubkey: String,
    /// Base58 destination of daily sweeps.
    pub cold_wallet: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RiskCfg {
    pub max_snipe_lamports: u64,
    pub max_probe_lamports: u64,
    pub max_probes_per_day: u64,
    pub max_open_positions: u64,
    pub max_daily_loss_lamports: u64,
    pub max_consec_losses_per_op: u64,
    pub max_buys_per_op_per_day: u64,
    pub max_buys_per_hour: u64,
    pub max_bid_lamports: u64,
    pub max_sell_spend_lamports: u64,
    pub max_entry_vsol_lamports: u64,
    pub max_hot_balance_lamports: u64,
    pub min_hot_balance_lamports: u64,
    pub kill_file: String,
    pub armset_max_age_s: u64,
    pub decode_miss_kill_ratio: f64,
    pub send_error_kill_ratio: f64,
    pub max_clock_offset_ms: f64,
}
impl Default for RiskCfg {
    /// Every cap 0 = missing (probe/live refused); paper works.
    fn default() -> Self {
        Self {
            max_snipe_lamports: 0,
            max_probe_lamports: 0,
            max_probes_per_day: 0,
            max_open_positions: 0,
            max_daily_loss_lamports: 0,
            max_consec_losses_per_op: 0,
            max_buys_per_op_per_day: 0,
            max_buys_per_hour: 0,
            max_bid_lamports: 0,
            max_sell_spend_lamports: 0,
            max_entry_vsol_lamports: 0,
            max_hot_balance_lamports: 0,
            min_hot_balance_lamports: 0,
            kill_file: "/run/rugsnipe/KILL".into(),
            armset_max_age_s: 21_600,
            decode_miss_kill_ratio: 0.01,
            send_error_kill_ratio: 0.20,
            max_clock_offset_ms: 5.0,
        }
    }
}
impl RiskCfg {
    /// `(name, value)` of every cap that must be non-zero for probe/live.
    pub fn caps(&self) -> [(&'static str, u64); 13] {
        [
            ("max_snipe_lamports", self.max_snipe_lamports),
            ("max_probe_lamports", self.max_probe_lamports),
            ("max_probes_per_day", self.max_probes_per_day),
            ("max_open_positions", self.max_open_positions),
            ("max_daily_loss_lamports", self.max_daily_loss_lamports),
            ("max_consec_losses_per_op", self.max_consec_losses_per_op),
            ("max_buys_per_op_per_day", self.max_buys_per_op_per_day),
            ("max_buys_per_hour", self.max_buys_per_hour),
            ("max_bid_lamports", self.max_bid_lamports),
            ("max_sell_spend_lamports", self.max_sell_spend_lamports),
            ("max_entry_vsol_lamports", self.max_entry_vsol_lamports),
            ("max_hot_balance_lamports", self.max_hot_balance_lamports),
            ("min_hot_balance_lamports", self.min_hot_balance_lamports),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LogsWsCfg {
    pub enabled: bool,
    pub url: String,
    pub stale_ms: u64,
}
impl Default for LogsWsCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            url: "wss://api.mainnet-beta.solana.com".into(),
            stale_ms: 10_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct RelayCfg {
    pub enabled: bool,
    pub socket: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct GrpcCfg {
    pub enabled: bool,
    pub endpoint: String,
    /// Credential name of the token.
    pub token: String,
    pub stale_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct ShredsCfg {
    pub enabled: bool,
    pub bind: String,
    pub sources: Vec<String>,
}

/// PumpPortal `subscribeNewToken` (ONE connection per IP; default off: the
/// collector usually holds it and relays frames).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PumpPortalCfg {
    pub enabled: bool,
    pub url: String,
}
impl Default for PumpPortalCfg {
    fn default() -> Self {
        Self {
            enabled: false,
            url: "wss://pumpportal.fun/api/data".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct FeedsCfg {
    pub logsws: LogsWsCfg,
    pub relay: RelayCfg,
    pub grpc: GrpcCfg,
    pub shreds: ShredsCfg,
    pub pumpportal: PumpPortalCfg,
    /// Feed class required before probe/live may fire (`post_exec`).
    pub probe_live_require: String,
}
impl Default for FeedsCfg {
    fn default() -> Self {
        Self {
            logsws: LogsWsCfg::default(),
            relay: RelayCfg::default(),
            grpc: GrpcCfg::default(),
            shreds: ShredsCfg::default(),
            pumpportal: PumpPortalCfg::default(),
            probe_live_require: "post_exec".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Toggle {
    pub enabled: bool,
}
impl Default for Toggle {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RpcSendCfg {
    pub enabled: bool,
    pub url: String,
    pub sells_only: bool,
}
impl Default for RpcSendCfg {
    fn default() -> Self {
        Self {
            enabled: false,
            url: "https://api.mainnet-beta.solana.com".into(),
            sells_only: true,
        }
    }
}

/// One HTTP sender. `key` is a credential **name** inside the `api_keys`
/// credential, never the key itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct HttpSenderCfg {
    pub enabled: bool,
    pub url: String,
    pub key: String,
    /// Lamports; 0 = the provider default in `rugsnipe-tx`.
    pub min_tip: u64,
    /// Helius only: `?swqos_only=true` (5,000-lamport minimum tip).
    pub swqos_only: bool,
    /// Jito only: `"auto"` or a block-engine region.
    pub region: String,
    /// Requests per second allowed per IP (Jito, keyless Helius: 1).
    pub max_rps: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct TpuCfg {
    pub enabled: bool,
    pub leaders_ahead: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct SendersCfg {
    pub paper: Toggle,
    pub rpcsend: RpcSendCfg,
    pub helius: HttpSenderCfg,
    pub nozomi: HttpSenderCfg,
    pub astralane: HttpSenderCfg,
    pub jito: HttpSenderCfg,
    pub nextblock: HttpSenderCfg,
    pub tpu: TpuCfg,
}
impl SendersCfg {
    /// Names of enabled senders that can move money (everything but paper).
    pub fn real_enabled(&self) -> Vec<&'static str> {
        let mut v = vec![];
        if self.rpcsend.enabled {
            v.push("rpcsend");
        }
        for (n, s) in [
            ("helius", &self.helius),
            ("nozomi", &self.nozomi),
            ("astralane", &self.astralane),
            ("jito", &self.jito),
            ("nextblock", &self.nextblock),
        ] {
            if s.enabled {
                v.push(n);
            }
        }
        if self.tpu.enabled {
            v.push("tpu");
        }
        v
    }
}

/// Requests/s per role against public RPC (per egress IP), plus the
/// per-host / per-(host, method) caps the Python token buckets apply
/// (`analysis/rugsnipe_analysis/rpcbudget.py`). Only the five roles count
/// toward the sum rule (<= [`RPC_BUDGET_MAX`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RpcBudgetCfg {
    pub hot: u32,
    pub guard: u32,
    pub keeper: u32,
    pub collector: u32,
    pub backfill: u32,
    /// Cap per (endpoint host, method), req/s (public mainnet-beta allows 40 / 10 s per method).
    pub per_method: f64,
    /// Cap per endpoint host, all methods, req/s.
    pub per_host: f64,
    /// Per-method overrides of `per_method` (`[rpc_budget.methods]`, e.g. `getBlock = 2`).
    pub methods: BTreeMap<String, f64>,
}
impl Default for RpcBudgetCfg {
    fn default() -> Self {
        Self {
            hot: 4,
            guard: 4,
            keeper: 8,
            collector: 12,
            backfill: 4,
            per_method: 4.0,
            per_host: 10.0,
            methods: BTreeMap::new(),
        }
    }
}
impl RpcBudgetCfg {
    /// Sum of the five role budgets (the sum rule).
    pub fn sum(&self) -> u32 {
        self.hot + self.guard + self.keeper + self.collector + self.backfill
    }
    /// The role names the Python budget accepts.
    pub const ROLES: [&'static str; 5] = ["hot", "guard", "keeper", "collector", "backfill"];
}

/// `[collector]`: overrides of the Python collector's `CollectorConfig`
/// (`analysis/rugsnipe_analysis/collector.py`). Every key is optional; an
/// absent key keeps the Python default. Unknown keys are refused here, so a
/// typo that Python would silently ignore is caught by `ctl config validate`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct CollectorCfg {
    /// SQLite path of `analytics.db`.
    pub db_path: Option<String>,
    /// Stop after this many seconds (unset = run forever).
    pub duration_s: Option<f64>,
    #[serde(rename = "box")]
    pub box_: Option<String>,
    /// Unix socket the collector relays PumpPortal frames on.
    pub relay_socket: Option<String>,
    pub pp_url: Option<String>,
    pub logs_url: Option<String>,
    pub rpc_endpoints: Option<Vec<String>>,
    /// `[rpc_budget]` role the collector draws from (default `collector`).
    pub budget_role: Option<String>,
    pub config_path: Option<String>,
    pub pp_stale_s: Option<f64>,
    pub logs_stale_s: Option<f64>,
    pub decode_delay_s: Option<f64>,
    pub funding_delay_s: Option<f64>,
    pub rivals_delay_s: Option<f64>,
    pub tape_delay_s: Option<f64>,
    pub tape_early_s: Option<u64>,
    pub tape_early_pages: Option<u64>,
    /// 0 = the signer's own funding only; N = trace N hops up.
    pub funding_hops: Option<u64>,
    /// Oldest pre-create transactions scanned for incoming SOL.
    pub funding_scan: Option<u64>,
    pub cache_tx: Option<bool>,
    pub workers: Option<u64>,
    /// pump.fun frontend API requests/s.
    pub api_rps: Option<f64>,
    pub stats_every_s: Option<f64>,
}

/// Wire format of the buy template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TxFormat {
    #[default]
    Legacy,
    V1,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct TxCfg {
    pub format: TxFormat,
    /// **Floor** of the per-mint compute-unit limit: the hot path patches
    /// `65,000 + 1,500*S` per mint (S = Σ (255 - bump) over the four snipe
    /// PDAs, docs/research/gap-5.md), clamped to `[cu_limit, 1,400,000]`;
    /// sells use it as their static limit. 88,000 recommended (a static
    /// 88,000 fails 0.075% of mints; the per-mint value covers those). 0 =
    /// probe/live refused until `rugsnipe-ctl sim` confirms the fit.
    pub cu_limit: u32,
    /// Loaded-accounts data size limit (12,124,160 = 370 pages).
    pub lads_bytes: u32,
    pub nonce_pool: u16,
    pub sell_nonce_pool: u16,
    pub burn_after_slots: u64,
    pub partial_fill: bool,
}
impl Default for TxCfg {
    fn default() -> Self {
        Self {
            format: TxFormat::Legacy,
            cu_limit: 0,
            lads_bytes: 0,
            nonce_pool: 32,
            sell_nonce_pool: 8,
            burn_after_slots: 3,
            partial_fill: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct BidSplit {
    pub jito_agave: [f64; 2],
    pub harmonic: [f64; 2],
    pub bam: [f64; 2],
}
impl Default for BidSplit {
    fn default() -> Self {
        Self {
            jito_agave: [0.7, 0.3],
            harmonic: [1.0, 0.0],
            bam: [0.5, 0.5],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct BidCfg {
    pub quantile: f64,
    /// `[priority, tip]` fractions per leader class.
    pub split: BidSplit,
}
impl Default for BidCfg {
    fn default() -> Self {
        Self {
            quantile: 0.75,
            split: BidSplit::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct GuardCfg {
    pub takeover_after_ms: u64,
    pub drop_from_peak_bps: u64,
    pub grace_s: u64,
}
impl Default for GuardCfg {
    fn default() -> Self {
        Self {
            takeover_after_ms: 5_000,
            drop_from_peak_bps: 3_000,
            grace_s: 60,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct WatchCfg {
    pub default_ttl_s: u64,
    pub remove_if_no_launch_s: u64,
    pub last_transfer_min_frac: f64,
}
impl Default for WatchCfg {
    fn default() -> Self {
        Self {
            default_ttl_s: 1_800,
            remove_if_no_launch_s: 3_600,
            last_transfer_min_frac: 0.9,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct AlertCfg {
    pub telegram: String,
    pub deadman: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ObsCfg {
    pub metrics_listen: Vec<String>,
    pub rslog_dir: String,
    pub alert: AlertCfg,
}
impl Default for ObsCfg {
    fn default() -> Self {
        Self {
            metrics_listen: vec!["127.0.0.1:9464".into()],
            rslog_dir: "/var/lib/rugsnipe/rslog".into(),
            alert: AlertCfg::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct AbCfg {
    pub skip_p: f64,
    pub small_arm: [f64; 2],
    pub small_lamports: u64,
    pub size_jitter: f64,
}
impl Default for AbCfg {
    fn default() -> Self {
        Self {
            skip_p: 0.2,
            small_arm: [0.2, 0.35],
            small_lamports: 50_000_000,
            size_jitter: 0.10,
        }
    }
}

/// Which launches the entry takes (`entry.filter`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryFilter {
    /// Every create that matches the watchlist.
    #[default]
    Any,
    /// The creation slot holds a bundle with at least one wallet outside the
    /// creator set (docs/04 section 2; gap-3 bundle definition).
    MultiBundle,
}

/// `[entry]` (docs/04 section 6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct EntryCfg {
    /// Fill cap over the post-bundle price (`max_sol_cost` bound): a fill
    /// that would pay more fails (cheaply) instead.
    pub max_premium: f64,
    pub filter: EntryFilter,
}
impl Default for EntryCfg {
    fn default() -> Self {
        Self {
            max_premium: 0.02,
            filter: EntryFilter::Any,
        }
    }
}

/// Paper fill model (`paper.fill_model`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FillModel {
    /// We land in the creation slot right behind the creator's bundle and
    /// pay `entry_premium` over the post-bundle state (docs/04 section 2).
    #[default]
    E1,
    /// The curve state at the end of slot+1 (what a free feed can reach).
    E2,
}

impl FillModel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::E1 => "e1",
            Self::E2 => "e2",
        }
    }
}

/// `[paper]`: how paper buys and sells are filled and costed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PaperCfg {
    pub fill_model: FillModel,
    /// E1 premium over the post-bundle price (gap-2: +1.1% median for the
    /// first reactive buyer). A third-party buy interleaved in the bundle run
    /// raises it to that buy's price impact.
    pub entry_premium: f64,
    /// Priority fee a failed buy still pays (lost race, slippage cap).
    pub failed_attempt_lamports: u64,
    /// Fixed cost of a landed round trip (priority + tips), charged in the
    /// session summary's net P&L (docs/04: 0.004 SOL base).
    pub round_trip_lamports: u64,
    /// A paper sell fills at the curve state at the end of trigger slot + N.
    pub exit_latency_slots: u64,
    /// E1 positions are replayed from confirmed history on a slot clock:
    /// `ms = (slot - landing slot) * slot_ms` (gap-3 tapes: 217 ms after Oct 9).
    pub slot_ms: u64,
    /// Slots after the creation slot the E1 lane fetches (the exits of the
    /// lead policy all fire by landing + 5 and fill one slot later).
    pub replay_slots: u64,
    /// Wait after the decision before reading confirmed history.
    pub delay_ms: u64,
    /// A create in the last fraction of its block cannot be followed in the
    /// same slot (gap-2): the attempt fails as `not_in_slot0`.
    pub late_block_frac: f64,
    /// Transactions fetched per intent (slot 0 first, then the replay slots).
    pub max_tx: u32,
    /// A fill needs its tape complete through `s + min_replay_slots` (the
    /// lead exits fire by landing + 5 and fill one slot later); a shorter
    /// tape (fetch cap, RPC errors) makes the intent `unresolved`.
    pub min_replay_slots: u64,
}
impl Default for PaperCfg {
    fn default() -> Self {
        Self {
            fill_model: FillModel::E1,
            entry_premium: 0.011,
            failed_attempt_lamports: 3_000_000,
            round_trip_lamports: 4_000_000,
            exit_latency_slots: 1,
            slot_ms: 217,
            replay_slots: 12,
            delay_ms: 8_000,
            late_block_frac: 0.10,
            max_tx: 100,
            min_replay_slots: 7,
        }
    }
}

// ------------------------------------------------------------- validation

/// What the caller learned about the ArmSet (see [`crate::armset`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArmSetGate {
    #[default]
    Missing,
    Unsigned,
    BadSignature,
    ShaMismatch,
    Stale,
    WrongRegime,
    /// Signature and sha verified, fresh, right regime.
    Verified {
        /// The go/no-go gate passed (docs/06 section 9.5): required for live.
        gate_passed: bool,
        /// Ohio allows capped probe buys (docs/06 section 10): required
        /// for probe; the gate is not.
        probe_allowed: bool,
    },
}

/// Facts from outside the config file (stat results, clock, feed health).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ValidateEnv {
    /// Measured clock offset (chrony); `None` = unknown.
    pub clock_offset_ms: Option<f64>,
    pub post_exec_feed_healthy: bool,
    pub armset: ArmSetGate,
    /// Credential name -> file mode bits (`None` = missing).
    pub credential_modes: BTreeMap<String, Option<u32>>,
    /// `--live` was given on the command line.
    pub live_cli_flag: bool,
    /// A valid local `ctl arm-live` token for the gate run exists.
    pub arm_live_token: bool,
}

/// One reason the configuration (or the environment) is refused.
#[derive(Debug, Clone, PartialEq)]
pub enum Refusal {
    /// A field is invalid in every mode.
    Invalid {
        field: &'static str,
        why: String,
    },
    CapMissing(&'static str),
    CuLimitZero,
    LadsZero,
    CredentialMissing(String),
    CredentialPermissive {
        name: String,
        mode: u32,
    },
    ClockUnknown,
    ClockOffset {
        ms: f64,
        max: f64,
    },
    NoPostExecFeed,
    ArmSet(ArmSetGate),
    GateNotPassed,
    ProbeNotAllowed,
    RpcBudget {
        sum: u32,
    },
    BadPubkey {
        field: &'static str,
    },
    LiveFlagMissing,
    ArmLiveTokenMissing,
}

impl core::fmt::Display for Refusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Invalid { field, why } => write!(f, "{field}: {why}"),
            Self::CapMissing(n) => write!(f, "risk.{n} is 0 or missing"),
            Self::CuLimitZero => write!(
                f,
                "tx.cu_limit = 0 (the per-mint CU floor; run `rugsnipe-ctl sim`, then set 88,000)"
            ),
            Self::LadsZero => write!(f, "tx.lads_bytes = 0"),
            Self::CredentialMissing(n) => write!(f, "credential {n:?} missing"),
            Self::CredentialPermissive { name, mode } => {
                write!(
                    f,
                    "credential {name:?} is group/world-accessible (mode {mode:o})"
                )
            }
            Self::ClockUnknown => write!(f, "clock offset unknown"),
            Self::ClockOffset { ms, max } => write!(f, "clock offset {ms} ms > {max} ms"),
            Self::NoPostExecFeed => write!(f, "no healthy post-execution feed"),
            Self::ArmSet(s) => write!(f, "ArmSet not usable: {s:?}"),
            Self::GateNotPassed => write!(
                f,
                "ArmSet gate_passed = false (live needs the go/no-go gate)"
            ),
            Self::ProbeNotAllowed => write!(f, "ArmSet probe_allowed = false (probe needs it)"),
            Self::RpcBudget { sum } => write!(f, "rpc_budget sum {sum} > {RPC_BUDGET_MAX}"),
            Self::BadPubkey { field } => write!(f, "{field} is not a base58 pubkey"),
            Self::LiveFlagMissing => write!(f, "live mode needs the --live CLI flag"),
            Self::ArmLiveTokenMissing => write!(f, "live mode needs a valid `ctl arm-live` token"),
        }
    }
}

/// Result of [`Config::validate`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Validation {
    pub mode: Mode,
    pub refusals: Vec<Refusal>,
    pub warnings: Vec<String>,
}

impl Validation {
    pub fn ok(&self) -> bool {
        self.refusals.is_empty()
    }
}

impl Config {
    /// Parse TOML (unknown keys are an error). `[exit]` is kept raw for the
    /// exit engine (which refuses unknown keys inside `[exit.default]`), so
    /// an unknown table name under it (`[exit.defualt]`, which the hot path
    /// would silently replace with the default exits) is refused here.
    pub fn from_toml_str(s: &str) -> Result<Self, String> {
        let cfg: Self = toml::from_str(s).map_err(|e| e.to_string())?;
        if let Some(k) = cfg.exit.keys().find(|k| k.as_str() != "default") {
            return Err(format!(
                "unknown table [exit.{k}]: only [exit.default] is read"
            ));
        }
        Ok(cfg)
    }

    /// Every refusal reason for running in `self.mode.mode` given `env`.
    pub fn validate(&self, env: &ValidateEnv) -> Validation {
        let mode = self.mode.mode;
        let mut r = vec![];
        let mut w = vec![];
        let inv = |r: &mut Vec<Refusal>, field: &'static str, why: &str| {
            r.push(Refusal::Invalid {
                field,
                why: why.to_string(),
            })
        };
        // ---- every mode
        if self.mode.max_age_slots == 0 {
            inv(&mut r, "mode.max_age_slots", "must be >= 1");
        }
        if self.mode.regime_freeze_slots.iter().any(|[a, b]| a > b) {
            inv(&mut r, "mode.regime_freeze_slots", "range start > end");
        }
        if self.risk.kill_file.trim().is_empty() {
            inv(
                &mut r,
                "risk.kill_file",
                "must be set (a kill file stops buys)",
            );
        }
        if self.tx.nonce_pool == 0 {
            inv(&mut r, "tx.nonce_pool", "must be >= 1");
        }
        if self.tx.burn_after_slots == 0 {
            inv(&mut r, "tx.burn_after_slots", "must be >= 1");
        }
        if !(self.bid.quantile > 0.0 && self.bid.quantile <= 1.0) {
            inv(&mut r, "bid.quantile", "must be in (0, 1]");
        }
        for (n, s) in [
            ("bid.split.jito_agave", self.bid.split.jito_agave),
            ("bid.split.harmonic", self.bid.split.harmonic),
            ("bid.split.bam", self.bid.split.bam),
        ] {
            if s.iter().any(|x| !(0.0..=1.0).contains(x)) || ((s[0] + s[1]) - 1.0).abs() > 1e-6 {
                inv(&mut r, n, "fractions must be in [0,1] and sum to 1");
            }
        }
        if !(0.0..1.0).contains(&self.ab.skip_p) {
            inv(&mut r, "ab.skip_p", "must be in [0, 1)");
        }
        if !(0.0..1.0).contains(&self.entry.max_premium) {
            inv(&mut r, "entry.max_premium", "must be in [0, 1)");
        }
        if !(0.0..1.0).contains(&self.paper.entry_premium) {
            inv(&mut r, "paper.entry_premium", "must be in [0, 1)");
        }
        if !(0.0..1.0).contains(&self.paper.late_block_frac) {
            inv(&mut r, "paper.late_block_frac", "must be in [0, 1)");
        }
        if self.paper.slot_ms == 0 {
            inv(&mut r, "paper.slot_ms", "must be >= 1");
        }
        if self.paper.exit_latency_slots > 16 {
            inv(&mut r, "paper.exit_latency_slots", "must be <= 16");
        }
        if self.paper.replay_slots == 0 || self.paper.replay_slots > 150 {
            inv(&mut r, "paper.replay_slots", "must be in [1, 150]");
        }
        if self.paper.min_replay_slots > self.paper.replay_slots {
            inv(&mut r, "paper.min_replay_slots", "must be <= replay_slots");
        }
        if self.feeds.logsws.enabled && !self.feeds.logsws.url.starts_with("wss://") {
            inv(&mut r, "feeds.logsws.url", "must be wss://");
        }
        if self.risk.min_hot_balance_lamports > self.risk.max_hot_balance_lamports
            && self.risk.max_hot_balance_lamports != 0
        {
            inv(
                &mut r,
                "risk.min_hot_balance_lamports",
                "above max_hot_balance_lamports",
            );
        }
        if self.tx.cu_limit > 1_400_000 {
            inv(&mut r, "tx.cu_limit", "above 1,400,000");
        }
        if self.tx.lads_bytes > 64 * 1024 * 1024 {
            inv(&mut r, "tx.lads_bytes", "above 64 MiB");
        }
        let rb = &self.rpc_budget;
        for (field, v) in [
            ("rpc_budget.per_method", rb.per_method),
            ("rpc_budget.per_host", rb.per_host),
        ] {
            if !(v.is_finite() && v > 0.0) {
                inv(&mut r, field, "must be > 0 req/s");
            }
        }
        if rb.methods.values().any(|v| !(v.is_finite() && *v > 0.0)) {
            inv(
                &mut r,
                "rpc_budget.methods",
                "every per-method cap must be > 0 req/s",
            );
        }
        if rb
            .methods
            .keys()
            .any(|k| k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric()))
        {
            inv(
                &mut r,
                "rpc_budget.methods",
                "keys must be RPC method names (e.g. getBlock)",
            );
        }
        if rb.per_method > rb.per_host && rb.per_host > 0.0 {
            w.push(format!(
                "rpc_budget.per_method {} > per_host {}: the host cap binds first",
                rb.per_method, rb.per_host
            ));
        }
        let co = &self.collector;
        if co.workers == Some(0) {
            inv(&mut r, "collector.workers", "must be >= 1");
        }
        if co.api_rps.is_some_and(|v| !(v.is_finite() && v > 0.0)) {
            inv(&mut r, "collector.api_rps", "must be > 0");
        }
        for (field, v) in [
            ("collector.duration_s", co.duration_s),
            ("collector.pp_stale_s", co.pp_stale_s),
            ("collector.logs_stale_s", co.logs_stale_s),
            ("collector.decode_delay_s", co.decode_delay_s),
            ("collector.funding_delay_s", co.funding_delay_s),
            ("collector.rivals_delay_s", co.rivals_delay_s),
            ("collector.tape_delay_s", co.tape_delay_s),
            ("collector.stats_every_s", co.stats_every_s),
        ] {
            if v.is_some_and(|x| !(x.is_finite() && x >= 0.0)) {
                inv(&mut r, field, "must be a finite number of seconds >= 0");
            }
        }
        if let Some(role) = &co.budget_role {
            if !RpcBudgetCfg::ROLES.contains(&role.as_str()) {
                inv(
                    &mut r,
                    "collector.budget_role",
                    "must be one of hot, guard, keeper, collector, backfill",
                );
            }
        }
        if co.rpc_endpoints.as_ref().is_some_and(|v| {
            v.is_empty()
                || v.iter()
                    .any(|u| !u.starts_with("https://") && !u.starts_with("http://"))
        }) {
            inv(
                &mut r,
                "collector.rpc_endpoints",
                "must be a non-empty list of http(s) URLs",
            );
        }
        for (field, u) in [
            ("collector.pp_url", &co.pp_url),
            ("collector.logs_url", &co.logs_url),
        ] {
            if u.as_ref()
                .is_some_and(|u| !u.starts_with("wss://") && !u.starts_with("ws://"))
            {
                inv(&mut r, field, "must be a ws(s):// URL");
            }
        }
        if self.feeds.pumpportal.enabled {
            w.push(
                "feeds.pumpportal enabled: PumpPortal allows ONE connection per IP (the collector usually holds it)"
                    .to_string(),
            );
        }
        // ---- paper: nothing else is required
        if !mode.is_real() {
            let real = self.senders.real_enabled();
            if !real.is_empty() {
                w.push(format!(
                    "mode = paper: real senders {real:?} are configured but will not be constructed"
                ));
            }
            return Validation {
                mode,
                refusals: r,
                warnings: w,
            };
        }
        // ---- probe / live
        for (name, v) in self.risk.caps() {
            if v == 0 {
                r.push(Refusal::CapMissing(name));
            }
        }
        for (name, v) in [
            ("max_clock_offset_ms", self.risk.max_clock_offset_ms),
            ("decode_miss_kill_ratio", self.risk.decode_miss_kill_ratio),
            ("send_error_kill_ratio", self.risk.send_error_kill_ratio),
        ] {
            if v <= 0.0 {
                r.push(Refusal::CapMissing(name));
            }
        }
        if self.risk.armset_max_age_s == 0 {
            r.push(Refusal::CapMissing("armset_max_age_s"));
        }
        if self.tx.cu_limit == 0 {
            r.push(Refusal::CuLimitZero);
        }
        if self.tx.lads_bytes == 0 {
            r.push(Refusal::LadsZero);
        }
        let mut creds: Vec<&str> = vec![];
        match mode {
            Mode::Probe => creds.push(&self.secrets.probe_keypair),
            Mode::Live => creds.push(&self.secrets.hot_keypair),
            Mode::Paper => {}
        }
        let http_enabled = [
            &self.senders.helius,
            &self.senders.nozomi,
            &self.senders.astralane,
            &self.senders.jito,
            &self.senders.nextblock,
        ]
        .iter()
        .any(|s| s.enabled && !s.key.is_empty());
        if http_enabled {
            creds.push(&self.secrets.api_keys);
        }
        for c in creds {
            if c.is_empty() {
                r.push(Refusal::CredentialMissing("<unnamed>".into()));
                continue;
            }
            match env.credential_modes.get(c) {
                None | Some(None) => r.push(Refusal::CredentialMissing(c.to_string())),
                Some(Some(m)) if m & 0o077 != 0 => r.push(Refusal::CredentialPermissive {
                    name: c.to_string(),
                    mode: *m & 0o7777,
                }),
                _ => {}
            }
        }
        match env.clock_offset_ms {
            None => r.push(Refusal::ClockUnknown),
            Some(ms) if ms.abs() > self.risk.max_clock_offset_ms => r.push(Refusal::ClockOffset {
                ms,
                max: self.risk.max_clock_offset_ms,
            }),
            _ => {}
        }
        if !env.post_exec_feed_healthy {
            r.push(Refusal::NoPostExecFeed);
        }
        // probe: a signed, fresh ArmSet with probe_allowed (not gate_passed);
        // live: gate_passed
        match (mode, env.armset) {
            (
                Mode::Probe,
                ArmSetGate::Verified {
                    probe_allowed: false,
                    ..
                },
            ) => r.push(Refusal::ProbeNotAllowed),
            (
                Mode::Live,
                ArmSetGate::Verified {
                    gate_passed: false, ..
                },
            ) => r.push(Refusal::GateNotPassed),
            (_, ArmSetGate::Verified { .. }) => {}
            (_, other) => r.push(Refusal::ArmSet(other)),
        }
        if self.rpc_budget.sum() > RPC_BUDGET_MAX {
            r.push(Refusal::RpcBudget {
                sum: self.rpc_budget.sum(),
            });
        }
        if self
            .secrets
            .armset_pubkey
            .parse::<pump_codec::Pubkey>()
            .is_err()
        {
            r.push(Refusal::BadPubkey {
                field: "secrets.armset_pubkey",
            });
        }
        if self
            .secrets
            .cold_wallet
            .parse::<pump_codec::Pubkey>()
            .is_err()
        {
            r.push(Refusal::BadPubkey {
                field: "secrets.cold_wallet",
            });
        }
        if mode == Mode::Live {
            if !env.live_cli_flag {
                r.push(Refusal::LiveFlagMissing);
            }
            if !env.arm_live_token {
                r.push(Refusal::ArmLiveTokenMissing);
            }
        }
        Validation {
            mode,
            refusals: r,
            warnings: w,
        }
    }
}
