//! Live tail of the free LogsWs feed (paper only, read-only, no keys).
//!
//! ```text
//! cargo run --release -p rugsnipe-feed --example tail -- [--secs 600] [--record DIR]
//!     [--ws wss://api.mainnet-beta.solana.com] [--rpc https://api.mainnet-beta.solana.com]
//!     [--sample-ms 2500] [--summary PATH]
//! ```
//!
//! Prints one line per minute (creates, decode success, reconnects, lag) and a
//! JSON summary at the end. Lag = local receive wall time - `getBlockTime(slot)`
//! (integer seconds, sampled at most every `--sample-ms` on the cold RPC
//! client), plus a second estimate against a least-squares slot -> time fit
//! over all samples, which averages out the 1 s rounding of `blockTime`.

use rugsnipe_feed::clock::{mono_ns, unix_ns};
use rugsnipe_feed::record::RecordConfig;
use rugsnipe_feed::ws::rpc::{get_block_time_body, RpcClient};
use rugsnipe_feed::ws::{LogsWs, LogsWsConfig};
use rugsnipe_feed::{hit_flags, Feed, FeedBuf, Payload};
use std::sync::mpsc;
use std::time::Duration;

struct Args {
    secs: u64,
    record: Option<String>,
    ws: String,
    rpc: String,
    sample_ms: u64,
    summary: Option<String>,
}

fn args() -> Args {
    let mut a = Args {
        secs: 600,
        record: None,
        ws: "wss://api.mainnet-beta.solana.com".into(),
        rpc: "https://api.mainnet-beta.solana.com".into(),
        sample_ms: 2500,
        summary: None,
    };
    let v: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < v.len() {
        let next = v.get(i + 1).cloned().unwrap_or_default();
        match v[i].as_str() {
            "--secs" => a.secs = next.parse().unwrap_or(600),
            "--record" => a.record = Some(next),
            "--ws" => a.ws = next,
            "--rpc" => a.rpc = next,
            "--sample-ms" => a.sample_ms = next.parse().unwrap_or(2500),
            "--summary" => a.summary = Some(next),
            _ => {
                eprintln!("unknown argument {}", v[i]);
                std::process::exit(2);
            }
        }
        i += 2;
    }
    a
}

/// (slot, recv_unix_ms) -> (slot, recv_unix_ms, block_time_s)
fn sampler(rpc: String, rx: mpsc::Receiver<(u64, u64)>, tx: mpsc::Sender<(u64, u64, i64)>) {
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return;
    };
    rt.block_on(async move {
        let Ok(mut c) = RpcClient::new(&rpc) else {
            return;
        };
        let mut id = 0u64;
        while let Ok((slot, recv_ms)) = rx.recv() {
            // the block must be confirmed before getBlockTime answers
            tokio::time::sleep(Duration::from_millis(2500)).await;
            for _ in 0..3 {
                id += 1;
                match c.call(&get_block_time_body(id, slot)).await {
                    Ok(text) => {
                        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
                        if let Some(t) = v["result"].as_i64() {
                            let _ = tx.send((slot, recv_ms, t));
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(1500)).await;
                    }
                    Err(_) => tokio::time::sleep(Duration::from_millis(1500)).await,
                }
            }
        }
    });
}

fn pct(v: &mut [f64], p: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let i = ((v.len() - 1) as f64 * p).round() as usize;
    v[i]
}

