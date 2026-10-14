//! Entry-filter scoring latency (H1: < 5 us per score on the hot thread).
//!
//! ```text
//! cargo run --release -p rugsnipe --example filter_bench [-- model.json check.json]
//! ```
//!
//! Scores the 200 exported parity rows (`entry_filter_v1_check.json`)
//! 20,000 times each through [`rugsnipe::filter::Model::score`], timing every
//! call, and prints the per-call quantiles plus the worst |Rust - Python|.

use rugsnipe::filter::{Feat, Model};
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/reports/strategy");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let model_p = args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("entry_filter_v1.json"));
    let check_p = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("entry_filter_v1_check.json"));
    let m = Model::load(&model_p).expect("model");
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&check_p).expect("check rows"))
            .expect("json");
    let rows: Vec<(Feat, f64)> = v["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|r| {
            let mut f = Feat::default();
            for (k, x) in r["x"].as_array().expect("x").iter().enumerate() {
                f.x[k] = x.as_f64().expect("number");
            }
            (f, r["score"].as_f64().expect("score"))
        })
        .collect();
    let worst = rows
        .iter()
        .map(|(f, s)| (m.score(f) - s).abs())
        .fold(0.0, f64::max);
    let reps = 20_000;
    let mut ns = Vec::with_capacity(reps * rows.len());
    let mut acc = 0.0;
    for _ in 0..reps {
        for (f, _) in &rows {
            let t = Instant::now();
            acc += std::hint::black_box(m.score(std::hint::black_box(f)));
            ns.push(t.elapsed().as_nanos() as u64);
        }
    }
    ns.sort_unstable();
    let q = |p: f64| ns[((ns.len() - 1) as f64 * p) as usize];
    let mean = ns.iter().sum::<u64>() as f64 / ns.len() as f64;
    println!(
        "{} trees, {} rows x {reps}: per score mean {mean:.0} ns, p50 {} ns, p90 {} ns, p99 {} ns, p99.9 {} ns, max {} ns; max |rust - python| = {worst:e} (checksum {acc:.3})",
        m.n_trees(),
        rows.len(),
        q(0.5),
        q(0.9),
        q(0.99),
        q(0.999),
        ns[ns.len() - 1]
    );
}
