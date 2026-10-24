//! Process assembly shared by `rugsnipe-hot` and `rugsnipe-replay`: the
//! safety gate, the wallet, ArmSet, risk gate, nonce pool, templates,
//! state.db + recovery, rslog streams, the rings, the sender and cold
//! threads, and the hot loop itself (returned to the caller, which runs it
//! on its own thread).

use crate::cold::{self, ArmReload, ColdCfg, ColdParts, ColdReport, E2Cfg, RealCold};
use crate::hot::{HotCfg, HotLoop, HotParts, Shared};
use crate::msgs::{ColdMsg, FireJob, HotIn};
use crate::setup::{self, ArmState};
use crate::stats::HotStats;
use arc_swap::ArcSwap;
use prometheus_client::registry::Registry;
use pump_codec::Pubkey;
use rugsnipe_core::armset::ArmSet;
use rugsnipe_core::config::{ArmSetGate, Config, ValidateEnv};
use rugsnipe_core::kill::KillFile;
use rugsnipe_core::risk::RiskGate;
use rugsnipe_core::rslog::{box_id, RslogWriter};
use rugsnipe_core::watchlist::Watchlist;
use rugsnipe_core::Mode;
use rugsnipe_feed::{FeedHealth, FiredSet};
use rugsnipe_state::chain::ChainRead;
use rugsnipe_state::recovery::{recover, RecoverCtx, RecoveryReport};
use rugsnipe_state::rows::NonceRowState;
use rugsnipe_state::{Ledger, RpcBudget, StateStore};
use rugsnipe_tx::nonce::NoncePool;
use rugsnipe_tx::senders::{ApiKeys, SenderSet};
use rugsnipe_tx::signer::HotSigner;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::*};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// Where the ArmSet comes from.
#[derive(Debug, Clone)]
pub enum ArmChoice {
    /// A signed file (`armset-<ver>-<sha>.json` + `.sig`), verified at start
    /// and reloaded on change.
    File(PathBuf),
    /// An in-memory set (replay tests; paper only).
    Memory(Box<ArmSet>),
    /// No ArmSet: exits only.
    None,
}

/// Inputs of [`build`].
pub struct AppOpts {
    pub cfg: Config,
    pub armset: ArmChoice,
    pub regime: Option<String>,
    /// Paper only: load an unsigned/invalid ArmSet anyway (test runs).
    pub paper_unsigned: bool,
    pub state_path: Option<PathBuf>,
    pub rslog_dir: PathBuf,
    pub logsws: Option<rugsnipe_feed::ws::LogsWsHandle>,
    pub healths: Vec<Arc<FeedHealth>>,
    /// The paper fill lane (RPC endpoint and budget). With
    /// `paper.fill_model = "e1"` it runs the E1 lane, else the E2 lane.
    pub e2: Option<E2Cfg>,
    pub metrics: bool,
    pub fill_timeout_slots: u64,
    pub lease: bool,
    pub sigusr1: Arc<AtomicBool>,
    /// Paper wallet balance fed to the risk gate (`would_skip` realism).
    pub paper_balance: u64,
    /// `--live` was given on the command line.
    pub live_flag: bool,
    /// Probe only: where the keys are, what the box measured, the chain.
    pub real: Option<RealOpts>,
    /// The post-execution (gRPC) feed for the cold thread: held-mint
    /// watches, health (stale / drift triggers), bundle runs.
    pub post_exec: Option<crate::cold::PostExecFeed>,
    /// Run the leader schedule lane against this RPC (probe; paper only
    /// with `--leader-schedule`).
    pub leader_rpc: Option<String>,
    /// Telegram alerts (credentials found).
    pub alerts: Option<crate::cold::AlertCreds>,
    /// Tests: adjust the trigger settings derived from the config (e.g. the
    /// chain-check periods). `None` in the binaries.
    pub trigger_tuning: Option<fn(&mut crate::cold::TriggerCfg)>,
}

/// What a probe process needs beyond the config (docs/06 section 6): the
/// measured environment, the credentials directory and the chain source.
pub struct RealOpts {
    /// `$CREDENTIALS_DIRECTORY` (`rugsnipe-hot` reads it from the
    /// environment only: keys come from systemd credentials).
    pub creds_dir: PathBuf,
    /// Measured clock offset (`chronyc tracking`); `None` = unknown (refused).
    pub clock_offset_ms: Option<f64>,
    /// A post-execution feed (gRPC / shreds) is enabled and healthy.
    pub post_exec_feed_healthy: bool,
    /// `--probe-dry`: sign everything with the real key, send nothing
    /// (the post-execution feed requirement is waived: nothing can land).
    pub dry: bool,
    /// JSON-RPC of the real fill lane, nonce seeding and recovery.
    pub rpc_url: String,
    /// Chain reads for recovery and nonce seeding (`None`: public JSON-RPC
    /// on `rpc_url`, feature `rpc`).
    pub chain: Option<Box<dyn ChainRead + Send + Sync>>,
}

impl core::fmt::Debug for RealOpts {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RealOpts")
            .field("creds_dir", &self.creds_dir)
            .field("clock_offset_ms", &self.clock_offset_ms)
            .field("post_exec_feed_healthy", &self.post_exec_feed_healthy)
            .field("dry", &self.dry)
            .finish_non_exhaustive()
    }
}

/// The sender's budget for fetching a blockhash for a real sell that has
/// none (no fresh lane blockhash, no free sell nonce).
pub const SELL_BLOCKHASH_BUDGET_MS: u64 = 300;