fn main() {
    let a = args();
    let mut cfg = LogsWsConfig {
        url: a.ws.clone(),
        rpc_url: None, // no funders watched here
        ..Default::default()
    };
    if let Some(dir) = &a.record {
        let mut rc = RecordConfig::new(dir, "logsws");
        rc.max_file_bytes = 32 << 20;
        rc.max_files = 4;
        cfg.record = Some(rc);
    }
    let mut feed = LogsWs::spawn(cfg).expect("spawn LogsWs");
    let (sreq_tx, sreq_rx) = mpsc::channel::<(u64, u64)>();
    let (sres_tx, sres_rx) = mpsc::channel::<(u64, u64, i64)>();
    let rpc = a.rpc.clone();
    std::thread::spawn(move || sampler(rpc, sreq_rx, sres_tx));

    // mono -> unix conversion
    let (mono0, unix0) = (mono_ns(), unix_ns());
    let to_unix_ms = |mono: u64| (unix0 + mono.saturating_sub(mono0)) / 1_000_000;

    let mut buf = FeedBuf::with_capacity(1024, 0);
    let start = mono_ns();
    let end = start + a.secs * 1_000_000_000;
    let mut last_sample = 0u64;
    let mut minute_creates: Vec<u64> = vec![0];
    let mut minute_start = start;
    let (mut hits, mut with_buys, mut mayhem, mut non_sol, mut truncated) = (0u64, 0, 0, 0, 0);
    let mut samples: Vec<(u64, u64, i64)> = Vec::new();
    let h = feed.health_arc().clone();
    eprintln!(
        "tailing {} for {} s (logsSubscribe mentions TSLvdd…, processed)",
        a.ws, a.secs
    );
    while mono_ns() < end {
        buf.clear();
        feed.poll(&mut buf);
        for ev in buf.events() {
            if let Payload::Hit(hit) = ev.payload {
                hits += 1;
                *minute_creates.last_mut().unwrap() += 1;
                if hit.intx_buys > 0 {
                    with_buys += 1;
                }
                if hit.mayhem {
                    mayhem += 1;
                }
                if !hit.is_sol_quote() {
                    non_sol += 1;
                }
                if hit.has(hit_flags::LOGS_TRUNCATED) {
                    truncated += 1;
                }
                let now = mono_ns();
                if now.saturating_sub(last_sample) >= a.sample_ms * 1_000_000 {
                    last_sample = now;
                    let _ = sreq_tx.send((ev.slot, to_unix_ms(ev.recv_ns)));
                }
            }
        }
        while let Ok(s) = sres_rx.try_recv() {
            samples.push(s);
        }
        if mono_ns() - minute_start >= 60_000_000_000 {
            let s = h.snapshot();
            let mut lags: Vec<f64> = samples
                .iter()
                .map(|(_, r, t)| *r as f64 - *t as f64 * 1000.0)
                .collect();
            eprintln!(
                "min {:>2}: creates {:>4} | total {:>5} decode {:.4} ({} miss) failed_tx {} | reconnects {} stale {} | lag median {:.0} ms (n={})",
                minute_creates.len(),
                minute_creates.last().unwrap(),
                hits,
                s.decode_success_rate(),
                s.decode_miss,
                s.failed_tx,
                s.reconnects,
                s.stale_reconnects,
                pct(&mut lags, 0.5),
                lags.len()
            );
            minute_creates.push(0);
            minute_start += 60_000_000_000;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    // let the last samples arrive
    std::thread::sleep(Duration::from_secs(5));
    while let Ok(s) = sres_rx.try_recv() {
        samples.push(s);
    }
    let s = h.snapshot();
    feed.shutdown();

    let mut lags: Vec<f64> = samples
        .iter()
        .map(|(_, r, t)| *r as f64 - *t as f64 * 1000.0)
        .collect();
    // least squares t_ms = a + b * slot over the samples
    let n = samples.len() as f64;
    let (fit_a, fit_b) = if samples.len() >= 3 {
        let s0 = samples[0].0 as f64;
        let xs: Vec<f64> = samples.iter().map(|x| x.0 as f64 - s0).collect();
        let ys: Vec<f64> = samples.iter().map(|x| x.2 as f64 * 1000.0).collect();
        let mx = xs.iter().sum::<f64>() / n;
        let my = ys.iter().sum::<f64>() / n;
        let sxy: f64 = xs.iter().zip(&ys).map(|(x, y)| (x - mx) * (y - my)).sum();
        let sxx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum();
        let b = if sxx > 0.0 { sxy / sxx } else { 400.0 };
        (my - b * mx - b * s0, b)
    } else {
        (f64::NAN, f64::NAN)
    };
    let mut fit_lags: Vec<f64> = samples
        .iter()
        .map(|(slot, r, _)| *r as f64 - (fit_a + fit_b * *slot as f64))
        .collect();
    let full_minutes: Vec<u64> = minute_creates
        .iter()
        .take(minute_creates.len().saturating_sub(1))
        .copied()
        .collect();
    let per_min = if full_minutes.is_empty() {
        hits as f64 / (a.secs as f64 / 60.0)
    } else {
        full_minutes.iter().sum::<u64>() as f64 / full_minutes.len() as f64
    };
    let summary = serde_json::json!({
        "endpoint": a.ws,
        "duration_s": a.secs,
        "creates": hits,
        "creates_per_min": (per_min * 100.0).round() / 100.0,
        "creates_by_minute": full_minutes,
        "decode_success_rate": s.decode_success_rate(),
        "decode_miss": s.decode_miss,
        "decode_errors": s.decode_errors,
        "failed_tx_ignored": s.failed_tx,
        "truncated_logs": truncated,
        "frames": s.frames,
        "reconnects": s.reconnects,
        "stale_reconnects": s.stale_reconnects,
        "ring_drops": s.ring_drops,
        "share_with_intx_buys": with_buys as f64 / hits.max(1) as f64,
        "share_mayhem": mayhem as f64 / hits.max(1) as f64,
        "share_non_sol_quote": non_sol as f64 / hits.max(1) as f64,
        "lag_samples": samples.len(),
        "lag_ms_vs_blocktime": {"p25": pct(&mut lags.clone(), 0.25), "median": pct(&mut lags, 0.5), "p75": pct(&mut lags.clone(), 0.75)},
        "lag_ms_vs_slot_fit": {"median": pct(&mut fit_lags, 0.5), "slot_ms": fit_b},
    });
    let text = serde_json::to_string_pretty(&summary).unwrap();
    println!("{text}");
    if let Some(p) = a.summary {
        let _ = std::fs::write(p, text);
    }
}
