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
    /// `[sells]`: which senders a real sell goes to (default: all).
    pub sells: SellsCfg,
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
    /// The two probe stages and the probe kill rule (docs/04 section 7).
    pub probe: ProbeCfg,
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
    /// Spread `max_buys_per_hour` across the hour: the k-th buy of a UTC hour
    /// is allowed only once `(k-1) * 3600 / max_buys_per_hour` seconds of the
    /// hour have passed (unused windows carry over within the hour). Off =
    /// first-come, which in a busy market spends the hour in its first minute.
    pub spread_buys_per_hour: bool,
    pub max_bid_lamports: u64,
    pub max_sell_spend_lamports: u64,
    pub max_entry_vsol_lamports: u64,
    pub max_hot_balance_lamports: u64,
    pub min_hot_balance_lamports: u64,
    pub kill_file: String,
    pub armset_max_age_s: u64,
    pub decode_miss_kill_ratio: f64,
    pub send_error_kill_ratio: f64,
    /// Start refusal: the clock offset a real mode may start with.
    pub max_clock_offset_ms: f64,
    /// Kill triggers (docs/06 section 13; `rugsnipe::triggers`), probe/live:
    /// the send-error ratio is read over the last `send_error_window` sends;
    pub send_error_window: u64,
    /// the wallet may fall by at most our known spends plus this between two reads;
    pub unexplained_drop_lamports: u64,
    /// the state partition's used share (percent);
    pub max_disk_used_pct: f64,
    /// a running box's clock offset (`chronyc -c tracking`);
    pub kill_clock_offset_ms: f64,
    /// how far the gRPC feed's slot may trail `getSlot`.
    pub max_slot_drift: u64,
    /// Real modes: new buys pause (decision `skip_no_blockhash`) while no
    /// fresh blockhash has been available for longer than this AND the sell
    /// nonce pool has no free nonce, i.e. while a position could not be
    /// sold; transient, clears on the next fresh blockhash (never latched).
    pub max_blockhash_age_ms: u64,
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
            spread_buys_per_hour: false,
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
            send_error_window: 50,
            unexplained_drop_lamports: 50_000_000,
            max_disk_used_pct: 90.0,
            kill_clock_offset_ms: 50.0,
            max_slot_drift: 8,
            max_blockhash_age_ms: 5_000,
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

/// `grpc = { enabled, url, x_token, stale_ms, bundle_run }`: the Yellowstone
/// post-execution feed (docs/08 section 9). `x_token` is a **name** inside
/// the `api-keys` credential (or a credential file of that name), never the
/// token; empty = no `x-token` header (IP-allow-listed endpoints).
/// `endpoint` / `token` are accepted as the older spellings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct GrpcCfg {
    pub enabled: bool,
    #[serde(alias = "endpoint")]
    pub url: String,
    #[serde(alias = "token")]
    pub x_token: String,
    pub stale_ms: u64,
    /// Also stream every pump transaction to assemble the creation slot's
    /// bundle run (the entry filter's bundle features from the feed).
    pub bundle_run: bool,
}
impl Default for GrpcCfg {
    fn default() -> Self {
        Self {
            enabled: false,
            url: String::new(),
            x_token: "GRPC_TOKEN".into(),
            stale_ms: 2_000,
            bundle_run: true,
        }
    }
}

/// A shredstream proxy's entry stream (`feeds.entries`): the
/// `jito-shredstream-proxy` protocol (Jito's, or a gateway speaking it such
/// as AllenHark's Frankfurt feed) run on this box with `--grpc-service-port`,
/// its `ShredstreamProxy/SubscribeEntries` consumed as a second
/// pre-execution feed, source `shredstream`, beside `feeds.deshred`. The
/// first-seen report races the two (`lag."shreds_behind_shredstream"` /
/// `"shredstream_behind_shreds"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct EntriesCfg {
    pub enabled: bool,
    /// The proxy's gRPC service, plain h2c on this box.
    pub url: String,
    pub stale_ms: u64,
    /// Assemble the create slot's bundle run from this stream too (the
    /// hot loop's bundle gate fires on whichever feed completes it first).
    pub bundle_run: bool,
}
impl Default for EntriesCfg {
    fn default() -> Self {
        Self {
            enabled: false,
            url: "http://127.0.0.1:9999".into(),
            stale_ms: 4_000,
            bundle_run: true,
        }
    }
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
    /// A second Yellowstone endpoint run beside `grpc` for comparison (a
    /// provider on trial): the same adapter, source `grpc_b`, no bundle
    /// run, never the kill's post-execution feed. The hot loop fires on
    /// whichever copy of a create arrives first; the cold report's
    /// `first_seen` block (`first_by`, `lag."grpc_behind_grpc_b"` /
    /// `"grpc_b_behind_grpc"`) says which feed is ahead and by how much.
    pub grpc_b: GrpcCfg,
    /// The pre-execution feed (Yellowstone `SubscribeDeshred`: transactions
    /// decoded from shreds before the block is replayed; Triton's Dragon's
    /// Mouth). The same adapter in deshred mode, source `shreds`: creates
    /// reach the hot loop 50-80 ms before the post-execution copy, and the
    /// create slot's buys are quoted on our own curve so the bundle gate
    /// fires before the chain executes them. `feeds.grpc` stays on for
    /// fills, exits and the kill's post-execution health.
    pub deshred: GrpcCfg,
    /// A shredstream proxy's entry stream, the second pre-execution feed.
    pub entries: EntriesCfg,
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
            grpc_b: GrpcCfg {
                x_token: "GRPC_B_TOKEN".into(),
                stale_ms: 4_000,
                bundle_run: false,
                ..GrpcCfg::default()
            },
            deshred: GrpcCfg {
                x_token: "DESHRED_TOKEN".into(),
                stale_ms: 4_000,
                bundle_run: true,
                ..GrpcCfg::default()
            },
            entries: EntriesCfg::default(),
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
    /// Jito only: block-engine regions, one sender (and one 1 req/s token
    /// bucket) each, e.g. `["frankfurt", "amsterdam"]`; empty = `region`.
    pub regions: Vec<String>,
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
    /// Helius Sender with `&swqos_only=true` (5,000-lamport minimum tip), a
    /// second leg on the same durable nonce (shares `helius.key`'s name if
    /// its own `key` is empty).
    pub helius_swqos: HttpSenderCfg,
    pub nozomi: HttpSenderCfg,
    pub astralane: HttpSenderCfg,
    pub jito: HttpSenderCfg,
    pub nextblock: HttpSenderCfg,
    /// BlockRazor `sendTransaction`, mode `fast` (0.0001 SOL minimum tip, 3 TPS).
    pub blockrazor: HttpSenderCfg,
    pub tpu: TpuCfg,
    /// Minutes a sender whose own send-error ratio is above
    /// `risk.send_error_kill_ratio` stays disabled (0 = the default, 10).
    pub quarantine_min: u64,
}
impl SendersCfg {
    /// The per-sender quarantine in ms (`quarantine_min`, default 10 min).
    pub fn quarantine_ms(&self) -> u64 {
        let m = if self.quarantine_min == 0 {
            10
        } else {
            self.quarantine_min
        };
        m.saturating_mul(60_000)
    }

