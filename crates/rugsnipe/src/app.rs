//! Process assembly shared by `rugsnipe-hot` and `rugsnipe-replay`: the
//! safety gate, the wallet, ArmSet, risk gate, nonce pool, templates,
//! state.db + recovery, rslog streams, the rings, the sender and cold
//! threads, and the hot loop itself (returned to the caller, which runs it
//! on its own thread).

use crate::cold::{self, ArmReload, ColdCfg, ColdParts, ColdReport, E2Cfg};
use crate::hot::{HotCfg, HotLoop, HotParts, Shared};
use crate::msgs::{ColdMsg, FireJob, HotIn};
use crate::setup::{self, ArmState};
use crate::stats::HotStats;
use arc_swap::ArcSwap;
use prometheus_client::registry::Registry;
use pump_codec::Pubkey;
use rugsnipe_core::armset::ArmSet;
use rugsnipe_core::config::{ArmSetGate, Config, ValidateEnv};
use rugsnipe_core::risk::RiskGate;
use rugsnipe_core::rslog::{box_id, RslogWriter};
use rugsnipe_core::watchlist::Watchlist;
use rugsnipe_core::Mode;
use rugsnipe_feed::{FeedHealth, FiredSet};
use rugsnipe_state::recovery::{recover, RecoverCtx, RecoveryReport};
use rugsnipe_state::{Ledger, StateStore};
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
    pub e2: Option<E2Cfg>,
    pub metrics: bool,
    pub fill_timeout_slots: u64,
    pub lease: bool,
    pub sigusr1: Arc<AtomicBool>,
    /// Paper wallet balance fed to the risk gate (`would_skip` realism).
    pub paper_balance: u64,
    /// `--live` was given on the command line.
    pub live_flag: bool,
}

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
    store: Option<StateStore>,
    cold: Option<JoinHandle<()>>,
    sender: Option<JoinHandle<u64>>,
    writers: Vec<(String, RslogWriter)>,
    state_path: Option<PathBuf>,
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

/// The safety gate for real modes (docs/06 section 6): every refusal is
/// returned. Paper needs nothing. This binary measures neither the clock
/// offset nor a post-execution feed, so probe/live are refused here.
pub fn real_mode_gate(cfg: &Config, gate: ArmSetGate, live_flag: bool) -> Result<(), String> {
    if !cfg.mode.mode.is_real() {
        return Ok(());
    }
    let env = ValidateEnv {
        clock_offset_ms: None,
        post_exec_feed_healthy: false,
        armset: gate,
        credential_modes: Default::default(),
        live_cli_flag: live_flag,
        arm_live_token: false,
    };
    let v = cfg.validate(&env);
    if v.ok() {
        return Ok(());
    }
    Err(v
        .refusals
        .iter()
        .map(|r| format!("  - {r}"))
        .collect::<Vec<_>>()
        .join("\n"))
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
                (None, _) => (None, r.gate, String::new(), r.error),
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
    // --- safety gate (refuses probe/live with every reason)
    real_mode_gate(&cfg, gate, o.live_flag).map_err(|r| format!("mode {mode} refused:\n{r}"))?;
    let env = ValidateEnv {
        armset: gate,
        ..Default::default()
    };
    let senders = Arc::new(
        SenderSet::build(&cfg, &env, &ApiKeys::default()).map_err(|e| format!("senders: {e:?}"))?,
    );
    if mode == Mode::Paper && !senders.real().is_empty() {
        return Err("paper mode built a real sender (bug)".into());
    }
    // --- wallet
    let signer: Arc<HotSigner> = match mode {
        Mode::Paper => Arc::new(setup::throwaway_signer().map_err(|e| e.to_string())?),
        _ => return Err("real-mode key loading is not part of this build".into()),
    };
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
    let arm = Arc::new(ArcSwap::from_pointee(arm_state));
    // --- risk
    let risk = Arc::new(RiskGate::new(cfg.risk.clone()));
    if mode == Mode::Paper {
        risk.set_hot_balance(o.paper_balance);
    }
    // --- nonce pool, templates
    let nonces = Arc::new(setup::paper_nonce_pool(owner, cfg.tx.nonce_pool as usize));
    let templates = setup::templates(&cfg, owner, &nonces)?;
    let policy = setup::exit_policy(&cfg)?;
    // --- state.db + recovery (before READY)
    let (store, ledger, recovery) = match &o.state_path {
        Some(p) => {
            let s = StateStore::open(p).map_err(|e| format!("state.db: {e}"))?;
            let l = Ledger::new(s.writer());
            let c = s.reader().map_err(|e| e.to_string())?;
            let rep = recover(
                &c,
                &l,
                &RecoverCtx {
                    owner: None,
                    mode: mode.as_str(),
                    nonce_accounts: &[],
                    chain: None,
                },
            )
            .map_err(|e| format!("recovery: {e}"))?;
            l.kv("mode", mode.as_str());
            l.kv("box", cfg.box_.id.clone());
            l.kv("paper_wallet", owner.to_string());
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
    let report = Arc::new(Mutex::new(ColdReport::default()));
    // --- threads
    let sender = crate::sender::spawn(
        fire_rx,
        senders.clone(),
        signer.clone(),
        send_log,
        send_cold_tx,
        stats.clone(),
        Arc::new(AtomicBool::new(false)),
    )
    .map_err(|e| e.to_string())?;
    let fired = Arc::new(FiredSet::with_capacity(1 << 16));
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
        e2: o.e2,
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
    })
    .map_err(|e| e.to_string())?;
    let mut hot_cfg = HotCfg::from_config(&cfg, policy);
    hot_cfg.fill_timeout_slots = o.fill_timeout_slots;
    let hot = HotLoop::new(HotParts {
        cfg: hot_cfg,
        watch: watch.clone(),
        arm: arm.clone(),
        risk: risk.clone(),
        fired,
        nonces,
        signer,
        templates,
        senders: senders.clone(),
        rslog: hot_log,
        cold: cold_tx,
        fills_in: fill_rx,
        fire: fire_tx,
        stats: stats.clone(),
        shared: shared.clone(),
    });
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
        state_path: o.state_path.clone(),
        store,
        cold: Some(cold),
        sender: Some(sender),
        writers,
    })
}

impl App {
    /// Stop the threads, flush, and collect the summary.
    pub fn shutdown(self, feeds: Vec<serde_json::Value>) -> Summary {
        self.shared.stop.store(true, Release);
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
            s.flush();
            state_errors = s.stats().errors.load(Relaxed);
            if let Some(p) = &state_path {
                if let Ok(c) = rugsnipe_state::store::open_conn(p) {
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
