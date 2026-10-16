//! The kill latch survives a SIGKILL and a restart (docs/06 section 13,
//! docs/08 section 10): the real `rugsnipe-hot` binary (paper, relay feed of
//! recorded creates, an ArmSet watching every signer) latches on the kill
//! file and persists `kv.kill_latch`; it is SIGKILLed; the kill file is
//! removed; the restarted process is still killed (no intent, the restored
//! latch in the summary) until the local unkill
//! (`rugsnipe_state::killkv::unkill`, what `rugsnipe-ctl unkill` runs); the
//! next start decides again.

mod common;

use common::*;
use std::sync::atomic::{AtomicBool, Ordering::*};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn latch(db: &std::path::Path) -> Option<rugsnipe_state::killkv::Latch> {
    let c = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .ok()?;
    let v: Option<String> = c
        .query_row("SELECT v FROM kv WHERE k = 'kill_latch'", [], |r| r.get(0))
        .ok();
    v.and_then(|s| rugsnipe_state::killkv::Latch::from_json(&s))
}

struct Started {
    child: std::process::Child,
    stop: Arc<AtomicBool>,
    relay: std::thread::JoinHandle<()>,
}

fn start(dir: &std::path::Path, run: u32, duration_s: u64) -> Started {
    let (lines, _) = fixture_relay();
    let sock = dir.join(format!("pp{run}.sock"));
    let stop = Arc::new(AtomicBool::new(false));
    let relay = serve_relay(&sock, lines, stop.clone());
    let cfg = dir.join(format!("paper{run}.toml"));
    std::fs::write(&cfg, paper_toml(dir, &sock, [9; 6])).unwrap();
    let child = hot_cmd(
        &cfg,
        &dir.join("armset.json"),
        &dir.join("state.db"),
        duration_s,
    )
    .arg("--summary")
    .arg(dir.join(format!("summary{run}.json")))
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::piped())
    .spawn()
    .unwrap();
    Started { child, stop, relay }
}

fn finish(s: Started, dir: &std::path::Path, run: u32) -> (serde_json::Value, String) {
    let Started { child, stop, relay } = s;
    let out = child.wait_with_output().unwrap();
    stop.store(true, Release);
    let _ = relay.join();
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(out.status.success(), "run {run}: {err}");
    let v = serde_json::from_slice(&std::fs::read(dir.join(format!("summary{run}.json"))).unwrap())
        .unwrap();
    (v, err)
}

#[test]
fn the_kill_latch_survives_sigkill_and_restart_until_a_local_unkill() {
    let dir = tmpdir("kill-persist");
    let (_, users) = fixture_relay();
    std::fs::write(
        dir.join("armset.json"),
        serde_json::to_vec(&armset(&users, &[])).unwrap(),
    )
    .unwrap();
    let db = dir.join("state.db");
    // 1: the kill file latches; the latch reaches state.db; SIGKILL
    std::fs::write(dir.join("KILL"), b"").unwrap();
    let mut s = start(&dir, 1, 60);
    let t0 = Instant::now();
    let l = loop {
        if let Some(l) = latch(&db) {
            break l;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "the latch never reached state.db"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(l.reason, "kill_file");
    s.child.kill().unwrap(); // SIGKILL: no summary, no shutdown path
    let _ = s.child.wait();
    s.stop.store(true, Release);
    let _ = s.relay.join();
    std::fs::remove_file(dir.join("KILL")).unwrap();
    // 2: restarted without the kill file: still killed
    let (sum, err) = finish(start(&dir, 2, 3), &dir, 2);
    assert!(err.contains("KILL latch restored from state.db"), "{err}");
    assert_eq!(sum["cold"]["killed"], true, "{}", sum["cold"]);
    assert!(sum["cold"]["kill_restored"]
        .as_str()
        .unwrap()
        .starts_with("kill_file"));
    assert_eq!(sum["counters"]["intents"], 0, "{}", sum["decisions"]);
    assert!(decision(&sum, "skip_killed") >= 1, "{}", sum["decisions"]);
    assert!(latch(&db).is_some(), "the latch stays");
    // 3: the local unkill (what `rugsnipe-ctl unkill --state` does), then a start decides again
    let c = rugsnipe_state::store::open_conn(&db).unwrap();
    let cleared = rugsnipe_state::killkv::unkill(&c, rugsnipe::now_unix()).unwrap();
    assert_eq!(cleared.map(|l| l.reason).as_deref(), Some("kill_file"));
    drop(c);
    let (sum, _) = finish(start(&dir, 3, 3), &dir, 3);
    assert_eq!(sum["cold"]["killed"], false, "{}", sum["cold"]);
    assert!(sum["cold"]["kill_restored"].is_null());
    assert!(
        sum["counters"]["intents"].as_u64().unwrap() >= 1,
        "{}",
        sum["decisions"]
    );
    assert!(latch(&db).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
