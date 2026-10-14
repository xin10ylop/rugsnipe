//! `rugsnipe-replay`: recorded feed frames through the **same** hot loop as
//! `rugsnipe-hot` (docs/06 section 14 item 5), with a counting
//! `#[global_allocator]` (hot thread only) and hdrhistogram timing.
//!
//! After `--warmup` events the hot thread must not allocate at all; the run
//! reports p50/p99 of the decision path (event -> patched, signed, primary
//! variant handed to the paper sender) and of every event.
//!
//! ```text
//! rugsnipe-replay --frames data/raw/feed/logsws-*.jsonl [--watch-every 5]
//!     [--assert-zero-alloc] [--p99-max-us 100] [--summary out.json]
//! ```
//!
//! Without `--armset`, an in-memory paper ArmSet is synthesized from the
//! recording: the `user` of every K-th create becomes a signer key of one
//! armed operator (0.1 SOL, v_max 40 SOL) and every K-th creator argument a
//! shadow key, so the full decision path runs on real frames.

use rugsnipe::alloc_count::{self, CountingAlloc};
use rugsnipe::app::{self, AppOpts, ArmChoice, Quantiles};
use rugsnipe::cli::{flag, num, opt, opts};
use rugsnipe_core::armset::{ArmSet, OpEntry, OpState, WatchJson, WatchKind};
use rugsnipe_core::config::Config;
use rugsnipe_feed::replay::Replay;
use rugsnipe_feed::{Feed, FeedBuf, FeedMux, FeedStatus, Payload};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

const USAGE: &str =
    "usage: rugsnipe-replay --frames <jsonl> [--frames <jsonl> ...] [--config <toml>]
    [--armset <file> [--paper-unsigned-armset]] [--watch-every K] [--warmup N] [--loops N]
    [--state <db>] [--rslog-dir <dir>] [--no-trades] [--fill-timeout-slots N]
    [--assert-zero-alloc] [--p99-max-us N] [--summary <json>]";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if flag(&args, "--help") || flag(&args, "-h") {
        println!("{USAGE}");
        return;
    }
    match run(&args) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("rugsnipe-replay: {e}");
            std::process::exit(2);
        }
    }
}

fn synth_armset(replay: &Replay, every: usize) -> ArmSet {
    let mut watch = vec![];
    let mut n = 0usize;
    for ev in replay.events() {
        if let Payload::Hit(h) = ev.payload {
            if n.is_multiple_of(every) {
                watch.push(WatchJson {
                    key: h.user,
                    kind: WatchKind::Signer,
                    op_id: 1,
                    rule_id: 0,
                    expires_slot: None,
                    keep: false,
                });
            } else if n % every == 1 && h.creator != h.user {
                watch.push(WatchJson {
                    key: h.creator,
                    kind: WatchKind::Creator,
                    op_id: 1,
                    rule_id: 0,
                    expires_slot: None,
                    keep: false,
                });
            }
            n += 1;
        }
    }
    ArmSet {
        ver: 0,
        run_id: "replay-synthetic".into(),
        code_sha: String::new(),
        prereg_sha: String::new(),
        created_at: rugsnipe::now_unix(),
        valid_until: None,
        regime: "replay".into(),
        min_hot_version: String::new(),
        gate_passed: false,
        shared_fee_payers: vec![],
        operators: vec![OpEntry {
            op_id: 1,
            state: OpState::Armed,
            size_lamports: 100_000_000,
            v_max_lamports: 40_000_000_000,
            ev_cap_lamports: 0,
            ab_skip_p: 0.0,
            family: None,
            exit: None,
            stats: None,
        }],
        watch,
        funding_rules: vec![],
    }
}

