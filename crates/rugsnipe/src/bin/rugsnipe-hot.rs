//! `rugsnipe-hot`: the hot box process (docs/06 sections 7.1 and 12).
//!
//! Paper mode is the default and the only mode that runs without keys: the
//! wallet is a throwaway keypair generated at start, the only sender is the
//! paper recorder (no socket), and every decision is recorded (rslog +
//! state.db).
//!
//! **Probe** (docs/08 section 9) runs only when every gate of docs/06
//! section 6 passes: a signed ArmSet with `probe_allowed` and the config's
//! `probe_stage`, every cap, the clock offset measured by `chronyc`, a
//! healthy post-execution feed, the probe wallet and `api-keys` from
//! `$CREDENTIALS_DIRECTORY` (0400), and the real senders compiled in
//! (`--features real-senders`). Real buys need the post-execution feed:
//! `[feeds] grpc` (Yellowstone, feature `grpc`, on by default) is started
//! first and must deliver within `--feed-wait-s` (default 20 s), else probe
//! is refused with "no healthy post-execution feed"; decisions from the free
//! feeds stay paper. A gRPC feed configured in paper mode runs too (a feed,
//! not a sender: the first-seen benchmark compares it with the logs path).
//! `--probe-dry` signs everything with the real key (the per-sender buy
//! variants of a self-test included), prints the first signed transaction
//! and its `simulateTransaction` result (read-only, `sigVerify = true`) and
//! sends nothing. **Live** is refused in this build.
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

const USAGE: &str =
    "usage: rugsnipe-hot --config <toml> [--mode paper|probe|live] [--live] [--probe-dry]
    [--armset <armset-V-SHA.json>] [--paper-unsigned-armset] [--regime <name>]
    [--state <state.db>] [--rslog-dir <dir>] [--duration-s N] [--record <dir>]
    [--max-watches N] [--rpc-url <https url>] [--no-e2] [--lane-rps R] [--no-metrics]
    [--pin-core N|none] [--busy-poll] [--fill-timeout-slots N] [--summary <json>]
    [--feed-wait-s N] [--leader-schedule]

  --probe-dry  probe mode: load the real keys, sign with them, send nothing; print
               the first signed transaction (base64) and simulateTransaction results
  --no-e2      no paper fill lane (every model falls back to E2 from buffered trades)
  --feed-wait-s  probe: how long the gRPC feed may take to deliver its first slot (default 20)
  --leader-schedule  paper: also run the leader schedule lane (probe always does)
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