    /// Names of enabled senders that can move money (everything but paper).
    pub fn real_enabled(&self) -> Vec<&'static str> {
        let mut v = vec![];
        if self.rpcsend.enabled {
            v.push("rpcsend");
        }
        for (n, s) in self.http() {
            if s.enabled {
                v.push(n);
            }
        }
        if self.tpu.enabled {
            v.push("tpu");
        }
        v
    }

    /// Every HTTP sender table with its name, in fan-out order.
    pub fn http(&self) -> [(&'static str, &HttpSenderCfg); 7] {
        [
            ("helius", &self.helius),
            ("helius_swqos", &self.helius_swqos),
            ("nozomi", &self.nozomi),
            ("astralane", &self.astralane),
            ("blockrazor", &self.blockrazor),
            ("jito", &self.jito),
            ("nextblock", &self.nextblock),
        ]
    }
}

/// `[sells]`: the sell fan-out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct SellsCfg {
    /// Sender labels a real sell goes to (`helius`, `helius_swqos`,
    /// `nozomi`, `astralane`, `blockrazor`, `nextblock`, `rpcsend`,
    /// `jito_<region>`, or `jito` for every region); empty = every
    /// configured sender (the buy set).
    pub senders: Vec<String>,
}
impl SellsCfg {
    /// A sell may go to the sender labelled `label`.
    pub fn allows(&self, label: &str) -> bool {
        self.senders.is_empty()
            || self
                .senders
                .iter()
                .any(|n| n == label || (n == "jito" && label.starts_with("jito")))
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
    /// sells use `max(cu_limit, 88,000)` as their static limit. 65,000
    /// recommended (the formula at S = 0, so the floor never binds). 0 =
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

/// `[priority, tip]` fractions of the bid per leader class (docs/03
/// section 6 "Pay on success"). The tip is a transfer **after** the buy in
/// the same transaction: it reverts with a failed buy, while the priority
/// fee (and the base fee) is charged on failure too. Tip-ordered classes
/// therefore put the bid in the tip and keep the priority fee at
/// `min_priority_lamports`; priority-ordered classes bid with the priority
/// fee, clamped by `failed_attempt_lamports_max`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct BidSplit {
    /// Jito-Agave (`JitoLabs`): tips count through the block engine.
    pub jito_agave: [f64; 2],
    /// Harmonic: priority-fee ordered batches (tip accounts buy nothing).
    pub harmonic: [f64; 2],
    /// BAM: the BAM node's auction. Priority-fee ordered by default: inside
    /// a BAM round the earlier transaction had the higher compute-unit price
    /// 86-89% of the time (docs/03 section 6), until the probe measures
    /// which variant lands.
    pub bam: [f64; 2],
    /// Firedancer / Frankendancer / plain Agave / unknown: priority fee.
    pub other: [f64; 2],
}
impl Default for BidSplit {
    fn default() -> Self {
        Self {
            jito_agave: [0.0, 1.0],
            harmonic: [1.0, 0.0],
            bam: [1.0, 0.0],
            other: [1.0, 0.0],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct BidCfg {
    pub quantile: f64,
    /// The buy bid in lamports, split per leader class by `split` (0 = the
    /// measured median first-winner bid of docs/03 section 6, 3,400,000).
    /// Bounded by `risk.max_bid_lamports`. 2026-10-23: the snipers ahead of
    /// us on contested launches paid a median 2.1M priority fee plus an
    /// 8.4M tip, about three times this default.
    pub lamports: u64,
    /// `[priority, tip]` fractions per leader class.
    pub split: BidSplit,
    /// Header priority fee of every variant (the scheduling floor): what a
    /// pay-on-success variant still pays when it lands and fails. 10,000
    /// lamports sits between the p10 (1,000) and p25 (45,000) priority fee
    /// of slot-0 winners (gap-2 section 4).
    pub min_priority_lamports: u64,
    /// Most a failed attempt may cost (base fee + priority fee of the variant
    /// that lands; tips revert). The hot path clamps the priority fee to
    /// `cap - base fee`. Validated: at least `2 x 5,000 + min_priority_lamports`.
    pub failed_attempt_lamports_max: u64,
    /// Per-leader-class override of `failed_attempt_lamports_max` (0 = the
    /// global cap, the default). The probe-A arm design (docs/06 section 6)
    /// can raise the cap of a priority-ordered class (rivals' median ~25.8M
    /// µL/CU against our ~990k-lamport clamp) without touching sizing.
    pub failed_attempt_max_by_class: ClassCaps,
}

/// `bid.failed_attempt_max_by_class`: lamports per leader class, 0 = the
/// global `bid.failed_attempt_lamports_max`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ClassCaps {
    pub jito_agave: u64,
    pub bam: u64,
    pub harmonic: u64,
    pub other: u64,
}
impl Default for BidCfg {
    fn default() -> Self {
        Self {
            quantile: 0.75,
            lamports: 0,
            split: BidSplit::default(),
            min_priority_lamports: 10_000,
            failed_attempt_lamports_max: 1_000_000,
            failed_attempt_max_by_class: ClassCaps::default(),
        }
    }
}

/// Shares of leader slots by class (`paper.leader_mix`): the weights of the
/// modelled failed-attempt cost while the leader of the create slot is not
/// known (docs/03 section 6: BAM 37.9%, Jito-Agave 27.2%, Harmonic 25.1%,
/// Firedancer 7.2% + other 2.6%).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LeaderMix {
    pub jito_agave: f64,
    pub bam: f64,
    pub harmonic: f64,
    pub other: f64,
}
impl Default for LeaderMix {
    fn default() -> Self {
        Self {
            jito_agave: 0.272,
            bam: 0.379,
            harmonic: 0.251,
            other: 0.098,
        }
    }
}
impl LeaderMix {
    pub fn share(&self, c: crate::bid::LeaderClass) -> f64 {
        use crate::bid::LeaderClass as L;
        match c {
            L::JitoAgave => self.jito_agave,
            L::Bam => self.bam,
            L::Harmonic => self.harmonic,
            L::Other => self.other,
        }
    }
    pub fn total(&self) -> f64 {
        self.jito_agave + self.bam + self.harmonic + self.other
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

/// `alert = { telegram, chat, deadman, hourly, commands }`: credential
/// **names** (in the `api-keys` credential or files of that name). Telegram
/// is optional: without both the token and the chat id nothing is sent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct AlertCfg {
    pub telegram: String,
    pub chat: String,
    pub deadman: String,
    /// An hourly summary message.
    pub hourly: bool,
    /// Poll the bot for `/kill` (latches the kill; unkill stays local).
    pub commands: bool,
}
impl Default for AlertCfg {
    fn default() -> Self {
        Self {
            telegram: String::new(),
            chat: "TG_CHAT".into(),
            deadman: String::new(),
            hourly: true,
            commands: false,
        }
    }
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
    /// The fallback expectation of the bundle's SOL outside the create
    /// transaction, used only when the bundle run was not observed
    /// (`require_bundle` off, or a feed without block order). The on-chain
    /// bound is computed at `V_cap = min(v_max, V_pb * sqrt(1 +
    /// max_premium))`; with `require_bundle` `V_pb` is the run's observed
    /// post-trade reserve, otherwise `V0 + bundle_lamports` (V0: the curve
    /// after the create's in-tx buys), and paper E1 applies the same bound
    /// (docs/06 7.1). 2026-10-22: with the fixed 4 SOL expectation every
    /// bundle above about 4.3 SOL failed on chain (43 of 61 slippage
    /// failures; the landed fills averaged a 0.79 SOL bundle).
    pub bundle_lamports: u64,
    /// With `require_bundle`: how long (ms) a parked create waits after its
    /// first bundle buy for the rest of the run (the same Jito bundle's
    /// other buys, adjacent in the block) before it fires anyway. The
    /// stream's run-complete signal (a pump transaction beyond create + 5)
    /// or a trade at create + 5 fires it earlier. 0 fires on the first
    /// bundle buy (the bound then misses the rest of the bundle).
    pub bundle_wait_ms: u64,
    /// The same wait in microseconds; when set (> 0) it replaces
    /// `bundle_wait_ms`. A Jito bundle's buys sit in one entry and reach
    /// the feed within microseconds of each other, so a wait of a few
    /// hundred microseconds sees the whole bundle; the 2 ms default was
    /// measured as dead time on every attempt (2026-10-23).
    pub bundle_wait_us: u64,
    /// Dev buys (SOL, the create transaction's own buy) a buy may fire on;
    /// empty = any. The amount is an operator template marker: in the dry
    /// run (2026-10-23, 2,645 launches, no sniper in the create slot, which
    /// is the population this box lands) the launches with a dev buy of
    /// 5.00, 2.96, 6.91 or 3.85 SOL averaged +3.7% under the LEAD policy,
    /// positive in both halves of the data; the rest averaged -0.95%.
    pub dev_buy_sol: Vec<f64>,
    /// Tolerance around each listed dev buy, SOL (default 0.005).
    pub dev_buy_tol_sol: f64,
    /// Regions of the create slot's leader a buy may fire at (`eu`, `na`,
    /// `asia`, `other`); empty = any. A leader not in the table counts as
    /// reachable. 2026-10-23: from Frankfurt the buy landed in the create slot
    /// 62% of the time with a European leader, 33% North America, 9% Asia.
    pub leader_regions: Vec<String>,
    /// The validator region table (`data/leader_regions.json`, installed to
    /// `/var/lib/rugsnipe/data/`); required when `leader_regions` is set.
    pub leader_regions_file: String,
    /// The operator-dump skip (2026-10-23): a create whose dev, creator or
    /// bundle wallets belong to an earlier launch whose insiders sold at
    /// least half of their tokens within 3 slots of the create. `log`
    /// records the hit and fires anyway, `skip` drops the create, `off`
    /// does neither. The set grows live from every bundled launch the feed
    /// shows and persists in `state.db` (`dumpers`).
    pub dumper_skip: DumperSkip,
    pub filter: EntryFilter,
    /// The frozen P(take-profit) model of the entry filter
    /// (`data/reports/strategy/entry_filter_v1.json`, docs/04 section 2);
    /// empty = no filter. Loaded once at startup.
    pub filter_model: String,
    /// `log` (default): score every intent and record score and tertile;
    /// `skip`: also drop the intents scoring below the bottom-tertile cut
    /// point `t1` (no send, no cost).
    pub filter_mode: FilterMode,
    /// Real modes only: send a buy only once another wallet's buy of the
    /// mint has shown in the create slot (the launch is bundled, the
    /// population the LEAD policy was scored on: docs/04 section 2). The
    /// create is parked after the class filters and fires on that buy, right
    /// behind the bundle; a create slot that moves on without one is skipped
    /// (`skip_no_bundle`, no risk or nonce taken). Needs
    /// `feeds.grpc.bundle_run` (the bundle's buys come from the pump
    /// transaction stream). 2026-10-22: the first 10 stage-B attempts all
    /// hit launches with no bundle wallet and all exited on no_activity.
    pub require_bundle: bool,
    /// The pre-send entry gate's model (an `entry_filter` export, the same
    /// format as `filter_model`); empty = no gate. Scored at the fire point
    /// on what is known before the send: the creation slot's trades of the
    /// mint seen so far (the bundle run), the create transaction's wire
    /// bytes, the signer's operator history, unknown funding, the time.
    pub presend_model: String,
    /// `off` (default): no gate; `log`: score and record every decision
    /// that reaches the send, never skip; `skip`: also skip (`skip_presend`,
    /// nothing sent) a create scoring below `presend_cut`.
    pub presend_mode: PresendMode,
    /// The cut `skip` applies: the model's `t1` (bottom tertile) or `t2`
    /// (default: bottom two tertiles).
    pub presend_cut: PresendCut,
    /// The operator table (docs/09, `data/operators/*.json` from
    /// `analysis/scripts/strategy/operators_from_rt.py`): a JSON object keyed
    /// by the create transaction's signer (base58), each value
    /// `{hold_slots, n, wins, pnl_sol, dev_sol, bundle_wallets}`. Loaded once
    /// at startup; a listed signer's launches exit at `create + hold_slots`
    /// (overriding `[exit.default] hold_to_create_slots`). `""` = no table.
    pub operators_file: String,
    /// Fire only on creates whose signer is in `operators_file`; any other
    /// create is skipped as `skip_not_operator` (nothing parked or sent).
    pub operators_only: bool,
    /// Distinct bundle wallets a parked create may fire with (`[3]`, `[2,
    /// 3, 4]`); empty = any. Counted at the fire point from the park: the
    /// run's buyers outside the creator set (signer, creator, fee payer,
    /// the create transaction's buyers), at most 5. Needs `require_bundle`
    /// (the count comes from the parked run); a create outside the list is
    /// skipped as `skip_bundle_shape`.
    pub bundle_wallets: Vec<u32>,
    /// The pick template (docs/09, 2026-10-23): skip a create whose
    /// post-bundle virtual SOL reserve exceeds this many SOL, as
    /// `skip_bundle_too_big` (0x57); 0 = off. The reserve is the one the buy
    /// is bound at: the parked run's last creator-set / bundle trade, or
    /// without a run (`require_bundle` off, paper) the curve after the
    /// create's own buy (V0). The curve starts at 30 virtual SOL, so a
    /// value must be above 30. The copied wallet bought at about 37-47
    /// (post-bundle mcap 36-65 SOL); the 2026-10-23 loss was bought at 58.
    pub max_post_bundle_sol: f64,
    /// Skip a create whose dev buy (the create transaction's own buys) plus
    /// the parked run's creator-set and bundle buys exceed this many SOL, as
    /// `skip_bundle_sol` (0x58); 0 = off. Without a run only the dev buy
    /// counts. The copied wallet's dev + bundle: about 7 SOL, p90 17.
    pub max_bundle_sol: f64,
    /// An alternative to `dev_buy_sol`: the create's own buy may also be a
    /// whole number of SOL (+- `dev_buy_tol_sol`) in `[dev_buy_round_min,
    /// dev_buy_round_max]`. With `dev_buy_sol` also set a dev buy passes on
    /// either; a create passing neither is `skip_dev_buy` (0x50).
    pub dev_buy_round: bool,
    pub dev_buy_round_min: f64,
    pub dev_buy_round_max: f64,
    /// The create's uri host (`https?://<host>/...`, as the wire decoder
    /// keeps it: lower case, at most 64 bytes) must end in one of these
    /// (case-insensitive, on a label boundary: `uxento.io` matches
    /// `meta.uxento.io`, not `xuxento.io`); empty = any. A create with no
    /// such host (`ipfs://`, no scheme, a host over 64 bytes) fails a
    /// non-empty list. Else `skip_uri_host` (0x59), nothing parked.
    pub uri_hosts: Vec<String>,
    /// Hosts that skip a create the same way (`skip_uri_host`), matched
    /// like `uri_hosts`; empty = none.
    pub uri_hosts_deny: Vec<String>,
}
impl Default for EntryCfg {
    fn default() -> Self {
        Self {
            max_premium: 0.02,
            bundle_lamports: 4_000_000_000,
            bundle_wait_ms: 2,
            bundle_wait_us: 0,
            dumper_skip: DumperSkip::Log,
            dev_buy_sol: vec![],
            dev_buy_tol_sol: 0.005,
            leader_regions: vec![],
            leader_regions_file: String::new(),
            filter: EntryFilter::Any,
            filter_model: String::new(),
            filter_mode: FilterMode::Log,
            require_bundle: false,
            presend_model: String::new(),
            presend_mode: PresendMode::Off,
            presend_cut: PresendCut::T2,
            operators_file: String::new(),
            operators_only: false,
            bundle_wallets: vec![],
            max_post_bundle_sol: 0.0,
            max_bundle_sol: 0.0,
            dev_buy_round: false,
            dev_buy_round_min: 1.0,
            dev_buy_round_max: 6.0,
            uri_hosts: vec![],
            uri_hosts_deny: vec![],
        }
    }
}

/// An `entry.uri_hosts` / `uri_hosts_deny` entry: a host name or a suffix
/// of one (letters, digits, `.`, `-`, `_`; a leading `.` is allowed), at
/// most 64 bytes (the decoder's host limit), no scheme, path or port.
pub fn uri_host_pattern_ok(s: &str) -> bool {
    let h = s.trim_start_matches('.');
    !h.is_empty()
        && h.len() <= 64
        && !h.ends_with('.')
        && h.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

/// `entry.presend_mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresendMode {
    #[default]
    Off,
    /// Score and record, never skip.
    Log,
    /// Skip a create scoring below the cut.
    Skip,
}

impl PresendMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Log => "log",
            Self::Skip => "skip",
        }
    }
}