fn run(args: &[String]) -> Result<i32, String> {
    let frames = opts(args, "--frames");
    if frames.is_empty() {
        return Err(format!("--frames required\n{USAGE}"));
    }
    let emit_trades = !flag(args, "--no-trades");
    let mut text = String::new();
    for f in &frames {
        text.push_str(&std::fs::read_to_string(f).map_err(|e| format!("{f}: {e}"))?);
        if !text.ends_with('\n') {
            text.push('\n');
        }
    }
    let loops = num(args, "--loops", 1usize)?.max(1);
    let mut cfg = match opt(args, "--config") {
        Some(p) => rugsnipe::setup::load_config(&PathBuf::from(p))?,
        None => Config::default(),
    };
    if cfg.mode.mode.is_real() {
        return Err("rugsnipe-replay is paper only".into());
    }
    rugsnipe::setup::paper_caps(&mut cfg.risk);
    let tmp = std::env::temp_dir().join(format!("rugsnipe-replay-{}", std::process::id()));
    let rslog_dir = opt(args, "--rslog-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| tmp.join("rslog"));
    let state_path = match opt(args, "--state") {
        Some("none") => None,
        Some(p) => Some(PathBuf::from(p)),
        None => Some(tmp.join("state.db")),
    };
    // the kill file of a real box must not stop a replay (and vice versa)
    cfg.risk.kill_file = tmp.join("KILL").display().to_string();

    let first = Replay::from_text(&text, emit_trades);
    let load = first.load_health().snapshot();
    let armset = match opt(args, "--armset") {
        Some(p) => ArmChoice::File(PathBuf::from(p)),
        None => ArmChoice::Memory(Box::new(synth_armset(
            &first,
            num(args, "--watch-every", 5usize)?.max(1),
        ))),
    };
    let n_events = first.events().len() * loops;
    let mut feeds: Vec<Box<dyn Feed + Send>> = vec![Box::new(first.restamped())];
    for _ in 1..loops {
        // later loops: the same frames with distinct signatures would be needed to
        // avoid dedup; repeated loops measure the dedup/duplicate path instead
        feeds.push(Box::new(Replay::from_text(&text, emit_trades).restamped()));
    }
    let mut mux = FeedMux::new(feeds);
    let mut app = app::build(AppOpts {
        cfg,
        armset,
        regime: None,
        paper_unsigned: flag(args, "--paper-unsigned-armset"),
        state_path,
        rslog_dir: rslog_dir.clone(),
        logsws: None,
        healths: vec![],
        e2: None,
        metrics: false,
        fill_timeout_slots: num(args, "--fill-timeout-slots", 20u64)?,
        lease: false,
        sigusr1: Arc::new(AtomicBool::new(false)),
        paper_balance: 5_000_000_000,
        live_flag: false,
    })?;
    let warmup = num(args, "--warmup", (n_events / 10).clamp(200, 5_000) as u64)?;
    let mut buf = FeedBuf::with_capacity(256, 1 << 20);
    let mut counted_events = 0u64;
    let mut counting = false;
    let t0 = std::time::Instant::now();
    loop {
        if !counting && app.hot.counters.events >= warmup {
            app.hot.reset_histograms();
            alloc_count::reset();
            alloc_count::set_counting(true);
            counting = true;
            counted_events = app.hot.counters.events;
        }
        let st = app.hot.step(&mut mux, &mut buf);
        if st == FeedStatus::Ended {
            break;
        }
    }
    alloc_count::set_counting(false);
    let (allocs, bytes) = alloc_count::counts();
    let wall = t0.elapsed().as_secs_f64();
    let measured_events = app.hot.counters.events.saturating_sub(counted_events);
    let (hd, hw, he) = app.hot.histograms();
    let (qd, qe) = (Quantiles::of(hd), Quantiles::of(he));
    let _ = hw;
    // let the cold side catch up, then stop
    std::thread::sleep(std::time::Duration::from_millis(300));
    drop(mux);
    let summary = app.shutdown(vec![]);
    let p99_max_us = num(args, "--p99-max-us", 100u64)?;
    let mut v = serde_json::to_value(&summary).map_err(|e| e.to_string())?;
    v["replay"] = serde_json::json!({
        "frames": frames,
        "events_loaded": n_events,
        "load": {"creates": load.creates, "decode_miss": load.decode_miss, "decode_success_rate": load.decode_success_rate(),
                 "truncated": load.truncated, "failed_tx": load.failed_tx},
        "warmup_events": warmup,
        "measured_events": measured_events,
        "hot_thread_allocations_after_warmup": allocs,
        "hot_thread_alloc_bytes_after_warmup": bytes,
        "decision_ns_after_warmup": qd,
        "event_ns_after_warmup": qe,
        "wall_s": wall,
        "p99_max_us": p99_max_us,
        "state_db": tmp.join("state.db"),
        "rslog_dir": rslog_dir,
    });
    let text = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    println!("{text}");
    if let Some(p) = opt(args, "--summary") {
        std::fs::write(p, &text).map_err(|e| format!("{p}: {e}"))?;
    }
    let mut code = 0;
    if flag(args, "--assert-zero-alloc") && allocs != 0 {
        eprintln!("FAIL: {allocs} allocation(s) ({bytes} bytes) on the hot thread after warm-up");
        code = 1;
    }
    if flag(args, "--assert-p99") || opt(args, "--p99-max-us").is_some() {
        if qd.n == 0 {
            eprintln!("FAIL: no decision reached the send path (nothing to time)");
            code = 1;
        } else if qd.p99 > p99_max_us * 1_000 {
            eprintln!(
                "FAIL: decision p99 {} us > {} us",
                qd.p99 / 1_000,
                p99_max_us
            );
            code = 1;
        }
    }
    Ok(code)
}
