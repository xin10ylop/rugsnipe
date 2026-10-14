//! `rugsnipe-hot`: the hot box process (docs/06 sections 7.1 and 12).
//!
//! Paper mode is the default and the only mode that runs without keys: the
//! wallet is a throwaway keypair generated at start, the only sender is the
//! paper recorder (no socket), and every decision is recorded (rslog +
//! state.db). Probe/live are refused unless every gate of docs/06 section 6
//! passes, which this build cannot satisfy (no clock or post-exec feed
//! measurement, no key loading, real senders not compiled in).
//!
//! ```text
//! rugsnipe-hot --config config/paper.example.toml --armset <armset-V-SHA.json> \
//!     --state data/raw/paper/state.db --rslog-dir data/raw/paper/rslog \
//!     [--duration-s 1200] [--record data/raw/feed] [--summary out.json]
//! ```

use rugsnipe::app::{self, AppOpts, ArmChoice};
use rugsnipe::cli::{flag, num, opt};
use rugsnipe::cold::E2Cfg;
use rugsnipe_core::Mode;
use rugsnipe_feed::record::RecordConfig;
use rugsnipe_feed::ws::{LogsWs, LogsWsConfig, PumpPortal, PumpPortalConfig};
use rugsnipe_feed::{Feed, FeedBuf, FeedMux, FeedStatus};
use rugsnipe_state::RpcBudget;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::*};
use std::sync::Arc;
use std::time::{Duration, Instant};