/// `entry.presend_cut`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresendCut {
    /// The model's `t1`: skip the bottom tertile.
    T1,
    /// The model's `t2`: skip the bottom two tertiles.
    #[default]
    T2,
}

impl PresendCut {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::T1 => "t1",
            Self::T2 => "t2",
        }
    }
}

/// `entry.filter_mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FilterMode {
    /// Score and record, never drop.
    #[default]
    Log,
    /// Drop intents with `score < t1`.
    Skip,
}

/// `entry.dumper_skip`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DumperSkip {
    Off,
    /// Count and journal the hit, fire anyway.
    #[default]
    Log,
    /// Skip the create (`skip_dumper`).
    Skip,
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

/// Probe stage (docs/04 section 7, docs/08 section 9): `a` measures (0.02
/// SOL buys), `b` is the capped trading probe (0.25 SOL buys), entered only
/// if stage A's measurements clear the entry rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProbeStage {
    #[default]
    A,
    B,
}
impl ProbeStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::A => "a",
            Self::B => "b",
        }
    }
    /// The verified ArmSet's `probe_stage` admits this config stage: it
    /// names the same stage, or names none and this is stage `a`. Stage `b`
    /// needs a signed ArmSet that names `b` (moving to B takes both keys).
    pub fn admitted_by(self, armset: Option<ProbeStage>) -> bool {
        armset.map_or(self == Self::A, |a| a == self)
    }
}