/// A running process minus the hot loop.
pub struct App {
    pub hot: HotLoop,
    pub shared: Arc<Shared>,
    pub stats: Arc<HotStats>,
    pub registry: Arc<Registry>,
    pub report: Arc<Mutex<ColdReport>>,
    pub watch: Arc<Watchlist>,
    pub arm: Arc<ArcSwap<ArmState>>,
    pub risk: Arc<RiskGate>,
    pub senders: Arc<SenderSet>,
    pub owner: Pubkey,
    pub armset_gate: ArmSetGate,
    pub armset_error: Option<String>,
    pub recovery: Option<RecoveryReport>,
    /// The wallet (probe: the real probe key; paper: the throwaway one).
    pub signer: Arc<HotSigner>,
    pub nonces: Arc<NoncePool>,
    /// A copy of the compiled templates (the dry run's self-test).
    pub templates: setup::Templates,
    /// The leader schedule (empty until the lane publishes).
    pub leaders: crate::leader::LeaderSnapshot,
    store: Option<StateStore>,
    cold: Option<JoinHandle<()>>,
    sender: Option<JoinHandle<u64>>,
    writers: Vec<(String, RslogWriter)>,
    state_path: Option<PathBuf>,
    /// The chain source of recovery (probe), kept for the shutdown: a sell
    /// sent before the stop is resolved on chain and booked ([`crate::inflight`]).
    chain: Option<Arc<dyn ChainRead + Send + Sync>>,
}

impl core::fmt::Debug for App {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("App")
            .field("owner", &self.owner.to_string())
            .field("armset_gate", &self.armset_gate)
            .finish_non_exhaustive()
    }
}

/// End-of-run numbers.
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub mode: String,
    pub owner: String,
    pub armset_gate: String,
    pub counters: crate::hot::LoopCounters,
    pub decisions: Vec<(String, u64)>,
    pub cold: ColdReport,
    pub paper_variants_recorded: u64,
    pub sender_thread_sends: u64,
    pub rslog_records: Vec<(String, u64)>,
    pub rslog_dropped: u64,
    pub state_rows: Vec<(String, i64)>,
    pub state_errors: u64,
    pub open_positions_at_exit: usize,
    pub decision_latency_ns: Quantiles,
    pub wire_latency_ns: Quantiles,
    pub event_latency_ns: Quantiles,
    pub feeds: Vec<serde_json::Value>,
}

/// Latency quantiles (ns).
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Quantiles {
    pub n: u64,
    pub p50: u64,
    pub p90: u64,
    pub p99: u64,
    pub max: u64,
}

impl Quantiles {
    pub fn of(h: &hdrhistogram::Histogram<u64>) -> Self {
        if h.is_empty() {
            return Self::default();
        }
        Self {
            n: h.len(),
            p50: h.value_at_quantile(0.5),
            p90: h.value_at_quantile(0.9),
            p99: h.value_at_quantile(0.99),
            max: h.max(),
        }
    }
}

/// Parse `chronyc -c tracking` (CSV): field 5 is the system clock's offset
/// from NTP time in seconds. Returns milliseconds.
pub fn parse_chrony_tracking(csv: &str) -> Option<f64> {
    let line = csv.lines().find(|l| l.split(',').count() >= 6)?;
    let secs: f64 = line.split(',').nth(4)?.trim().parse().ok()?;
    secs.is_finite().then_some(secs * 1_000.0)
}