const USAGE: &str = "usage: rugsnipe-hot --config <toml> [--mode paper|probe|live] [--live]
    [--armset <armset-V-SHA.json>] [--paper-unsigned-armset] [--regime <name>]
    [--state <state.db>] [--rslog-dir <dir>] [--duration-s N] [--record <dir>]
    [--max-watches N] [--rpc-url <https url>] [--no-e2] [--lane-rps R] [--no-metrics]
    [--pin-core N|none] [--busy-poll] [--fill-timeout-slots N] [--summary <json>]

  --no-e2      no paper fill lane (every model falls back to E2 from buffered trades)
  --lane-rps   total requests/s of the E1 lane (default 3.5, or 2 when the ArmSet has
               funders, whose lookups share the 4 req/s public budget); the E2 lane
               uses rpc_budget.hot / 2 per method";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if flag(&args, "--help") || flag(&args, "-h") {
        println!("{USAGE}");
        return;
    }
    match run(&args) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("rugsnipe-hot: {e}");
            std::process::exit(2);
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let cfg_path = opt(args, "--config").ok_or(format!("--config required\n{USAGE}"))?;
    let mut cfg = rugsnipe::setup::load_config(&PathBuf::from(cfg_path))?;
    if let Some(m) = opt(args, "--mode") {
        cfg.mode.mode = m.parse::<Mode>()?;
    }
    let mode = cfg.mode.mode;
    let live_flag = flag(args, "--live");
    let v = cfg.validate(&Default::default());
    for w in &v.warnings {
        eprintln!("config warning: {w}");
    }
    if !v.ok() && !mode.is_real() {
        return Err(format!(
            "config refused:\n{}",
            v.refusals
                .iter()
                .map(|r| format!("  - {r}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    let armset = match opt(args, "--armset") {
        Some(p) => ArmChoice::File(PathBuf::from(p)),
        None => ArmChoice::None,
    };
    // Real modes are refused before any network connection is opened.
    if mode.is_real() {
        let gate = match &armset {
            ArmChoice::File(p) => {
                rugsnipe::setup::load_armset_file(
                    p,
                    &cfg,
                    rugsnipe::now_unix(),
                    opt(args, "--regime"),
                )
                .gate
            }
            _ => rugsnipe_core::config::ArmSetGate::Missing,
        };
        app::real_mode_gate(&cfg, gate, live_flag)
            .map_err(|r| format!("mode {mode} refused:\n{r}"))?;
        return Err(format!(
            "mode {mode}: this build has no real key loading or real senders; refusing"
        ));
    }

    // --- signals: SIGUSR1 = kill latch (buys stop, exits continue); INT/TERM = stop
    let usr1 = Arc::new(AtomicBool::new(false));
    let term = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGUSR1, usr1.clone())
        .map_err(|e| e.to_string())?;
    for s in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        signal_hook::flag::register(s, term.clone()).map_err(|e| e.to_string())?;
    }

    // --- feeds (each adapter on its own thread)
    let rpc_url = opt(args, "--rpc-url")
        .unwrap_or("https://api.mainnet-beta.solana.com")
        .to_string();
    let funders: Vec<pump_codec::Pubkey> = match &armset {
        ArmChoice::File(p) => std::fs::read(p)
            .ok()
            .and_then(|b| serde_json::from_slice::<rugsnipe_core::armset::ArmSet>(&b).ok())
            .map(|a| {
                let mut f: Vec<_> = a.funding_rules.iter().map(|r| r.funder).collect();
                f.sort();
                f.dedup();
                f
            })
            .unwrap_or_default(),
        _ => vec![],
    };
    let record_dir = opt(args, "--record").map(PathBuf::from);
    let mut feeds: Vec<Box<dyn Feed + Send>> = vec![];
    let mut healths = vec![];
    let mut logsws_handle = None;
    if cfg.feeds.logsws.enabled {
        let lc = LogsWsConfig {
            url: cfg.feeds.logsws.url.clone(),
            stale_ms: cfg.feeds.logsws.stale_ms,
            max_watches: num(args, "--max-watches", 16usize)?,
            watch_funders: funders.clone(),
            rpc_url: Some(rpc_url.clone()),
            rpc_rps: 2.0,
            record: record_dir.as_ref().map(|d| {
                let mut r = RecordConfig::new(d, "logsws");
                r.max_file_bytes = 256 << 20;
                r
            }),
            ..Default::default()
        };
        let ws = LogsWs::spawn(lc).map_err(|e| format!("logsws: {e}"))?;
        logsws_handle = Some(ws.handle());
        healths.push(ws.health_arc().clone());
        feeds.push(Box::new(ws));
    }
    if cfg.feeds.pumpportal.enabled {
        eprintln!(
            "feeds.pumpportal enabled: ONE connection per IP (the collector usually holds it)"
        );
        let pp = PumpPortal::spawn(PumpPortalConfig {
            url: cfg.feeds.pumpportal.url.clone(),
            record: record_dir
                .as_ref()
                .map(|d| RecordConfig::new(d, "pumpportal")),
            ..Default::default()
        })
        .map_err(|e| format!("pumpportal: {e}"))?;
        feeds.push(Box::new(pp));
    }
    if cfg.feeds.relay.enabled {
        let r = rugsnipe_feed::relay::CollectorRelay::spawn(
            rugsnipe_feed::relay::RelayConfig::new(&cfg.feeds.relay.socket),
        )
        .map_err(|e| format!("relay: {e}"))?;
        feeds.push(Box::new(r));
    }
    if feeds.is_empty() {
        return Err("no feed enabled".into());
    }
    let mut mux = FeedMux::new(feeds);

    // --- process assembly (recovery happens inside, before READY)
    let rslog_dir = PathBuf::from(opt(args, "--rslog-dir").unwrap_or(&cfg.obs.rslog_dir));
    let state_path = Some(PathBuf::from(
        opt(args, "--state").unwrap_or("/var/lib/rugsnipe/state.db"),
    ));
    let e1 = cfg.paper.fill_model == rugsnipe_core::config::FillModel::E1;
    let lane_rps = if e1 {
        let default: f64 = if funders.is_empty() { 3.5 } else { 2.0 };
        num(args, "--lane-rps", default)?.min(cfg.rpc_budget.hot as f64)
    } else {
        cfg.rpc_budget.hot.min(4) as f64 / 2.0
    };
    let e2 = (!flag(args, "--no-e2")).then(|| E2Cfg {
        rpc_url: rpc_url.clone(),
        budget: Arc::new(RpcBudget::new(lane_rps)),
        delay_ms: 20_000,
        max_tx: 12,
    });
    let paper_balance = cfg.risk.max_hot_balance_lamports.max(2_000_000_000);
    let mut app = app::build(AppOpts {
        cfg: cfg.clone(),
        armset,
        regime: opt(args, "--regime").map(String::from),
        paper_unsigned: flag(args, "--paper-unsigned-armset"),
        state_path,
        rslog_dir: rslog_dir.clone(),
        logsws: logsws_handle,
        healths: healths.clone(),
        e2,
        metrics: !flag(args, "--no-metrics"),
        // E1 verdicts wait for confirmed history plus the lane's queue
        fill_timeout_slots: num(
            args,
            "--fill-timeout-slots",
            if e1 { 1_500u64 } else { 300 },
        )?,
        lease: true,
        sigusr1: usr1,
        paper_balance,
        live_flag,
    })?;
    if let Some(r) = &app.recovery {
        eprintln!(
            "recovery: {} open position(s), {} abandoned paper position(s), {} nonce row(s)",
            r.open_positions.len(),
            r.abandoned_paper.len(),
            r.nonces.len()
        );
    }
    eprintln!(
        "rugsnipe-hot {} mode={} paper_wallet={} (throwaway) armset={:?}{} watch_funders={} rslog={}",
        rugsnipe::VERSION,
        mode,
        app.owner,
        app.armset_gate,
        app.armset_error
            .as_ref()
            .map(|e| format!(" ({e}: exits only)"))
            .unwrap_or_default(),
        funders.len(),
        rslog_dir.display()
    );
    let _ = sd_notify::notify(&[sd_notify::NotifyState::Ready]);

    // --- pin the hot loop (this thread) when possible
    let pin = opt(args, "--pin-core").unwrap_or("auto");
    if pin != "none" {
        if let Some(ids) = core_affinity::get_core_ids() {
            let want = match pin {
                "auto" => ids.last().copied(),
                n => n
                    .parse::<usize>()
                    .ok()
                    .and_then(|n| ids.iter().find(|c| c.id == n).copied()),
            };
            if let Some(c) = want {
                if core_affinity::set_for_current(c) {
                    eprintln!("hot loop pinned to core {}", c.id);
                }
            }
        }
    }

    let duration = num(args, "--duration-s", 0u64)?;
    let busy = flag(args, "--busy-poll");
    let t0 = Instant::now();
    let mut buf = FeedBuf::with_capacity(1_024, 1 << 20);
    let mut idle = 0u32;
    let mut last_print = Instant::now();
    loop {
        if term.load(Acquire) || (duration > 0 && t0.elapsed() >= Duration::from_secs(duration)) {
            break;
        }
        match app.hot.step(&mut mux, &mut buf) {
            FeedStatus::Ready(_) | FeedStatus::Full => idle = 0,
            FeedStatus::Ended => {
                eprintln!("feeds ended");
                break;
            }
            _ => {
                idle = idle.saturating_add(1);
                if !busy && idle > 64 {
                    std::thread::sleep(Duration::from_micros(200));
                }
            }
        }
        if last_print.elapsed() >= Duration::from_secs(60) {
            last_print = Instant::now();
            let c = app.hot.counters;
            let h: Vec<String> = healths
                .iter()
                .map(|h| {
                    let s = h.snapshot();
                    format!(
                        "{}: creates {} miss {} reconnects {} since_frame {} ms",
                        s.src.name(),
                        s.creates,
                        s.decode_miss,
                        s.reconnects,
                        s.since_frame_ms
                    )
                })
                .collect();
            let paper = {
                let r = app.report.lock().unwrap_or_else(|e| e.into_inner());
                let f = &r.paper.filter;
                // entry filter: intents / attempts / EV per attempt by tertile
                let tert: Vec<String> = ["low", "mid", "high"]
                    .iter()
                    .filter_map(|k| f.tertiles.get(*k).map(|t| (k, t)))
                    .map(|(k, t)| {
                        format!(
                            "{k} {}/{} ev {:+.3}",
                            t.intents, t.attempts, t.ev_per_attempt
                        )
                    })
                    .collect();
                format!(
                    "lane {}/{} calls {} | {:?} | landing {:.2} net {} lamports | filter {} {}",
                    r.e1_resolved,
                    r.e1_requested,
                    r.e1_rpc_calls,
                    r.paper.outcomes,
                    r.paper.landing_rate,
                    r.paper.net_lamports,
                    f.mode,
                    tert.join(", ")
                )
            };
            eprintln!(
                "[{:>5}s] events {} creates {} intents {} fills {} unfilled {} sells {} closed {} positions {} | {} | {}",
                t0.elapsed().as_secs(),
                c.events,
                c.creates,
                c.intents,
                c.fills,
                c.unfilled,
                c.sells,
                c.closed,
                app.hot.positions(),
                paper,
                h.join("; ")
            );
        }
    }
    let feeds_json: Vec<serde_json::Value> = healths
        .iter()
        .map(|h| {
            let s = h.snapshot();
            serde_json::json!({
                "src": s.src.name(), "connected": s.connected, "frames": s.frames, "events": s.events,
                "creates": s.creates, "decode_miss": s.decode_miss, "decode_errors": s.decode_errors,
                "decode_success_rate": s.decode_success_rate(), "failed_tx": s.failed_tx,
                "truncated": s.truncated, "reconnects": s.reconnects, "stale_reconnects": s.stale_reconnects,
                "ring_drops": s.ring_drops, "rpc_calls": s.rpc_calls, "rpc_errors": s.rpc_errors,
                "last_slot": s.last_slot,
            })
        })
        .collect();
    let elapsed = t0.elapsed().as_secs_f64();
    drop(mux); // stops the feed threads
    let summary = app.shutdown(feeds_json);
    let mut v = serde_json::to_value(&summary).map_err(|e| e.to_string())?;
    v["elapsed_s"] = serde_json::json!(elapsed);
    let text = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    println!("{text}");
    if let Some(p) = opt(args, "--summary") {
        std::fs::write(p, &text).map_err(|e| format!("{p}: {e}"))?;
    }
    Ok(())
}
