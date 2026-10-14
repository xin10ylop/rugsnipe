//! Recovery after SIGKILL during a paper run (docs/06 sections 12 and 14
//! item 9, invariant I6 in paper form). The real `rugsnipe-hot` binary is
//! killed with SIGKILL while paper positions are open and nonces are in
//! flight. Then:
//!
//! * `state.db` (WAL) is intact and holds every intent, the open positions,
//!   the nonce rows and the dead process's exits lease;
//! * the restarted process runs recovery before READY: it loads every open
//!   position (paper positions cannot be resumed, so they are closed as
//!   abandoned) and every nonce row, then takes the exits lease over from
//!   the dead owner (compare-and-set, `epoch + 1`) once its heartbeat is
//!   older than `takeover_after_ms`, and runs normally.

mod common;

use common::*;
use rusqlite::{Connection, OpenFlags};
use std::os::unix::process::ExitStatusExt;
use std::sync::atomic::{AtomicBool, Ordering::*};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn ro(p: &std::path::Path) -> Option<Connection> {
    let c = Connection::open_with_flags(
        p,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    c.busy_timeout(Duration::from_millis(500)).ok()?;
    Some(c)
}

fn q(c: &Connection, sql: &str) -> i64 {
    c.query_row(sql, [], |r| r.get(0)).unwrap_or(-1)
}

fn lease(c: &Connection) -> Option<(String, i64)> {
    c.query_row(
        "SELECT owner, epoch FROM lease WHERE name = 'exits'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .ok()
}

#[test]
fn sigkill_mid_run_then_recovery_before_ready() {
    let dir = tmpdir("sigkill");
    let (lines, users) = fixture_relay();
    let armset_path = dir.join("armset.json");
    std::fs::write(
        &armset_path,
        serde_json::to_vec(&armset(&users, &[])).unwrap(),
    )
    .unwrap();
    let state = dir.join("state.db");
    // --- run 1: decisions, deadline fills (no trades on the relay), then SIGKILL
    let stop = Arc::new(AtomicBool::new(false));
    let sock = dir.join("pp1.sock");
    let relay = serve_relay(&sock, lines, stop.clone());
    let cfg1 = dir.join("paper1.toml");
    std::fs::write(&cfg1, paper_toml(&dir, &sock, [9; 6])).unwrap();
    let mut child = hot_cmd(&cfg1, &armset_path, &state, 120)
        .arg("--fill-timeout-slots")
        .arg("2")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let pid1 = child.id();
    let t0 = Instant::now();
    loop {
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "no open paper position appeared in state.db"
        );
        if let Some(c) = ro(&state) {
            let open = q(&c, "SELECT COUNT(*) FROM positions WHERE state = 'open'");
            let inflight = q(&c, "SELECT COUNT(*) FROM nonces WHERE state = 'inflight'");
            let leased = lease(&c).is_some_and(|(o, _)| o.ends_with(&format!(":{pid1}")));
            if open >= 2 && inflight >= 1 && leased {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    child.kill().unwrap(); // SIGKILL
    let st = child.wait().unwrap();
    assert_eq!(st.signal(), Some(9), "killed by SIGKILL");
    stop.store(true, Release);
    let _ = relay.join();
    // --- what the dead process left behind
    let c = Connection::open(&state).unwrap();
    let integrity: String = c
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    let open_before = q(
        &c,
        "SELECT COUNT(*) FROM positions WHERE state IN ('open','exiting')",
    );
    let intents_before = q(&c, "SELECT COUNT(*) FROM intents WHERE state = 'paper'");
    let nonce_rows = q(&c, "SELECT COUNT(*) FROM nonces");
    let (owner1, epoch1) = lease(&c).expect("lease row");
    assert!(open_before >= 2, "open positions at kill: {open_before}");
    assert!(intents_before >= open_before);
    assert_eq!(nonce_rows, 32, "one row per buy nonce");
    assert!(owner1.ends_with(&format!(":{pid1}")), "{owner1}");
    drop(c);
    // --- run 2: restart on the same state.db (the feed has nothing new)
    let stop = Arc::new(AtomicBool::new(false));
    let sock2 = dir.join("pp2.sock");
    let relay2 = serve_relay(&sock2, String::new(), stop.clone());
    let cfg2 = dir.join("paper2.toml");
    std::fs::write(&cfg2, paper_toml(&dir, &sock2, [9; 6])).unwrap();
    let mut child2 = hot_cmd(&cfg2, &armset_path, &state, 9)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let pid2 = child2.id();
    let mut took_over = None;
    let t0 = Instant::now();
    let status = loop {
        if let Some(s) = child2.try_wait().unwrap() {
            break s;
        }
        assert!(t0.elapsed() < Duration::from_secs(40), "restart hangs");
        if took_over.is_none() {
            if let Some((o, e)) = ro(&state).as_ref().and_then(lease) {
                if o.ends_with(&format!(":{pid2}")) {
                    took_over = Some((o, e, t0.elapsed()));
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    stop.store(true, Release);
    let _ = relay2.join();
    let mut err = String::new();
    std::io::Read::read_to_string(child2.stderr.as_mut().unwrap(), &mut err).unwrap();
    assert!(status.success(), "{err}");
    // recovery ran before READY and found everything the dead process left
    let want = format!(
        "recovery: 0 open position(s), {open_before} abandoned paper position(s), {nonce_rows} nonce row(s)"
    );
    assert!(err.contains(&want), "want {want:?} in\n{err}");
    let i_rec = err.find("recovery:").unwrap();
    let i_ready = err.find("rugsnipe-hot 0.1").unwrap();
    assert!(i_rec < i_ready, "recovery is reported before the READY banner");
    // the dead owner's lease was taken over (compare-and-set, epoch + 1)
    let (o2, e2, after) = took_over.expect("the restarted process took the exits lease over");
    assert_eq!(e2, epoch1 + 1, "{o2}");
    assert!(after >= Duration::from_secs(1), "not before the old heartbeat went stale");
    assert!(err.contains("exits lease held by"), "{err}");
    // the database after the restart: history kept, nothing left open
    let c = Connection::open(&state).unwrap();
    assert_eq!(
        q(&c, "SELECT COUNT(*) FROM intents WHERE state = 'paper'"),
        intents_before
    );
    assert_eq!(
        q(
            &c,
            "SELECT COUNT(*) FROM positions WHERE state IN ('open','exiting')"
        ),
        0
    );
    assert_eq!(
        q(&c, "SELECT COUNT(*) FROM positions WHERE state = 'closed'"),
        open_before
    );
    eprintln!(
        "SIGKILLed with {open_before} open paper position(s), {intents_before} intent(s), {nonce_rows} nonce row(s); restart recovered them, took the lease over at epoch {e2} after {after:?}; nonce rows now {}",
        q(&c, "SELECT COUNT(*) FROM nonces")
    );
    let _ = std::fs::remove_dir_all(&dir);
}