/// The caps of one probe stage. In `mode = "probe"` each one narrows the
/// `[risk]` cap of the same name (the smaller non-zero value binds).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeProfile {
    pub max_probe_lamports: u64,
    /// Attempts (sends), not fills.
    pub max_probes_per_day: u64,
    pub max_open_positions: u64,
    pub max_daily_loss_lamports: u64,
}
impl ProbeProfile {
    /// Probe-A, measurement: 0.02 SOL, <= 50 attempts a day, 1 open
    /// position, 0.2 SOL daily loss cap.
    pub fn stage_a() -> Self {
        Self {
            max_probe_lamports: 20_000_000,
            max_probes_per_day: 50,
            max_open_positions: 1,
            max_daily_loss_lamports: 200_000_000,
        }
    }
    /// Probe-B: 0.25 SOL, <= 300 attempts a day, 2 open positions, 1 SOL
    /// daily loss cap.
    pub fn stage_b() -> Self {
        Self {
            max_probe_lamports: 250_000_000,
            max_probes_per_day: 300,
            max_open_positions: 2,
            max_daily_loss_lamports: 1_000_000_000,
        }
    }
    fn caps(&self) -> [(&'static str, u64); 4] {
        [
            ("max_probe_lamports", self.max_probe_lamports),
            ("max_probes_per_day", self.max_probes_per_day),
            ("max_open_positions", self.max_open_positions),
            ("max_daily_loss_lamports", self.max_daily_loss_lamports),
        ]
    }
}

/// The probe kill rule (docs/04 section 7), read by
/// `analysis/scripts/shadow_gate.py --probe` from the inputs the hot
/// binary's end-of-run summary and `state.db` expose. Kill when the EV per
/// attempt (failed attempts included) has a 90% upper bound below 0 after
/// `min_landed` landed trades or `min_attempts` attempts, whichever comes
/// first; or the median realised premium exceeds `max_median_premium`; or
/// the landing rate stays below the break-even for the measured
/// failed-attempt cost over `landing_attempts` attempts; plus the stage's
/// daily loss cap (enforced by the risk gate).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ProbeKillCfg {
    pub min_landed: u64,
    pub min_attempts: u64,
    pub max_median_premium: f64,
    pub landing_attempts: u64,
}
impl Default for ProbeKillCfg {
    fn default() -> Self {
        Self {
            min_landed: 100,
            min_attempts: 400,
            max_median_premium: 0.03,
            landing_attempts: 200,
        }
    }
}

