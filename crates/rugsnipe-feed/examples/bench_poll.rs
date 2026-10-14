//! Hot-loop micro-benchmark: FeedMux (cross-feed dedup) over a Replay of
//! recorded frames, plus a FiredSet claim per create. Reports per-event cost.
//!
//! `cargo run --release -p rugsnipe-feed --example bench_poll -- [N]`

use rugsnipe_feed::replay::Replay;
use rugsnipe_feed::{Feed, FeedBuf, FeedMux, FeedStatus, FiredSet};
use std::time::Instant;

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000);
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/logs_frames.jsonl"
    ))
    .unwrap();
    let frames: Vec<serde_json::Value> = text
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .filter(|v| v["frame"]["method"] == "logsNotification")
        .collect();
    // n distinct notifications (fresh signatures) built from the recorded ones
    let mut lines = String::new();
    for i in 0..n {
        let mut v = frames[i % frames.len()].clone();
        let mut sig = [0u8; 64];
        sig[..8].copy_from_slice(&(i as u64).to_le_bytes());
        sig[63] = 1;
        v["frame"]["params"]["result"]["value"]["signature"] =
            bs58::encode(sig).into_string().into();
        v["recv_ns"] = (i as u64 * 1000).into();
        lines.push_str(&v.to_string());
        lines.push('\n');
    }
    let t_load = Instant::now();
    let replay = Replay::from_text(&lines, false);
    let load = t_load.elapsed();
    let events = replay.events().len();
    let mut mux = FeedMux::new(vec![Box::new(replay)]);
    let fired = FiredSet::default();
    let mut buf = FeedBuf::with_capacity(64, 0);
    let mut per_poll: Vec<(u64, usize)> = Vec::with_capacity(events / 8 + 16);
    let t0 = Instant::now();
    loop {
        buf.clear();
        let t = Instant::now();
        let st = mux.poll(&mut buf);
        for ev in buf.events() {
            if let Some(m) = ev.mint() {
                std::hint::black_box(fired.first(&m, 0, ev.src));
            }
        }
        let dt = t.elapsed().as_nanos() as u64;
        if buf.len() > 0 {
            per_poll.push((dt, buf.len()));
        }
        if st == FeedStatus::Ended {
            break;
        }
    }
    let total = t0.elapsed();
    let mut per_event: Vec<f64> = per_poll
        .iter()
        .map(|(d, n)| *d as f64 / *n as f64)
        .collect();
    per_event.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let q = |p: f64| per_event[((per_event.len() - 1) as f64 * p) as usize];
    println!(
        "{events} events (load/decode {:.1} ms = {:.1} us/frame on the adapter side); poll+dedup+fired: {:.0} ns/event mean, per-poll-batch p50 {:.0} ns/event, p99 {:.0} ns/event",
        load.as_secs_f64() * 1e3,
        load.as_secs_f64() * 1e6 / n as f64,
        total.as_nanos() as f64 / events as f64,
        q(0.5),
        q(0.99)
    );
}