/// `--probe-dry`: print the self-test transactions (signed with the real
/// key, never sent) and their read-only `simulateTransaction` results.
fn dry_selftest(app: &app::App, rpc_url: &str) {
    use base64::Engine as _;
    let (variants, burn) = app.dry_selftest();
    if variants.is_empty() {
        eprintln!(
            "dry self-test: no usable nonce account (create them with `rugsnipe-ctl nonce init --authority {}`)",
            app.owner
        );
    }
    for (label, v) in &variants {
        eprintln!(
            "dry self-test: {label} variant {} sig {} tip {} prio {} ({} bytes)",
            v.idx,
            v.signature_b58(),
            v.tip_lamports,
            v.priority_lamports,
            v.bytes().len()
        );
    }
    let mut txs: Vec<(String, Vec<u8>)> = vec![];
    if let Some((label, v)) = variants.first() {
        txs.push((
            format!("first signed buy ({label}, placeholder mint)"),
            v.bytes().to_vec(),
        ));
    }
    if let Some(b) = burn {
        txs.push(("nonce burn (AdvanceNonce only)".into(), b));
    }
    for (what, tx) in txs {
        let b64 = base64::engine::general_purpose::STANDARD.encode(&tx);
        println!("dry {what}: {b64}");
        #[cfg(feature = "rpc")]
        {
            let c = rugsnipe_tx::rpc::RpcClient::new(rpc_url);
            let r = c.call(
                "simulateTransaction",
                serde_json::json!([b64, {"encoding": "base64", "sigVerify": true,
                    "replaceRecentBlockhash": false, "commitment": "confirmed"}]),
            );
            match r {
                Ok(v) => println!(
                    "dry {what}: simulateTransaction err={} units={} logs={}",
                    v["value"]["err"],
                    v["value"]["unitsConsumed"],
                    v["value"]["logs"].as_array().map(|l| l.len()).unwrap_or(0)
                ),
                Err(e) => println!("dry {what}: simulateTransaction failed: {e}"),
            }
        }
        #[cfg(not(feature = "rpc"))]
        {
            let _ = rpc_url;
            println!("dry {what}: simulateTransaction skipped (built without the rpc feature)");
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
    let dry = flag(args, "--probe-dry");
    if dry && mode != Mode::Probe {
        return Err("--probe-dry needs mode = probe (paper never loads keys)".into());
    }
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
    let rpc_url = opt(args, "--rpc-url")
        .unwrap_or("https://api.mainnet-beta.solana.com")
        .to_string();
    // --- the gRPC feed's x-token (paper may run the gRPC feed: a feed, not
    // a sender; the token is a credential like the sender keys)
    if cfg.feeds.grpc.enabled && !cfg!(feature = "grpc") {
        return Err(
            "feeds.grpc is enabled but this binary has no gRPC feed (build with --features grpc)"
                .into(),
        );
    }
    let grpc_token: Option<zeroize::Zeroizing<String>> = if cfg.feeds.grpc.enabled
        && !cfg.feeds.grpc.x_token.is_empty()
    {
        let name = &cfg.feeds.grpc.x_token;
        let found = match rugsnipe::keys::credentials_dir() {
            Ok(d) => rugsnipe::keys::named_secret(&cfg, &d, name)
                .map_err(|r| format!("feeds.grpc.x_token: {r}"))?,
            Err(_) => None,
        };
        match found {
                Some(t) => Some(t),
                None => {
                    return Err(format!(
                        "feeds.grpc.x_token {name:?}: not found as a line of the api-keys credential nor as $CREDENTIALS_DIRECTORY/{name} (empty x_token = no x-token header)"
                    ))
                }
            }
    } else {
        None
    };
    // Real modes are refused before any feed, state file or sender exists
    // (the post-execution feed's health is checked once it runs, below).
    let mut real_opts = None;
    let mut wallet: Option<pump_codec::Pubkey> = None;
    if mode.is_real() {
        let (gate, stage) = match &armset {
            ArmChoice::File(p) => {
                let r = rugsnipe::setup::load_armset_file(
                    p,
                    &cfg,
                    rugsnipe::now_unix(),
                    opt(args, "--regime"),
                );
                (r.gate, r.armset.as_ref().and_then(|a| a.probe_stage))
            }
            _ => (rugsnipe_core::config::ArmSetGate::Missing, None),
        };
        let creds = rugsnipe::keys::credentials_dir();
        let opts = app::RealOpts {
            creds_dir: creds.clone().unwrap_or_default(),
            clock_offset_ms: app::chrony_offset_ms(),
            // checked for real once the gRPC feed runs (below); assumed here so
            // that every other refusal is reported before any socket opens
            post_exec_feed_healthy: true,
            dry,
            rpc_url: rpc_url.clone(),
            chain: None,
        };
        let env = app::real_env(&cfg, gate, stage, live_flag, Some(&opts));
        let mut lines: Vec<String> = app::real_mode_gate(&cfg, &env)
            .err()
            .map(|e| e.lines().map(String::from).collect())
            .unwrap_or_default();
        if !cfg!(feature = "real-senders") {
            lines.push(
                "  - this binary has no real senders (build with --features real-senders)".into(),
            );
        }
        match &creds {
            Err(r) => lines.push(format!("  - {r}")),
            Ok(dir) if mode == Mode::Probe => match rugsnipe::keys::load_real_keys(&cfg, dir) {
                Ok(k) => wallet = Some(k.signer.pubkey()),
                Err(rs) => {
                    for r in rs {
                        let l = format!("  - {r}");
                        if !lines.contains(&l) {
                            lines.push(l);
                        }
                    }
                }
            },
            Ok(_) => {}
        }
        if !lines.is_empty() {
            return Err(format!("mode {mode} refused:\n{}", lines.join("\n")));
        }
        if dry {
            eprintln!(
                "--probe-dry: real keys loaded; every transaction is signed and recorded, nothing is sent"
            );
        }
        real_opts = Some(opts);
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
    // --- the post-execution feed first (probe waits for it before anything else)
    #[allow(unused_mut)]
    let mut post_exec: Option<rugsnipe::cold::PostExecFeed> = None;
    #[allow(unused_mut, unused_assignments)]
    let mut grpc_stats_line: Option<Box<dyn Fn() -> String>> = None;
    #[cfg(feature = "grpc")]
    if cfg.feeds.grpc.enabled {
        use rugsnipe_feed::grpc::{Grpc, GrpcConfig};
        let mut g = Grpc::spawn(GrpcConfig {
            url: cfg.feeds.grpc.url.clone(),
            x_token: grpc_token.as_ref().map(|t| t.to_string()),
            stale_ms: cfg.feeds.grpc.stale_ms,
            wallet,
            bundle_run: cfg.feeds.grpc.bundle_run,
            max_watches: num(args, "--max-watches", 64usize)?,
            ..Default::default()
        })
        .map_err(|e| format!("grpc feed: {e}"))?;
        let h = g.handle();
        healths.push(g.health_arc().clone());
        if mode.is_real() {
            let wait = num(args, "--feed-wait-s", 20u64)?;
            let t0 = Instant::now();
            let healthy = loop {
                let hs = h.health().snapshot();
                if hs.connected && hs.last_slot > 0 && !hs.stale {
                    break true;
                }
                if t0.elapsed() >= Duration::from_secs(wait) || term.load(Acquire) {
                    break false;
                }
                std::thread::sleep(Duration::from_millis(50));
            };
            let hs = h.health().snapshot();
            eprintln!(
                "grpc feed {}: connected {} slot {} reconnects {} ({} ms)",
                cfg.feeds.grpc.url,
                hs.connected,
                hs.last_slot,
                hs.reconnects,
                t0.elapsed().as_millis()
            );
            if let Some(o) = real_opts.as_mut() {
                let o: &mut app::RealOpts = o;
                o.post_exec_feed_healthy = healthy;
            }
            if !healthy && !dry {
                g.shutdown();
                return Err(format!(
                    "mode {mode} refused:\n  - no healthy post-execution feed (gRPC {}: connected {}, last slot {}, reconnects {} after {wait} s)",
                    cfg.feeds.grpc.url, hs.connected, hs.last_slot, hs.reconnects
                ));
            }
        }
        let bundles = g.take_bundles();
        let (hw, hs) = (h.clone(), h.clone());
        post_exec = Some(rugsnipe::cold::PostExecFeed {
            health: h.health().clone(),
            watch: Box::new(move |m, on| {
                if on {
                    hw.watch_mint(m);
                } else {
                    hw.unwatch_mint(m);
                }
            }),
            stats: Box::new(move || {
                use std::sync::atomic::Ordering::Relaxed as R;
                let s = hs.stats();
                serde_json::json!({
                    "slot": s.slot.load(R), "transactions": s.transactions.load(R),
                    "wire_events": s.wire_events.load(R), "trade_events": s.trade_events.load(R),
                    "slot_updates": s.slot_updates.load(R), "pings": s.pings.load(R),
                    "reordered": s.reordered.load(R), "convert_errors": s.convert_errors.load(R),
                    "bundle_runs": s.bundle_runs.load(R), "bundle_drops": s.bundle_drops.load(R),
                    "resubscribes": s.resubscribes.load(R),
                })
            }),
            bundles,
        });
        let hl = h.clone();
        grpc_stats_line = Some(Box::new(move || {
            use std::sync::atomic::Ordering::Relaxed as R;
            let s = hl.stats();
            format!(
                "grpc: slot {} tx {} pings {} reordered {} runs {}",
                s.slot.load(R),
                s.transactions.load(R),
                s.pings.load(R),
                s.reordered.load(R),
                s.bundle_runs.load(R)
            )
        }));
        feeds.push(Box::new(g));
    }
    #[cfg(not(feature = "grpc"))]
    let _ = (&grpc_token, wallet);
    if mode.is_real() && post_exec.is_none() && !dry {
        return Err(format!(
            "mode {mode} refused:\n  - no healthy post-execution feed (feeds.grpc is not enabled)"
        ));
    }
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
    // --- Telegram (optional, any mode): both credentials or nothing
    let alerts = match (&cfg.obs.alert.telegram, rugsnipe::keys::credentials_dir()) {
        (t, Ok(dir)) if !t.is_empty() => {
            let tok = rugsnipe::keys::named_secret(&cfg, &dir, t)
                .map_err(|r| format!("obs.alert.telegram: {r}"))?;
            let chat = rugsnipe::keys::named_secret(&cfg, &dir, &cfg.obs.alert.chat)
                .map_err(|r| format!("obs.alert.chat: {r}"))?;
            match (tok, chat) {
                (Some(token), Some(chat)) => Some(rugsnipe::cold::AlertCreds {
                    api_base: "https://api.telegram.org".into(),
                    token,
                    chat,
                    hourly: cfg.obs.alert.hourly,
                    commands: cfg.obs.alert.commands,
                }),
                _ => {
                    eprintln!(
                        "telegram alerts off: credentials {:?} / {:?} not found",
                        t, cfg.obs.alert.chat
                    );
                    None
                }
            }
        }
        _ => None,
    };
    let leader_rpc = (mode.is_real() || flag(args, "--leader-schedule")).then(|| rpc_url.clone());
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
        real: real_opts,
        post_exec,
        leader_rpc,
        alerts,
        trigger_tuning: None,
    })?;
    if let Some(r) = &app.recovery {
        eprintln!(
            "recovery: {} open position(s), {} abandoned paper position(s), {} nonce row(s)",
            r.open_positions.len(),
            r.abandoned_paper.len(),
            r.nonces.len()
        );
    }
    if dry {
        dry_selftest(&app, &rpc_url);
    }
    eprintln!(
        "rugsnipe-hot {} mode={}{} {}={} armset={:?}{} watch_funders={} rslog={}",
        rugsnipe::VERSION,
        mode,
        if dry { " (dry)" } else { "" },
        if mode == Mode::Paper {
            "paper_wallet(throwaway)"
        } else {
            "probe_wallet"
        },
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
                let probe = if mode.is_real() {
                    let p = &r.probe;
                    format!(
                        " | probe attempts {} landed {} failed {} dropped {} fail_cost {} wallet {}",
                        p.attempts, p.landed, p.landed_failed, p.dropped, p.failed_cost_lamports,
                        p.wallet_lamports
                    )
                } else {
                    String::new()
                };
                format!(
                    "lane {}/{} calls {} | {:?} | landing {:.2} net {} lamports | filter {} {}{probe}",
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
            let kill = {
                let r = app.report.lock().unwrap_or_else(|e| e.into_inner());
                let trig: Vec<String> = r
                    .kill_triggers
                    .iter()
                    .map(|(k, v)| format!("{k}: {}", v.detail))
                    .chain(
                        r.would_kill
                            .iter()
                            .map(|(k, v)| format!("(paper) {k}: {v}")),
                    )
                    .collect();
                let fs = r
                    .first_seen
                    .lag
                    .iter()
                    .map(|(k, l)| format!("{k} p50 {:.0} ms (n {})", l.p50_ms, l.n))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "killed {}{}{}{}",
                    if app.risk.is_killed() {
                        app.risk.kill_reason().map(|k| k.name()).unwrap_or("yes")
                    } else {
                        "no"
                    },
                    if trig.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", trig.join("; "))
                    },
                    if fs.is_empty() {
                        String::new()
                    } else {
                        format!(" | first-seen {fs}")
                    },
                    r.leaders
                        .epoch
                        .map(|e| format!(" | leaders epoch {e}"))
                        .unwrap_or_default()
                )
            };
            let g = grpc_stats_line
                .as_ref()
                .map(|f| format!(" | {}", f()))
                .unwrap_or_default();
            eprintln!(
                "[{:>5}s] events {} creates {} intents {} fills {} unfilled {} sells {} closed {} positions {} | {kill}{g} | {} | {}",
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