/// The clock offset measured by chrony (`None` if `chronyc` is missing or
/// unsynchronised: probe is then refused as "clock offset unknown").
pub fn chrony_offset_ms() -> Option<f64> {
    let out = std::process::Command::new("chronyc")
        .args(["-c", "tracking"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_chrony_tracking(&String::from_utf8_lossy(&out.stdout))
}

/// The facts the safety gate checks for a real mode: the measured clock
/// and post-execution feed, the credentials' permission bits, the ArmSet.
/// Without [`RealOpts`] everything external is unknown (and refused).
pub fn real_env(
    cfg: &Config,
    gate: ArmSetGate,
    armset_probe_stage: Option<rugsnipe_core::config::ProbeStage>,
    live_flag: bool,
    real: Option<&RealOpts>,
) -> ValidateEnv {
    ValidateEnv {
        clock_offset_ms: real.and_then(|r| r.clock_offset_ms),
        post_exec_feed_healthy: real.is_some_and(|r| r.post_exec_feed_healthy || r.dry),
        armset: gate,
        credential_modes: real
            .map(|r| crate::keys::credential_modes(cfg, &r.creds_dir))
            .unwrap_or_default(),
        live_cli_flag: live_flag,
        arm_live_token: false,
        armset_probe_stage,
    }
}

/// The safety gate for real modes (docs/06 section 6): every refusal is
/// returned. Paper needs nothing. Live is refused in this build whatever the
/// gates say; probe needs every `Config::validate` check to pass on `env`.
pub fn real_mode_gate(cfg: &Config, env: &ValidateEnv) -> Result<(), String> {
    if !cfg.mode.mode.is_real() {
        return Ok(());
    }
    let v = cfg.validate(env);
    let mut lines: Vec<String> = v.refusals.iter().map(|r| format!("  - {r}")).collect();
    if cfg.mode.mode == Mode::Live {
        lines.push("  - live is refused in this build (probe only)".into());
    }
    if lines.is_empty() {
        return Ok(());
    }
    Err(lines.join("\n"))
}

/// Unclean starts within an hour that latch the kill (`crash_loop`).
pub const UNCLEAN_STARTS_KILL: usize = 3;

/// Seed the risk gate with what today already spent, from `state.db`
/// ([`rugsnipe_state::riskday::day_seed`]): today's realized P&L (failed
/// attempts' `fail_cost`, closed positions, the persisted `risk` row), the
/// real buys of the day and the hour (probes per day), and each armed
/// operator's buys today and consecutive-loss latch. Latches `daily_loss`
/// at once when today is already at the cap.
pub fn seed_risk_day(
    c: &rusqlite::Connection,
    risk: &RiskGate,
    ops: &std::collections::HashMap<u32, setup::OpInfo>,
    now_unix: u64,
) -> Result<rugsnipe_state::riskday::DaySeed, String> {
    let day_start_ms = now_unix / 86_400 * 86_400_000;
    let hour_start_ms = now_unix / 3_600 * 3_600_000;
    let seed = rugsnipe_state::riskday::day_seed(
        c,
        day_start_ms,
        hour_start_ms,
        &rugsnipe_state::rows::utc_day(now_unix),
    )
    .map_err(|e| format!("state.db risk day: {e}"))?;
    let per_op: Vec<(usize, u64, u32, u32)> = ops
        .values()
        .filter_map(|o| {
            let buys = seed
                .op_buys
                .iter()
                .find(|(id, _)| *id == o.op_id)
                .map(|x| x.1)
                .unwrap_or(0);
            let (consec, until) = seed
                .op_risk
                .iter()
                .find(|(id, ..)| *id == o.op_id)
                .map(|x| (x.1, (x.2 / 86_400) as u32))
                .unwrap_or((0, 0));
            (buys > 0 || consec > 0 || until > 0).then_some((o.slot, buys, consec, until))
        })
        .collect();
    risk.seed_day(
        now_unix,
        seed.realized(),
        seed.real_buys,
        seed.real_buys_hour,
        &per_op,
    );
    Ok(seed)
}

/// Assemble the process. Real modes are refused unless every gate passes.
pub fn build(o: AppOpts) -> Result<App, String> {
    let cfg = o.cfg;
    let mode = cfg.mode.mode;
    let now = crate::now_unix();
    // --- ArmSet
    let (armset, gate, sha, armset_error) = match &o.armset {
        ArmChoice::File(p) => {
            let r = setup::load_armset_file(p, &cfg, now, o.regime.as_deref());
            match (r.armset, mode) {
                (Some(a), _) => (Some(a), r.gate, r.sha, None),
                (None, Mode::Paper) if o.paper_unsigned => {
                    let bytes = std::fs::read(p).map_err(|e| e.to_string())?;
                    let a: ArmSet = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                    (Some(a), r.gate, String::new(), r.error)
                }
                // An --armset that does not verify is an operator error, not a
                // request for an exits-only run: starting silently cost a night
                // of shadow data on ohio1 (the .sig was not installed). Omit
                // --armset for exits only.
                (None, _) => {
                    return Err(format!(
                        "ArmSet {} failed verification ({}); refusing to start. Install the \
                         .json and its .sig together, re-sign if older than risk.armset_max_age_s, \
                         or omit --armset for an exits-only run",
                        p.display(),
                        r.error.unwrap_or_else(|| "unknown".into())
                    ));
                }
            }
        }
        ArmChoice::Memory(a) => (
            Some((**a).clone()),
            ArmSetGate::Unsigned,
            String::new(),
            None,
        ),
        ArmChoice::None => (
            None,
            ArmSetGate::Missing,
            String::new(),
            Some("no ArmSet".into()),
        ),
    };
    if mode.is_real() {
        if let ArmChoice::Memory(_) = o.armset {
            return Err("in-memory ArmSets are paper only".into());
        }
    }
    // --- safety gate (refuses live, and probe with every reason)
    let stage = armset.as_ref().and_then(|a| a.probe_stage);
    if mode == Mode::Paper && o.real.is_some() {
        return Err("real options given in paper mode (bug)".into());
    }
    let env = real_env(&cfg, gate, stage, o.live_flag, o.real.as_ref());
    let mut gate_err = real_mode_gate(&cfg, &env).err();
    if mode.is_real() && o.real.is_none() {
        let line = "  - no real options (credentials, clock, post-execution feed, chain)";
        gate_err = Some(match gate_err {
            Some(e) => format!("{e}\n{line}"),
            None => line.to_string(),
        });
    }
    if mode == Mode::Probe && gate_err.is_none() {
        let buyers = cfg.senders.real_enabled();
        if !buyers.iter().any(|n| *n != "rpcsend" && *n != "tpu") {
            gate_err =
                Some("  - probe needs at least one HTTP sender enabled under [senders]".into());
        }
    }
    if let Some(e) = gate_err {
        return Err(format!("mode {mode} refused:\n{e}"));
    }
    // --- wallet and senders (probe: the probe key from systemd credentials)
    let (signer, api): (Arc<HotSigner>, ApiKeys) = match (&o.real, mode) {
        (Some(r), Mode::Probe) => {
            let k = crate::keys::load_real_keys(&cfg, &r.creds_dir).map_err(|rs| {
                format!(
                    "mode {mode} refused:\n{}",
                    rs.iter()
                        .map(|r| format!("  - {r}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            })?;
            (Arc::new(k.signer), k.api)
        }
        (_, Mode::Paper) => (
            Arc::new(setup::throwaway_signer().map_err(|e| e.to_string())?),
            ApiKeys::default(),
        ),
        _ => {
            return Err(format!(
                "mode {mode} refused: no key loading for it in this build"
            ))
        }
    };
    let dry = o.real.as_ref().is_some_and(|r| r.dry);
    let senders = Arc::new(
        if dry {
            SenderSet::build_dry(&cfg, &env, &api)
        } else {
            SenderSet::build(&cfg, &env, &api)
        }
        .map_err(|e| format!("senders: {e:?}"))?,
    );
    drop(api);
    if mode == Mode::Paper && !senders.real().is_empty() {
        return Err("paper mode built a real sender (bug)".into());
    }
    let owner = signer.pubkey();
    // --- watchlist + ArmSet state
    let watch = Arc::new(Watchlist::new());
    let arm_state = match &armset {
        Some(a) => {
            watch.load_armset(a);
            ArmState::from_armset(a, matches!(gate, ArmSetGate::Verified { .. }), &sha)
        }
        None => ArmState::default(),
    };
    // an ArmSet size above the spend cap is clamped to it by the hot loop
    // (2026-10-23); say so at start
    for w in arm_state.size_cap_refusals(&cfg.effective_risk(), mode) {
        eprintln!("warning: {w}");
    }
    let arm = Arc::new(ArcSwap::from_pointee(arm_state));
    // --- risk
    // probe: the selected [probe] stage narrows the [risk] caps
    let risk = Arc::new(RiskGate::new(cfg.effective_risk()));
    // A kill file present at start latches before the hot loop exists: the
    // cold thread's first poll comes ~100 ms later, after the first frames.
    KillFile::new(&cfg.risk.kill_file).poll(&risk);
    if mode == Mode::Paper {
        risk.set_hot_balance(o.paper_balance);
    }
    // --- nonce pool (probe: the wallet's on-chain accounts, seeded from chain
    // during recovery; every slot is unknown until then), templates
    let nonces = Arc::new(if mode == Mode::Paper {
        setup::paper_nonce_pool(owner, cfg.tx.nonce_pool as usize)
    } else {
        let n = (cfg.tx.nonce_pool as usize).max(1);
        NoncePool::new(owner, &rugsnipe_tx::sysix::pool_addresses(&owner, "buy", n))
    });
    let templates = setup::templates(&cfg, owner, &nonces)?;
    // the sell nonce pool (probe/live; `rugsnipe-ctl nonce-init --pool sell`):
    // real sells ride it when no blockhash is fresh; unknown until seeded
    let sell_nonces = (mode.is_real() && cfg.tx.sell_nonce_pool > 0).then(|| {
        Arc::new(NoncePool::new(
            owner,
            &rugsnipe_tx::sysix::pool_addresses(&owner, "sell", cfg.tx.sell_nonce_pool as usize),
        ))
    });
    let sell_templates = match &sell_nonces {
        Some(p) => setup::sell_nonce_templates(&cfg, owner, p)?,
        None => vec![],
    };
    let policy = setup::exit_policy(&cfg)?;
    // --- entry filter (docs/04 section 2): loaded once, refused if broken
    let filter = if cfg.entry.filter_model.is_empty() {
        if cfg.entry.filter_mode == rugsnipe_core::config::FilterMode::Skip {
            return Err("entry.filter_mode = \"skip\" needs entry.filter_model".into());
        }
        None
    } else {
        Some(Arc::new(crate::filter::Model::load(
            &crate::filter::resolve_path(&cfg.entry.filter_model),
        )?))
    };
    // --- state.db + recovery (before READY)
    let mut outcomes = vec![];
    let mut restored_latch = None;
    let mut programdata_baseline = None;
    if mode.is_real() && o.state_path.is_none() {
        return Err(format!(
            "mode {mode} refused: a real mode needs --state (recovery, ledger)"
        ));
    }
    // probe: the chain source for recovery and nonce seeding
    #[allow(unused_mut)]
    let mut real_opts = o.real;
    let chain: Option<Arc<dyn ChainRead + Send + Sync>> = match real_opts.as_mut() {
        Some(r) => match r.chain.take() {
            Some(c) => Some(Arc::from(c)),
            None => {
                #[cfg(feature = "rpc")]
                {
                    let b = Arc::new(RpcBudget::new(cfg.rpc_budget.keeper.max(1) as f64));
                    Some(Arc::new(rugsnipe_state::chain::RpcChain::new(
                        &r.rpc_url, b,
                    )))
                }
                #[cfg(not(feature = "rpc"))]
                {
                    return Err(format!(
                        "mode {mode} refused: this binary has no chain reads (build with --features real-senders)"
                    ));
                }
            }
        },
        None => None,
    };
    let nonce_accounts: Vec<(Pubkey, &'static str, u16)> = if mode.is_real() {
        let sell = sell_nonces
            .iter()
            .flat_map(|p| (0..p.len()).map(move |i| (p.account(i), "sell", i as u16)));
        (0..nonces.len())
            .map(|i| (nonces.account(i), "buy", i as u16))
            .chain(sell)
            .collect()
    } else {
        vec![]
    };
    let (store, ledger, recovery) = match &o.state_path {
        Some(p) => {
            let s = StateStore::open(p).map_err(|e| format!("state.db: {e}"))?;
            let l = Ledger::new(s.writer());
            let c = s.reader().map_err(|e| e.to_string())?;
            let rep = recover(
                &c,
                &l,
                &RecoverCtx {
                    owner: mode.is_real().then_some(owner),
                    mode: mode.as_str(),
                    nonce_accounts: &nonce_accounts,
                    chain: chain.as_deref().map(|c| c as &dyn ChainRead),
                },
            )
            .map_err(|e| format!("recovery: {e}"))?;
            if mode.is_real() {
                seed_nonces(&nonces, &rep);
                if let Some(p) = &sell_nonces {
                    seed_pool(p, &rep, "sell");
                    eprintln!("sell nonce pool: {} of {} usable", p.free_count(), p.len());
                }
                match chain.as_deref().map(|c| c.balance(&owner)) {
                    Some(Ok(b)) => risk.set_hot_balance(b),
                    Some(Err(e)) => eprintln!("wallet balance: {e} (the real lane retries)"),
                    None => {}
                }
            }
            // the entry filter's outcome store, as the last run left it
            outcomes = rugsnipe_state::store::load_outcomes(&c)
                .map_err(|e| format!("state.db outcomes: {e}"))?;
            // a kill latched by an earlier run stays latched (docs/06 section 13)
            restored_latch = rugsnipe_state::killkv::latched(&c)
                .map_err(|e| format!("state.db kill latch: {e}"))?;
            if let Some(l) = &restored_latch {
                risk.kill(
                    rugsnipe_core::risk::KillReason::from_name(&l.reason)
                        .unwrap_or(rugsnipe_core::risk::KillReason::Persisted),
                );
            }
            if mode.is_real() {
                // the daily caps survive a restart (docs/08 section 9)
                let seed = seed_risk_day(&c, &risk, &arm.load().ops, now)?;
                eprintln!(
                    "risk day restored: realized {} lamports (failed attempts {}, closed {}), real buys today {} (this hour {})",
                    seed.realized(),
                    seed.failed_cost,
                    seed.closed_pnl,
                    seed.real_buys,
                    seed.real_buys_hour
                );
                // a crash loop latches the kill (docs/08 section 10)
                let unclean = rugsnipe_state::riskday::note_start(&c, now, 3_600)
                    .map_err(|e| format!("state.db run marker: {e}"))?;
                if unclean > 0 {
                    eprintln!(
                        "previous run did not shut down cleanly: {unclean} unclean start(s) in the last hour (the kill latches at {UNCLEAN_STARTS_KILL})"
                    );
                }
                if unclean >= UNCLEAN_STARTS_KILL {
                    risk.kill(rugsnipe_core::risk::KillReason::CrashLoop);
                    let row = rugsnipe_state::killkv::Latch {
                        reason: rugsnipe_core::risk::KillReason::CrashLoop.name().into(),
                        unix: now,
                        detail: format!("{unclean} starts without a clean shutdown within an hour"),
                    };
                    l.kv(
                        &format!("{}crash_loop", rugsnipe_state::killkv::TRIGGER_PREFIX),
                        row.to_json(),
                    );
                }
            }
            if restored_latch.is_none() {
                if let Some(k) = risk.kill_reason() {
                    // latched at start (daily loss, crash loop): persisted now,
                    // not on the cold thread's first tick
                    let row = rugsnipe_state::killkv::Latch {
                        reason: k.name().into(),
                        unix: now,
                        detail: format!("{} at start", k.name()),
                    };
                    l.kv(rugsnipe_state::killkv::LATCH_KEY, row.to_json());
                    l.flush();
                }
            }
            programdata_baseline = rugsnipe_state::store::kv_get(&c, "pump_programdata")
                .ok()
                .flatten()
                .and_then(|v| crate::triggers::ProgramData::from_kv(&v));
            l.kv("mode", mode.as_str());
            l.kv("box", cfg.box_.id.clone());
            l.kv(
                if mode == Mode::Paper {
                    "paper_wallet"
                } else {
                    "wallet"
                },
                owner.to_string(),
            );
            l.kv("armset_ver", arm.load().ver.to_string());
            l.kv("armset_sha", sha.clone());
            l.kv("started_unix", now.to_string());
            (Some(s), Some(l), Some(rep))
        }
        None => (None, None, None),
    };
    // --- rslog streams (one SPSC ring + writer thread per producing thread)
    let bid = box_id(&cfg.box_.id);
    let mut writers = vec![];
    let mut producer = |stream: &str| -> Result<rugsnipe_core::rslog::RslogProducer, String> {
        let (p, w) = RslogWriter::spawn(
            &o.rslog_dir,
            &format!("{}-{stream}", cfg.box_.id),
            bid,
            1 << 16,
        )
        .map_err(|e| format!("rslog {}: {e}", o.rslog_dir.display()))?;
        writers.push((stream.to_string(), w));
        Ok(p)
    };
    let hot_log = producer("hot")?;
    let send_log = producer("send")?;
    let cold_log = producer("cold")?;
    // --- rings
    let (cold_tx, cold_rx) = rtrb::RingBuffer::<ColdMsg>::new(8_192);
    let (fill_tx, fill_rx) = rtrb::RingBuffer::<HotIn>::new(1_024);
    let (fire_tx, fire_rx) = rtrb::RingBuffer::<FireJob>::new(1_024);
    let (send_cold_tx, send_cold_rx) = std::sync::mpsc::channel::<ColdMsg>();
    // --- metrics
    let mut registry = Registry::default();
    let stats = Arc::new(HotStats::new(&mut registry));
    stats.mode.set(mode.as_u8() as i64);
    let registry = Arc::new(registry);
    let shared = Arc::new(Shared::default());
    if let Some(p) = &sell_nonces {
        let _ = shared.sell_nonces.set(p.clone());
    }
    // the sender's fallback for a real sell with no fresh blockhash and no
    // free sell nonce: a bounded (300 ms) getLatestBlockhash through the
    // chain handle, on its own thread (never on the hot thread)
    let resolver = match (&chain, mode.is_real()) {
        (Some(c), true) => {
            let c = c.clone();
            let fetch: crate::sender::FetchBlockhash =
                Arc::new(move || match c.latest_blockhash() {
                    Ok(x) => Some(x),
                    Err(e) => {
                        eprintln!("sell blockhash fallback: {}", crate::keys::redact_text(&e));
                        None
                    }
                });
            Some(
                crate::sender::BlockhashResolver::spawn(
                    shared.clone(),
                    fetch,
                    std::time::Duration::from_millis(SELL_BLOCKHASH_BUDGET_MS),
                )
                .map_err(|e| e.to_string())?,
            )
        }
        _ => None,
    };
    let has_resolver = resolver.is_some();
    let report = Arc::new(Mutex::new(ColdReport::default()));
    // --- threads
    let sender = crate::sender::spawn_with(
        fire_rx,
        senders.clone(),
        signer.clone(),
        send_log,
        send_cold_tx,
        stats.clone(),
        Arc::new(AtomicBool::new(false)),
        resolver,
    )
    .map_err(|e| e.to_string())?;
    let fired = Arc::new(FiredSet::with_capacity(1 << 16));
    let (e1_lane, e2_lane) = match (o.e2, cfg.paper.fill_model) {
        (Some(l), rugsnipe_core::config::FillModel::E1) => (
            Some(crate::e1::E1Cfg {
                rpc_url: l.rpc_url,
                budget: l.budget,
                delay_ms: cfg.paper.delay_ms,
                max_tx: cfg.paper.max_tx as usize,
                replay_slots: cfg.paper.replay_slots,
                late_block_frac: cfg.paper.late_block_frac,
                entry_premium_bps: (cfg.paper.entry_premium * 10_000.0).round() as u32,
                max_premium_bps: (cfg.entry.max_premium * 10_000.0).round() as u32,
                filter: cfg.entry.filter,
                retry_ms: 5_000,
                min_replay_slots: cfg.paper.min_replay_slots,
            }),
            None,
        ),
        (l, _) => (None, l),
    };
    let fill_lane = e1_lane.is_some() || e2_lane.is_some();
    let leaders = crate::leader::empty_snapshot();
    let has_post_exec = o.post_exec.is_some();
    // one budget for the keeper's reads: real fill lane, chain checks, leaders
    let keeper_budget = Arc::new(RpcBudget::new(cfg.rpc_budget.keeper.max(1) as f64));
    let eff = cfg.effective_risk();
    let triggers = crate::cold::TriggerCfg {
        enforce: mode.is_real(),
        send_error_window: cfg.risk.send_error_window.max(1) as usize,
        send_error_ratio: cfg.risk.send_error_kill_ratio,
        unexplained_drop_lamports: cfg.risk.unexplained_drop_lamports,
        // a real intent's stake plus the most it can cost besides: the bid as a
        // tip (raised to the senders' 0.001 SOL floors), the failed-attempt cap,
        // the Token-2022 ATA rent and fees
        spend_pad_lamports: cfg.risk.max_bid_lamports.max(1_000_000)
            + cfg.bid.failed_attempt_lamports_max
            + 1_513_840
            + 20_000,
        max_disk_used_pct: cfg.risk.max_disk_used_pct,
        state_dir: o
            .state_path
            .as_ref()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .map(|d| {
                if d.as_os_str().is_empty() {
                    PathBuf::from(".")
                } else {
                    d
                }
            }),
        kill_clock_offset_ms: cfg.risk.kill_clock_offset_ms,
        clock_check: mode.is_real(),
        max_slot_drift: cfg.risk.max_slot_drift,
        daily_loss_cap: eff.max_daily_loss_lamports,
        feed_stale_ms: cfg.feeds.grpc.stale_ms,
        chain_checks: real_opts
            .as_ref()
            .map(|r| (r.rpc_url.clone(), keeper_budget.clone())),
        programdata_baseline,
        slot_check_ms: 10_000,
        upgrade_check_ms: 60_000,
    };
    let mut triggers = triggers;
    if let Some(f) = o.trigger_tuning {
        f(&mut triggers);
    }
    let leader_lane = o.leader_rpc.clone().map(|url| crate::leader::LeaderCfg {
        rpc_url: url,
        budget: if mode.is_real() {
            keeper_budget.clone()
        } else {
            Arc::new(RpcBudget::new(1.0))
        },
        nodes_every_s: 180,
        epoch_every_s: 60,
    });
    let cold_cfg = ColdCfg {
        box_id: cfg.box_.id.clone(),
        mode,
        owner,
        kill_file: PathBuf::from(&cfg.risk.kill_file),
        metrics_listen: if o.metrics {
            cfg.obs.metrics_listen.clone()
        } else {
            vec![]
        },
        lease: match (&o.state_path, o.lease) {
            (Some(p), true) => Some((
                p.clone(),
                format!("hot:{}:{}", cfg.box_.id, std::process::id()),
                cfg.guard.takeover_after_ms,
            )),
            _ => None,
        },
        e2: e2_lane,
        e1: e1_lane,
        round_trip_lamports: cfg.paper.round_trip_lamports,
        armset: match &o.armset {
            ArmChoice::File(p) => Some(ArmReload {
                path: p.clone(),
                cfg: cfg.clone(),
                regime: o.regime.clone(),
            }),
            _ => None,
        },
        armset_max_age_s: cfg.risk.armset_max_age_s,
        burn_after_slots: cfg.tx.burn_after_slots,
        decode_miss_kill_ratio: cfg.risk.decode_miss_kill_ratio,
        paper_start_balance: o.paper_balance,
        filter: filter.clone(),
        filter_mode: cfg.entry.filter_mode,
        real: real_opts.as_ref().map(|r| RealCold {
            lane: crate::real::LaneCfg {
                rpc_url: r.rpc_url.clone(),
                // the keeper's share of the RPC budget: statuses, landed
                // transactions, nonces, balances, blockhash (shared with the
                // chain checks and the leader lane)
                budget: keeper_budget.clone(),
                owner,
                dry: r.dry,
            },
            signer: signer.clone(),
        }),
        triggers,
        alerts: o.alerts.clone(),
        leader_lane,
        restored_latch,
    };
    let cold = cold::spawn(ColdParts {
        cfg: cold_cfg,
        from_hot: cold_rx,
        from_sender: send_cold_rx,
        to_hot: fill_tx,
        ledger,
        logsws: o.logsws,
        healths: o.healths,
        watch: watch.clone(),
        arm: arm.clone(),
        risk: risk.clone(),
        nonces: nonces.clone(),
        shared: shared.clone(),
        stats: stats.clone(),
        registry: registry.clone(),
        rslog: cold_log,
        sigusr1: o.sigusr1,
        report: report.clone(),
        outcomes,
        post_exec: o.post_exec,
        leaders: leaders.clone(),
    })
    .map_err(|e| e.to_string())?;
    let mut hot_cfg = HotCfg::from_config(&cfg, policy);
    hot_cfg.fill_timeout_slots = o.fill_timeout_slots;
    hot_cfg.fill_lane = fill_lane;
    hot_cfg.filter = filter;
    hot_cfg.post_exec_feed = has_post_exec && mode.is_real();
    hot_cfg.sell_resolver = has_resolver;
    let mut hot = HotLoop::new(HotParts {
        cfg: hot_cfg,
        watch: watch.clone(),
        arm: arm.clone(),
        risk: risk.clone(),
        fired,
        nonces: nonces.clone(),
        signer: signer.clone(),
        templates: templates.clone(),
        senders: senders.clone(),
        rslog: hot_log,
        cold: cold_tx,
        fills_in: fill_rx,
        fire: fire_tx,
        stats: stats.clone(),
        shared: shared.clone(),
        leaders: leaders.clone(),
    });
    // probe: every token balance recovery found gets an exit owner (I2/I6)
    if mode.is_real() {
        if let Some(rep) = &recovery {
            hot.set_sell_nonce_templates(sell_templates);
            for p in &rep.open_positions {
                // the store's cost / opened slot / bundle stay; the wallet
                // only reconciles the token amount
                // (a balance recovery just found has no row: nothing known)
                let fresh_orphan = rep.orphans.iter().any(|o| o.mint == p.mint);
                let known = (!fresh_orphan).then(|| crate::hot::Known {
                    cost: p.cost,
                    opened_slot: p.opened_slot,
                    v_fill: p.v_fill,
                    peak_v: p.peak_v,
                    bundle: serde_json::from_str(&p.bundle_json).unwrap_or_default(),
                    from_intent: p.origin == rugsnipe_state::rows::Origin::Intent,
                });
                if !hot.adopt_known(p.mint, p.tokens, p.intent_id as u64, known) {
                    eprintln!(
                        "recovery: no position slot for {} ({} tokens)",
                        p.mint, p.tokens
                    );
                } else {
                    risk.adopt_position();
                }
            }
        }
    }
    Ok(App {
        hot,
        shared,
        stats,
        registry,
        report,
        watch,
        arm,
        risk,
        senders,
        owner,
        armset_gate: gate,
        armset_error,
        recovery,
        signer,
        nonces,
        templates,
        leaders,
        state_path: o.state_path.clone(),
        store,
        cold: Some(cold),
        sender: Some(sender),
        writers,
        chain,
    })
}

/// Apply recovery's nonce rows to the pool (seeding from chain): a known
/// value makes the slot usable for creates after its `last_change_slot`; a
/// missing, uninitialized or foreign account is quarantined; a slot that was
/// in flight with an unchanged value is in flight again (its old variants
/// may still land), so the keeper burns it first.
pub fn seed_nonces(pool: &NoncePool, rep: &RecoveryReport) {
    seed_pool(pool, rep, "buy")
}

/// [`seed_nonces`] for the rows of pool `name` (`buy` / `sell`).
pub fn seed_pool(pool: &NoncePool, rep: &RecoveryReport, name: &str) {
    let slot = rep.slot.unwrap_or(0);
    for r in &rep.nonces {
        let i = r.idx as usize;
        if r.pool != name || i >= pool.len() || pool.account(i) != r.pubkey {
            continue;
        }
        match r.state {
            NonceRowState::Quarantined => pool.quarantine(i),
            st => {
                pool.on_value(i, r.value, r.last_change_slot.unwrap_or(slot));
                let changed = rep.nonce_changed.contains(&r.idx);
                if !changed && matches!(st, NonceRowState::InFlight | NonceRowState::Burning) {
                    pool.resume_inflight(i, r.intent_id.unwrap_or(0) as u64, slot);
                }
            }
        }
    }
}

impl App {
    /// `--probe-dry` self-test: the buy template of the first usable nonce,
    /// patched with placeholder mint keys and the nonce's real value,
    /// re-targeted and signed with the real key for every configured sender
    /// (`(sender label, variant)`), plus the AdvanceNonce-only burn of that
    /// nonce. Nothing is sent; simulate them (`sigVerify = true`) to check
    /// the key, the nonce account and its authority before funding.
    pub fn dry_selftest(
        &self,
    ) -> (
        Vec<(&'static str, rugsnipe_tx::senders::Variant)>,
        Option<Vec<u8>>,
    ) {
        let Some(i) = (0..self.nonces.len()).find(|&i| {
            let p = self.nonces.packed(i);
            p.state == rugsnipe_tx::nonce::NonceState::Free
                && p.last_change_slot != rugsnipe_tx::nonce::UNKNOWN_SLOT
        }) else {
            return (vec![], None);
        };
        let value = self.nonces.value(i);
        let mint = Pubkey::new_from_array([0x5e; 32]);
        let bc = pump_codec::pda::bonding_curve(&mint);
        let keys = rugsnipe_tx::template::MintKeys::curve(
            mint,
            bc,
            pump_codec::pda::associated_bonding_curve(
                &mint,
                &pump_codec::constants::TOKEN_2022_PROGRAM_ID,
            ),
            pump_codec::pda::ata_spl(&bc, &pump_codec::constants::WSOL_MINT),
            pump_codec::pda::ata_token_2022(&self.owner, &mint),
        );
        let mut t = self.templates.buys[i].clone();
        t.patch(&rugsnipe_tx::template::Patch {
            keys: &keys,
            lifetime: &value,
            amount: 1_000_000,
            limit: 1,
            priority_lamports: 10_000,
            tip_lamports: 1_000_000,
            cu_limit: None,
        });
        let job = crate::msgs::FireJob {
            tmpl: t,
            intent: 1,
            idx: 0,
            prio: 10_000,
            tip: 1_000_000,
            tip_account: self.templates.tip_account,
            bid: 1_000_000,
            real: true,
            leader: crate::leader::UNKNOWN,
            bundle_prio: 10_000,
        };
        let targets = self.senders.targets();
        let vs = crate::sender::plan_variants(std::slice::from_ref(&job), targets, &self.signer)
            .into_iter()
            .map(|(k, v)| (targets[k].label(), v))
            .collect();
        let burn = rugsnipe_tx::sysix::burn_tx(&self.signer, &self.nonces.account(i), &value)
            .ok()
            .map(|(b, _)| b);
        (vs, burn)
    }

    /// Stop the threads, book the sells already sent ([`crate::inflight`],
    /// at most [`crate::inflight::BookCfg`]'s budget), flush, close the
    /// ledger, and collect the summary.
    pub fn shutdown(self, feeds: Vec<serde_json::Value>) -> Summary {
        self.shared.stop.store(true, Release);
        // real positions with a sell out: it may land after the loop stopped
        // reading its fill (2026-10-23: booked by nobody, the guard closed it
        // at 0); their signatures are in `sends` once the cold thread is done
        let inflight = match self.chain {
            Some(_) => self.hot.inflight_sells(),
            None => vec![],
        };
        let (hd, hw, he) = self.hot.histograms();
        let (qd, qw, qe) = (Quantiles::of(hd), Quantiles::of(hw), Quantiles::of(he));
        let counters = self.hot.counters;
        let open_positions_at_exit = self.hot.positions();
        let rslog_dropped = self.hot.rslog_dropped();
        let decisions = self
            .stats
            .decision_counts()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        // sender: stop flag is its own; it ends once the ring is empty and we drop the hot loop
        let App {
            hot,
            stats: _,
            report,
            senders,
            owner,
            armset_gate,
            store,
            cold,
            sender,
            writers,
            state_path,
            chain,
            ..
        } = self;
        drop(hot); // drops the fire producer, the hot rslog producer and the cold ring producer
        let sender_thread_sends = sender
            .map(|h| {
                // the sender exits when its ring is abandoned and empty
                h.join().unwrap_or(0)
            })
            .unwrap_or(0);
        if let Some(c) = cold {
            let _ = c.join();
        }
        let mut rslog_records = vec![];
        for (name, w) in writers {
            rslog_records.push((name, w.join().unwrap_or(0)));
        }
        let mut state_rows = vec![];
        let mut state_errors = 0;
        if let Some(s) = store {
            if let (Some(chain), Some(p), false) = (chain, &state_path, inflight.is_empty()) {
                match rugsnipe_state::store::open_conn(p) {
                    Ok(c) => {
                        let l = Ledger::new(s.writer());
                        for b in crate::inflight::book_inflight_sells(
                            &c,
                            &l,
                            chain,
                            owner,
                            &inflight,
                            crate::inflight::BookCfg::default(),
                        ) {
                            eprintln!("{b}");
                        }
                    }
                    Err(e) => eprintln!("shutdown: state.db: {e} (in-flight sells not booked)"),
                }
            }
            if !s.flush() {
                eprintln!("state.db: the final flush was not confirmed (writer gone): rows may be missing");
            }
            state_errors = s.stats().errors.load(Relaxed);
            if let Some(p) = &state_path {
                if let Ok(c) = rugsnipe_state::store::open_conn(p) {
                    // a clean shutdown: the next start is not counted as unclean
                    let _ = rugsnipe_state::riskday::note_clean_shutdown(&c);
                    if let Ok(rows) = rugsnipe_state::schema::row_counts(&c) {
                        state_rows = rows.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
                    }
                }
            }
            s.close();
        }
        let cold = report.lock().unwrap_or_else(|e| e.into_inner()).clone();
        Summary {
            mode: senders.mode.to_string(),
            owner: owner.to_string(),
            armset_gate: format!("{armset_gate:?}"),
            counters,
            decisions,
            cold,
            paper_variants_recorded: senders.paper.total(),
            sender_thread_sends,
            rslog_records,
            rslog_dropped,
            state_rows,
            state_errors,
            open_positions_at_exit,
            decision_latency_ns: qd,
            wire_latency_ns: qw,
            event_latency_ns: qe,
            feeds,
        }
    }
}