/// `[probe]` (docs/04 section 7): two named stages and the kill rule.
/// `stage` selects the profile; a signed ArmSet that carries `probe_stage`
/// must name the same stage, and stage `b` needs one that names `b` (moving
/// to B takes both keys).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ProbeCfg {
    pub stage: ProbeStage,
    pub a: ProbeProfile,
    pub b: ProbeProfile,
    pub kill: ProbeKillCfg,
}
impl Default for ProbeCfg {
    fn default() -> Self {
        Self {
            stage: ProbeStage::A,
            a: ProbeProfile::stage_a(),
            b: ProbeProfile::stage_b(),
            kill: ProbeKillCfg::default(),
        }
    }
}
impl ProbeCfg {
    pub fn profile(&self, s: ProbeStage) -> &ProbeProfile {
        match s {
            ProbeStage::A => &self.a,
            ProbeStage::B => &self.b,
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
    /// Cost charged per failed attempt (lost race, slippage cap). **0 =
    /// modelled per leader class** from `[bid]` (base fee + the priority fee
    /// of the variant matching the leader, tips revert), weighted by
    /// `leader_mix`; a value > 0 is a fixed override (docs/04's 0.003 SOL
    /// sensitivity runs).
    pub failed_attempt_lamports: u64,
    /// Leader-class shares for the modelled failed-attempt cost.
    pub leader_mix: LeaderMix,
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
            failed_attempt_lamports: 0,
            leader_mix: LeaderMix::default(),
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
    /// `probe_stage` of the verified ArmSet, when it carries one.
    pub armset_probe_stage: Option<ProbeStage>,
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
    /// Probe/live with no post-execution feed configured (`feeds.grpc`).
    NoPostExecFeedConfigured,
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
                "tx.cu_limit = 0 (the per-mint CU floor; run `rugsnipe-ctl sim`, then set 65,000)"
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
            Self::NoPostExecFeedConfigured => write!(
                f,
                "no post-execution feed configured (feeds.probe_live_require = post_exec: enable feeds.grpc with a Yellowstone url)"
            ),
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

/// A real mode's `risk.kill_file` must be under `/run/rugsnipe/` (the
/// units' tmpfiles directory). `RUGSNIPE_KILL_DIR` (an absolute path) adds
/// one more directory for the test suite only; no unit sets it.
pub fn kill_dir_override() -> Option<String> {
    std::env::var("RUGSNIPE_KILL_DIR")
        .ok()
        .filter(|d| d.starts_with('/') && d.len() > 1)
        .map(|d| if d.ends_with('/') { d } else { format!("{d}/") })
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

    /// The risk caps the gate enforces in `self.mode.mode`: `[risk]`, and in
    /// probe mode each cap of the selected `[probe]` stage narrows the
    /// `[risk]` cap of the same name (the smaller non-zero value binds).
    pub fn effective_risk(&self) -> RiskCfg {
        let mut r = self.risk.clone();
        if self.mode.mode != Mode::Probe {
            return r;
        }
        let p = self.probe.profile(self.probe.stage);
        let narrow = |a: u64, b: u64| match (a, b) {
            (0, x) | (x, 0) => x,
            (x, y) => x.min(y),
        };
        r.max_probe_lamports = narrow(r.max_probe_lamports, p.max_probe_lamports);
        r.max_probes_per_day = narrow(r.max_probes_per_day, p.max_probes_per_day);
        r.max_open_positions = narrow(r.max_open_positions, p.max_open_positions);
        r.max_daily_loss_lamports = narrow(r.max_daily_loss_lamports, p.max_daily_loss_lamports);
        r
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
        if self
            .entry
            .dev_buy_sol
            .iter()
            .any(|v| !(*v > 0.0 && *v < 1_000.0))
        {
            inv(
                &mut r,
                "entry.dev_buy_sol",
                "every value must be in (0, 1000) SOL",
            );
        }
        if !(self.entry.dev_buy_tol_sol >= 0.0 && self.entry.dev_buy_tol_sol < 1.0) {
            inv(&mut r, "entry.dev_buy_tol_sol", "must be in [0, 1) SOL");
        }
        if self
            .entry
            .leader_regions
            .iter()
            .any(|x| !matches!(x.as_str(), "eu" | "na" | "asia" | "other"))
        {
            inv(
                &mut r,
                "entry.leader_regions",
                "values must be eu, na, asia or other",
            );
        }
        if self.entry.presend_mode != PresendMode::Off && self.entry.presend_model.trim().is_empty()
        {
            inv(
                &mut r,
                "entry.presend_model",
                "required when entry.presend_mode is log or skip (the gate needs a model)",
            );
        }
        if !self.entry.leader_regions.is_empty() && self.entry.leader_regions_file.is_empty() {
            inv(
                &mut r,
                "entry.leader_regions_file",
                "required when entry.leader_regions is set (data/leader_regions.json)",
            );
        }
        if self.bid.lamports > 0 {
            let max_bid = if self.risk.max_bid_lamports > 0 {
                self.risk.max_bid_lamports
            } else {
                25_000_000
            };
            if self.bid.lamports > max_bid {
                inv(
                    &mut r,
                    "bid.lamports",
                    &format!("must be <= risk.max_bid_lamports ({max_bid})"),
                );
            }
        }
        for (n, s) in [
            ("bid.split.jito_agave", self.bid.split.jito_agave),
            ("bid.split.harmonic", self.bid.split.harmonic),
            ("bid.split.bam", self.bid.split.bam),
            ("bid.split.other", self.bid.split.other),
        ] {
            if s.iter().any(|x| !(0.0..=1.0).contains(x)) || ((s[0] + s[1]) - 1.0).abs() > 1e-6 {
                inv(&mut r, n, "fractions must be in [0,1] and sum to 1");
            }
        }
        // the failed-attempt cap (docs/03 section 6 "Pay on success")
        let b = &self.bid;
        let floor = 2 * crate::bid::BASE_FEE_PER_SIGNATURE + b.min_priority_lamports;
        let cc = &b.failed_attempt_max_by_class;
        for (n, v) in [
            ("bid.failed_attempt_max_by_class.jito_agave", cc.jito_agave),
            ("bid.failed_attempt_max_by_class.bam", cc.bam),
            ("bid.failed_attempt_max_by_class.harmonic", cc.harmonic),
            ("bid.failed_attempt_max_by_class.other", cc.other),
        ] {
            if v > 0 && v < floor {
                inv(&mut r, n, &format!("must be 0 (global cap) or >= {floor}"));
            }
        }
        if b.failed_attempt_lamports_max < floor {
            inv(
                &mut r,
                "bid.failed_attempt_lamports_max",
                &format!(
                    "must be >= {floor} (2 signatures x 5,000 base fee + bid.min_priority_lamports)"
                ),
            );
        } else {
            let max_bid = if self.risk.max_bid_lamports > 0 {
                self.risk.max_bid_lamports
            } else {
                25_000_000
            };
            // priority-ordered classes are clamped to the cap by design (the
            // hot path cuts the fee); only a priority share on a tip class is
            // worth a warning
            let pol = crate::bid::BidPolicy::from_cfg(b);
            for c in crate::bid::LeaderClass::ALL {
                let p = pol.plan(c, max_bid, 1);
                if c.honours_tip() && p.priority_lamports > b.min_priority_lamports {
                    w.push(format!(
                        "bid.split.{}: {} lamports of a {max_bid}-lamport bid go to the priority fee, which is paid on failure; the class orders by tip",
                        c.as_str(),
                        p.priority_lamports
                    ));
                }
            }
        }
        let k = &self.probe.kill;
        if !(k.max_median_premium > 0.0 && k.max_median_premium < 1.0) {
            inv(&mut r, "probe.kill.max_median_premium", "must be in (0, 1)");
        }
        if k.min_landed == 0 || k.min_attempts == 0 || k.landing_attempts == 0 {
            inv(
                &mut r,
                "probe.kill",
                "min_landed, min_attempts and landing_attempts must be >= 1",
            );
        }
        let m = &self.paper.leader_mix;
        if [m.jito_agave, m.bam, m.harmonic, m.other]
            .iter()
            .any(|x| !(0.0..=1.0).contains(x))
            || (m.total() - 1.0).abs() > 1e-3
        {
            inv(
                &mut r,
                "paper.leader_mix",
                "shares must be in [0,1] and sum to 1",
            );
        }
        if !(0.0..1.0).contains(&self.ab.skip_p) {
            inv(&mut r, "ab.skip_p", "must be in [0, 1)");
        }
        // 0 is valid: min_tokens_out is then the exact post-bundle quote, so
        // any buy ahead of ours fails it on chain (docs/09)
        if !(0.0..1.0).contains(&self.entry.max_premium) {
            inv(&mut r, "entry.max_premium", "must be in [0, 1)");
        }
        if self.entry.operators_only && self.entry.operators_file.is_empty() {
            inv(
                &mut r,
                "entry.operators_only",
                "needs entry.operators_file (the operator table)",
            );
        }
        if !self.entry.bundle_wallets.is_empty() && !self.entry.require_bundle {
            inv(
                &mut r,
                "entry.bundle_wallets",
                "needs entry.require_bundle = true (the count comes from the parked bundle run)",
            );
        }
        let mp = self.entry.max_post_bundle_sol;
        if !(mp == 0.0 || (mp > 30.0 && mp < 1_000.0)) {
            inv(
                &mut r,
                "entry.max_post_bundle_sol",
                "must be 0 (off) or in (30, 1000) SOL: the post-bundle virtual SOL reserve (the curve starts at 30)",
            );
        }
        if !(self.entry.max_bundle_sol >= 0.0 && self.entry.max_bundle_sol < 1_000.0) {
            inv(
                &mut r,
                "entry.max_bundle_sol",
                "must be in [0, 1000) SOL (0 = off)",
            );
        }
        let (lo, hi) = (self.entry.dev_buy_round_min, self.entry.dev_buy_round_max);
        if !(lo >= 0.0 && lo <= hi && hi < 1_000.0) || lo.ceil() > hi.floor() {
            inv(
                &mut r,
                "entry.dev_buy_round_min",
                "dev_buy_round_min / dev_buy_round_max must satisfy 0 <= min <= max < 1000 SOL with a whole number between them",
            );
        }
        for (field, list) in [
            ("entry.uri_hosts", &self.entry.uri_hosts),
            ("entry.uri_hosts_deny", &self.entry.uri_hosts_deny),
        ] {
            if list.iter().any(|h| !uri_host_pattern_ok(h)) {
                inv(
                    &mut r,
                    field,
                    "each entry must be a host or host suffix (letters, digits, '.', '-', '_'; at most 64 bytes; no scheme, path or port)",
                );
            }
        }
        if self.entry.bundle_wait_us > 200_000 {
            inv(
                &mut r,
                "entry.bundle_wait_us",
                "must be at most 200000 (us; the create slot is 400 ms)",
            );
        }
        if self.entry.bundle_wait_ms > 200 {
            inv(
                &mut r,
                "entry.bundle_wait_ms",
                "must be at most 200 (ms; the create slot is 400 ms)",
            );
        }
        if self.entry.require_bundle && !self.feeds.grpc.bundle_run {
            inv(
                &mut r,
                "entry.require_bundle",
                "needs feeds.grpc.bundle_run = true (the bundle's buys come from the pump transaction stream)",
            );
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
        // `cred:NAME`: the URL is read from the credentials (docs/08 section 9)
        if self.feeds.logsws.enabled
            && !self.feeds.logsws.url.starts_with("wss://")
            && !self.feeds.logsws.url.starts_with("cred:")
        {
            inv(&mut r, "feeds.logsws.url", "must be wss://");
        }
        if self.feeds.probe_live_require != "post_exec" {
            inv(
                &mut r,
                "feeds.probe_live_require",
                "must be \"post_exec\" (probe and live fire only from a post-execution feed)",
            );
        }
        let g = &self.feeds.grpc;
        if g.enabled {
            if g.url.starts_with("cred:") {
                // resolved from the credentials at start
            } else if !(g.url.starts_with("https://") || g.url.starts_with("http://")) {
                inv(
                    &mut r,
                    "feeds.grpc.url",
                    "must be an https:// Yellowstone endpoint (http:// only for a local server)",
                );
            } else if g.url.starts_with("http://")
                && !(g.url.starts_with("http://127.") || g.url.starts_with("http://localhost"))
            {
                w.push(format!(
                    "feeds.grpc.url {} is plain http: the x-token travels unencrypted",
                    g.url
                ));
            }
            if !(200..=60_000).contains(&g.stale_ms) {
                inv(&mut r, "feeds.grpc.stale_ms", "must be in [200, 60000]");
            }
            if g.x_token.chars().any(|c| c.is_whitespace() || c == '=') {
                inv(
                    &mut r,
                    "feeds.grpc.x_token",
                    "is a credential NAME (e.g. GRPC_TOKEN), not the token",
                );
            }
        }
        let b = &self.feeds.grpc_b;
        if b.enabled {
            if !g.enabled {
                inv(
                    &mut r,
                    "feeds.grpc_b",
                    "is the comparison feed beside feeds.grpc: enable feeds.grpc too",
                );
            }
            if b.url.starts_with("cred:") {
                // resolved from the credentials at start
            } else if !(b.url.starts_with("https://") || b.url.starts_with("http://")) {
                inv(
                    &mut r,
                    "feeds.grpc_b.url",
                    "must be an https:// Yellowstone endpoint (http:// only for a local server)",
                );
            } else if b.url.starts_with("http://")
                && !(b.url.starts_with("http://127.") || b.url.starts_with("http://localhost"))
            {
                w.push(format!(
                    "feeds.grpc_b.url {} is plain http: the x-token travels unencrypted",
                    b.url
                ));
            }
            if b.url == g.url {
                inv(
                    &mut r,
                    "feeds.grpc_b.url",
                    "is the same endpoint as feeds.grpc",
                );
            }
            if !(200..=60_000).contains(&b.stale_ms) {
                inv(&mut r, "feeds.grpc_b.stale_ms", "must be in [200, 60000]");
            }
            if b.x_token.chars().any(|c| c.is_whitespace() || c == '=') {
                inv(
                    &mut r,
                    "feeds.grpc_b.x_token",
                    "is a credential NAME (e.g. GRPC_B_TOKEN), not the token",
                );
            }
            if b.bundle_run {
                w.push(
                    "feeds.grpc_b.bundle_run is ignored: the bundle run comes from feeds.grpc"
                        .into(),
                );
            }
        }
        let d = &self.feeds.deshred;
        if d.enabled {
            if !g.enabled {
                inv(
                    &mut r,
                    "feeds.deshred",
                    "is the pre-execution feed beside feeds.grpc: enable feeds.grpc too (fills, exits and the post-execution health come from it)",
                );
            }
            if d.url.starts_with("cred:") {
            } else if !(d.url.starts_with("https://") || d.url.starts_with("http://")) {
                inv(
                    &mut r,
                    "feeds.deshred.url",
                    "must be an https:// Yellowstone endpoint with SubscribeDeshred (http:// only for a local server)",
                );
            } else if d.url.starts_with("http://")
                && !(d.url.starts_with("http://127.") || d.url.starts_with("http://localhost"))
            {
                w.push(format!(
                    "feeds.deshred.url {} is plain http: the x-token travels unencrypted",
                    d.url
                ));
            }
            if !(200..=60_000).contains(&d.stale_ms) {
                inv(&mut r, "feeds.deshred.stale_ms", "must be in [200, 60000]");
            }
            if d.x_token.chars().any(|c| c.is_whitespace() || c == '=') {
                inv(
                    &mut r,
                    "feeds.deshred.x_token",
                    "is a credential NAME (e.g. DESHRED_TOKEN), not the token",
                );
            }
        }
        let e = &self.feeds.entries;
        if e.enabled {
            if !g.enabled {
                inv(
                    &mut r,
                    "feeds.entries",
                    "is a pre-execution feed beside feeds.grpc: enable feeds.grpc too (fills, exits and the post-execution health come from it)",
                );
            }
            if !(e.url.starts_with("http://") || e.url.starts_with("https://")) {
                inv(
                    &mut r,
                    "feeds.entries.url",
                    "must be the shredstream proxy's gRPC service (http://127.0.0.1:PORT, its --grpc-service-port)",
                );
            }
            if !(200..=60_000).contains(&e.stale_ms) {
                inv(&mut r, "feeds.entries.stale_ms", "must be in [200, 60000]");
            }
        }
        let rk = &self.risk;
        if rk.send_error_window < 10 {
            inv(&mut r, "risk.send_error_window", "must be >= 10 sends");
        }
        if !(rk.max_disk_used_pct > 0.0 && rk.max_disk_used_pct <= 100.0) {
            inv(&mut r, "risk.max_disk_used_pct", "must be in (0, 100]");
        }
        if !(rk.kill_clock_offset_ms.is_finite() && rk.kill_clock_offset_ms > 0.0) {
            inv(&mut r, "risk.kill_clock_offset_ms", "must be > 0");
        }
        if rk.max_slot_drift == 0 {
            inv(&mut r, "risk.max_slot_drift", "must be >= 1 slot");
        }
        if self.obs.alert.commands && self.obs.alert.telegram.is_empty() {
            inv(
                &mut r,
                "obs.alert.commands",
                "needs obs.alert.telegram (the bot token's credential name)",
            );
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
            let f = &self.feeds;
            if !(f.logsws.enabled || f.relay.enabled || f.grpc.enabled || f.shreds.enabled) {
                w.push(
                    "feeds.pumpportal is the only create feed: it carries no v1 transactions (about 45% of creates, docs/03 section 5); enable feeds.logsws"
                        .to_string(),
                );
            }
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
        // the kill file the units' tmpfiles directory holds: a relative path
        // resolves against whatever the working directory is
        let kf = self.risk.kill_file.trim();
        let under = |d: &str| kf.starts_with(d) && kf.len() > d.len();
        if !(under("/run/rugsnipe/") || kill_dir_override().is_some_and(|d| under(&d)))
            || kf.contains("/../")
            || kf.ends_with('/')
        {
            inv(
                &mut r,
                "risk.kill_file",
                &format!("{kf:?}: a real mode needs an absolute path under /run/rugsnipe/ (e.g. /run/rugsnipe/KILL)"),
            );
        }
        // a feed URL with a query string (an API key) comes from a credential
        for (field, on, u) in [
            (
                "feeds.logsws.url",
                self.feeds.logsws.enabled,
                &self.feeds.logsws.url,
            ),
            (
                "feeds.grpc.url",
                self.feeds.grpc.enabled,
                &self.feeds.grpc.url,
            ),
        ] {
            if on && !u.starts_with("cred:") && u.contains('?') {
                inv(
                    &mut r,
                    field,
                    "carries a query string (an API key?) in plain text: put the URL in the api-keys credential and write cred:NAME",
                );
            }
        }
        if mode == Mode::Probe {
            // the selected stage: every cap set, the ArmSet agrees, and a
            // [risk] cap tighter than the stage's is worth a warning
            let st = self.probe.stage;
            let prof = self.probe.profile(st);
            for (name, v) in prof.caps() {
                if v == 0 {
                    inv(
                        &mut r,
                        "probe",
                        &format!("probe.{}.{name} is 0", st.as_str()),
                    );
                }
            }
            if !st.admitted_by(env.armset_probe_stage) {
                let why = match env.armset_probe_stage {
                    Some(a) => format!(
                        "the ArmSet names probe stage {}, the config {}",
                        a.as_str(),
                        st.as_str()
                    ),
                    None => "probe stage b needs a signed ArmSet that names probe_stage = b".into(),
                };
                inv(&mut r, "probe.stage", &why);
            }
            let risk_caps = self.risk.caps();
            for (name, stage) in prof.caps() {
                let risk = risk_caps
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map_or(0, |(_, v)| *v);
                if risk > 0 && stage > 0 && risk < stage {
                    w.push(format!(
                        "risk.{name} = {risk} binds tighter than probe.{}.{name} = {stage}",
                        st.as_str()
                    ));
                }
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
        let http_enabled = self
            .senders
            .http()
            .iter()
            .any(|(_, s)| s.enabled && !s.key.is_empty());
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
        // the post-execution class: the gRPC feed (no shred feed is built
        // into this binary, so `feeds.shreds` alone does not count)
        if !self.feeds.grpc.enabled {
            r.push(Refusal::NoPostExecFeedConfigured);
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
