//! The entry filter against the Python export (H1):
//!
//! * the Rust score equals scikit-learn's on the 200 exported rows
//!   (`entry_filter_v1_check.json`) to 1e-9, and so do the tertiles;
//! * the feature extraction reproduces the training definitions on real
//!   data (`entry_filter_v1_fixtures.json`): the 17 bundle features of 60
//!   creation slots (gap-3 bundle, `predictor.bundle_features`) and the 21
//!   create-transaction / uri features of 24 jsonParsed create transactions
//!   (`predictor.create_tx_features` + `feature_row`);
//! * scoring allocates nothing and stays far below 5 us per call.

use pump_codec::Pubkey;
use rugsnipe::alloc_count::{self, CountingAlloc};
use rugsnipe::filter::{self, Feat, Model, S0Trade, FEATURES, NF};
use serde_json::Value;
use std::path::PathBuf;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

fn report(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/reports/strategy")
        .join(name)
}

fn model() -> Model {
    Model::load(&report("entry_filter_v1.json")).unwrap()
}

fn check_rows() -> Vec<(Feat, f64, u8)> {
    let v: Value = serde_json::from_str(
        &std::fs::read_to_string(report("entry_filter_v1_check.json")).unwrap(),
    )
    .unwrap();
    let names: Vec<&str> = v["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    assert_eq!(names, FEATURES);
    v["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            let mut f = Feat::default();
            for (k, x) in r["x"].as_array().unwrap().iter().enumerate() {
                f.x[k] = x.as_f64().unwrap();
            }
            (
                f,
                r["score"].as_f64().unwrap(),
                r["tertile"].as_u64().unwrap() as u8,
            )
        })
        .collect()
}

#[test]
fn rust_score_equals_python_on_the_200_exported_rows() {
    let m = model();
    let rows = check_rows();
    assert_eq!(rows.len(), 200);
    let mut worst = 0.0f64;
    let mut by_tertile = [0u32; 3];
    for (f, want, t) in &rows {
        let s = m.score(f);
        worst = worst.max((s - want).abs());
        assert_eq!(m.tertile(s), *t);
        by_tertile[*t as usize] += 1;
    }
    assert!(worst <= 1e-9, "max |rust - python| = {worst:e}");
    assert!(by_tertile.iter().all(|n| *n > 20), "{by_tertile:?}");
    eprintln!("200 rows: max |rust - python| = {worst:e}, tertiles {by_tertile:?}");
}

#[test]
fn scoring_allocates_nothing_and_is_fast() {
    let m = model();
    let rows = check_rows();
    // warm up, then count
    let mut acc = 0.0;
    for (f, _, _) in &rows {
        acc += m.score(f);
    }
    alloc_count::reset();
    alloc_count::set_counting(true);
    let t0 = std::time::Instant::now();
    let reps = 50;
    for _ in 0..reps {
        for (f, _, _) in &rows {
            acc += std::hint::black_box(m.score(std::hint::black_box(f)));
        }
    }
    let per = t0.elapsed().as_nanos() as f64 / (reps * rows.len()) as f64;
    alloc_count::set_counting(false);
    assert_eq!(alloc_count::counts().0, 0, "score() allocated");
    assert!(acc > 0.0);
    // the 5 us budget is for release builds (examples/filter_bench.rs); a
    // debug build is ~10x slower and still has to stay inside it x 10
    let budget = if cfg!(debug_assertions) {
        50_000.0
    } else {
        5_000.0
    };
    assert!(per < budget, "{per:.0} ns per score");
    eprintln!(
        "score: {per:.0} ns per call ({} build)",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
}

fn fixtures() -> Value {
    serde_json::from_str(&std::fs::read_to_string(report("entry_filter_v1_fixtures.json")).unwrap())
        .unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1.0 + b.abs())
}

#[test]
fn bundle_features_match_the_training_definitions_on_real_creation_slots() {
    let fx = fixtures();
    let cases = fx["bundle"].as_array().unwrap();
    assert!(cases.len() >= 50);
    for c in cases {
        let cset: Vec<Pubkey> = c["creator_set"]
            .as_array()
            .unwrap()
            .iter()
            .map(|k| k.as_str().unwrap().parse().unwrap())
            .collect();
        let trades: Vec<S0Trade> = c["trades"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| S0Trade {
                index: t[0].as_u64().unwrap() as u32,
                is_create: t[1].as_bool().unwrap(),
                user: t[2].as_str().unwrap().parse().unwrap(),
                is_buy: t[3].as_bool().unwrap(),
                sol: t[4].as_u64().unwrap(),
                tok: t[5].as_u64().unwrap(),
                v: 0,
                t: 0,
            })
            .collect();
        let me = c["me"].as_f64().unwrap();
        let b = filter::bundle_features(&trades, &cset, Some(me)).expect("a bundle");
        for (k, name) in FEATURES[..filter::N_BUNDLE].iter().enumerate() {
            let want = c["expected"][*name].as_f64().unwrap();
            assert!(
                close(b.x[k], want),
                "{} {name}: rust {} python {want}",
                c["mint"],
                b.x[k]
            );
        }
        assert_eq!(
            b.wallets.len() as f64,
            c["expected"]["nB"].as_f64().unwrap()
        );
    }
}

#[test]
fn create_tx_features_match_the_training_definitions_on_real_creates() {
    let m = model();
    let fx = fixtures();
    let cases = fx["create_tx"].as_array().unwrap();
    assert!(cases.len() >= 20);
    let mut tips = 0;
    for c in cases {
        let tx = &c["tx"];
        let signer: Pubkey = c["signer"].as_str().unwrap().parse().unwrap();
        let creator: Pubkey = c["creator"].as_str().unwrap().parse().unwrap();
        let uri = filter::create_uri(tx);
        let x = filter::create_tx_features(
            tx,
            c["tx_index"].as_u64().map(|i| i as u32),
            &creator,
            &signer,
            &uri,
            &m.tip_accounts,
        );
        for (k, name) in FEATURES[filter::CREATE_TX..filter::CREATE_TX + filter::N_CREATE]
            .iter()
            .enumerate()
        {
            let want = c["expected"][*name].as_f64().unwrap();
            assert!(
                close(x[k], want),
                "{} {name}: rust {} python {want}",
                c["sig"],
                x[k]
            );
        }
        tips += (x[2] > 0.0) as u32;
    }
    eprintln!(
        "{} create transactions, {tips} with a Jito tip",
        cases.len()
    );
    assert_eq!(NF, FEATURES.len());
}
