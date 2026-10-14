//! Conformance: replay the 20 tapes built from real pump.fun trade histories
//! (`scripts/pumpapi_tapes.py`) and compare with the checked-in expected
//! outputs (`fixtures/expected/*.jsonl`, the Python mirror's oracle).
//! Regenerate with `RUGSNIPE_BLESS=1 cargo test -p rugsnipe-exit --test tapes`.

use rugsnipe_exit::tape::{replay, Tape};
use rugsnipe_exit::{ExitAction, ExitReason, PositionState, SellKind};
use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn tapes() -> Vec<(String, Tape)> {
    tapes_in("tapes")
}

fn tapes_in(sub: &str) -> Vec<(String, Tape)> {
    let mut v: Vec<_> = std::fs::read_dir(dir().join(sub))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    v.sort();
    v.into_iter()
        .map(|p| {
            let name = p.file_stem().unwrap().to_string_lossy().to_string();
            let tape = Tape::parse(&std::fs::read_to_string(&p).unwrap())
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            (name, tape)
        })
        .collect()
}

#[test]
fn twenty_tapes_match_expected_outputs() {
    check_dir("tapes", "expected", 20);
}

/// Seeded synthetic tapes (`scripts/synth_tapes.py`): failed fills, retries,
/// escalation, spend caps, transfers, balances, completion and pool events.
#[test]
fn synthetic_tapes_match_expected_outputs() {
    check_dir("tapes_synth", "expected_synth", 12);
}

fn check_dir(sub: &str, expected: &str, n: usize) {
    let all = tapes_in(sub);
    assert_eq!(all.len(), n, "expected {n} tapes in {sub}");
    let bless = std::env::var_os("RUGSNIPE_BLESS").is_some();
    std::fs::create_dir_all(dir().join(expected)).unwrap();
    let mut mismatches = Vec::new();
    for (name, tape) in &all {
        let got = replay(tape).to_jsonl();
        let path = dir().join(expected).join(format!("{name}.jsonl"));
        if bless {
            std::fs::write(&path, &got).unwrap();
            continue;
        }
        let want = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("missing {path:?}; run with RUGSNIPE_BLESS=1"));
        if got != want {
            let line = got
                .lines()
                .zip(want.lines())
                .position(|(a, b)| a != b)
                .unwrap_or(got.lines().count().min(want.lines().count()));
            mismatches.push(format!(
                "{name}: first difference at line {}\n got: {}\nwant: {}",
                line + 1,
                got.lines().nth(line).unwrap_or("<eof>"),
                want.lines().nth(line).unwrap_or("<eof>")
            ));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
fn replay_is_deterministic_and_roundtrips() {
    for (name, tape) in tapes() {
        let a = replay(&tape).to_jsonl();
        let b = replay(&tape).to_jsonl();
        assert_eq!(a, b, "{name}");
        let again = Tape::parse(&tape.to_jsonl()).unwrap();
        assert_eq!(again, tape, "{name}: tape serialization roundtrip");
    }
}

#[test]
fn tapes_exercise_the_triggers_and_close() {
    let mut reasons = std::collections::BTreeMap::<String, usize>::new();
    let mut dumps = 0;
    for (name, tape) in tapes() {
        let out = replay(&tape);
        assert_eq!(
            out.summary.state,
            PositionState::Closed,
            "{name} did not close"
        );
        assert_eq!(out.summary.tokens, 0, "{name}");
        assert!(
            out.summary.sell_spend <= tape.header.policy.max_sell_spend_lamports,
            "{name}"
        );
        let mut opened = 0;
        for r in &out.records {
            match r.action {
                ExitAction::Opened { .. } => opened += 1,
                ExitAction::Sell(o) if o.try_no == 1 => {
                    *reasons.entry(o.reason.label()).or_default() += 1;
                    if o.kind == SellKind::Dump {
                        dumps += 1;
                    }
                }
                _ => {}
            }
        }
        assert_eq!(opened, 1, "{name}");
    }
    eprintln!("first-attempt sells by reason: {reasons:?}");
    assert!(dumps > 0, "no dump sells in the tapes");
    assert!(reasons.contains_key(&ExitReason::BundleSell.label()));
    assert!(
        reasons.keys().any(|k| k.starts_with("tp")),
        "no take-profit in the tapes"
    );
}
