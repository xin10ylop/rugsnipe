//! I1 at the process level: the real `rugsnipe-hot` binary in paper mode,
//! fed real recorded `logsSubscribe` frames (rugsnipe-feed fixture) through
//! the collector relay (a Unix socket served by this test), with an ArmSet
//! that watches every create's signer, **and every real sender enabled in
//! the TOML pointing at local mocks**, decides and records paper buys but:
//!
//! * never opens an AF_INET/AF_INET6 socket (its sockets are sampled from
//!   `/proc/<pid>/fd` and classified through `/proc/<pid>/net/*` every few ms;
//!   the relay's AF_UNIX socket is the positive control of the sampler);
//! * never connects to any configured sender endpoint (each mock counts
//!   accepted connections).
//!
//! The E2 lane (`--no-e2`) and metrics (`--no-metrics`) are off here: they
//! are the only other network users in paper mode (read-only RPC
//! `getSignaturesForAddress`/`getTransaction` and a loopback listener),
//! covered by the strace run documented in the verification report.
//!
//! The kill file, present before start, stops every buy of the binary.

mod common;

use common::*;
use std::collections::BTreeSet;
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::*};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Mock {
    port: u16,
    accepted: Arc<AtomicU64>,
}

fn mock(stop: &Arc<AtomicBool>) -> Mock {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.set_nonblocking(true).unwrap();
    let port = l.local_addr().unwrap().port();
    let accepted = Arc::new(AtomicU64::new(0));
    let (a, s) = (accepted.clone(), stop.clone());
    std::thread::spawn(move || {
        while !s.load(Acquire) {
            match l.accept() {
                Ok(_) => {
                    a.fetch_add(1, AcqRel);
                }
                Err(_) => std::thread::sleep(Duration::from_millis(5)),
            }
        }
    });
    Mock { port, accepted }
}

/// Socket inodes the process holds now.
fn socket_inodes(pid: u32) -> BTreeSet<u64> {
    let mut out = BTreeSet::new();
    if let Ok(d) = std::fs::read_dir(format!("/proc/{pid}/fd")) {
        for e in d.flatten() {
            if let Ok(l) = std::fs::read_link(e.path()) {
                let l = l.to_string_lossy();
                if let Some(n) = l.strip_prefix("socket:[").and_then(|x| x.strip_suffix(']')) {
                    if let Ok(i) = n.parse() {
                        out.insert(i);
                    }
                }
            }
        }
    }
    out
}

/// Inodes listed in `/proc/<pid>/net/<table>` (inode column `col`).
fn table_inodes(pid: u32, table: &str, col: usize) -> BTreeSet<u64> {
    std::fs::read_to_string(format!("/proc/{pid}/net/{table}"))
        .unwrap_or_default()
        .lines()
        .skip(1)
        .filter_map(|l| l.split_whitespace().nth(col)?.parse().ok())
        .collect()
}

struct Run {
    summary: serde_json::Value,
    stderr: String,
    samples: u64,
    seen: BTreeSet<u64>,
    unix: BTreeSet<u64>,
    inet: BTreeSet<u64>,
    contacted: Vec<(&'static str, u64)>,
}

fn run(name: &str, killed_before_start: bool) -> Run {
    let dir = tmpdir(name);
    let stop = Arc::new(AtomicBool::new(false));
    let names = [
        "rpcsend",
        "helius",
        "nozomi",
        "astralane",
        "jito",
        "nextblock",
        "rpc-url",
    ];
    let mocks: Vec<Mock> = names.iter().map(|_| mock(&stop)).collect();
    let (lines, users) = fixture_relay();
    assert!(users.len() >= 10, "fixture creates: {}", users.len());
    let armset_path = dir.join("armset.json");
    std::fs::write(
        &armset_path,
        serde_json::to_vec(&armset(&users, &[])).unwrap(),
    )
    .unwrap();
    let sock = dir.join("pp.sock");
    let relay = serve_relay(&sock, lines, stop.clone());
    let ports = [0, 1, 2, 3, 4, 5].map(|i| mocks[i].port);
    let cfg_path = dir.join("paper.toml");
    std::fs::write(&cfg_path, paper_toml(&dir, &sock, ports)).unwrap();
    if killed_before_start {
        std::fs::write(dir.join("KILL"), b"").unwrap();
    }
    let summary = dir.join("summary.json");
    let mut child = hot_cmd(&cfg_path, &armset_path, &dir.join("state.db"), 4)
        .arg("--rpc-url")
        .arg(format!("http://127.0.0.1:{}", mocks[6].port))
        .arg("--summary")
        .arg(&summary)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    let (mut seen, mut inet, mut unix) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    let mut samples = 0u64;
    let t0 = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        assert!(t0.elapsed() < Duration::from_secs(30), "rugsnipe-hot hangs");
        let s = socket_inodes(pid);
        if !s.is_empty() {
            let mut net = BTreeSet::new();
            for t in ["tcp", "tcp6", "udp", "udp6", "raw", "raw6"] {
                net.extend(table_inodes(pid, t, 9));
            }
            let ux = table_inodes(pid, "unix", 6);
            inet.extend(s.intersection(&net).copied());
            unix.extend(s.intersection(&ux).copied());
            seen.extend(s);
        }
        samples += 1;
        std::thread::sleep(Duration::from_millis(2));
    };
    stop.store(true, Release);
    let _ = relay.join();
    let mut stderr = String::new();
    std::io::Read::read_to_string(child.stderr.as_mut().unwrap(), &mut stderr).unwrap();
    assert!(status.success(), "rugsnipe-hot failed: {stderr}");
    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&summary).unwrap()).unwrap();
    let contacted = names
        .iter()
        .zip(&mocks)
        .map(|(n, m)| (*n, m.accepted.load(Acquire)))
        .collect();
    let _ = std::fs::remove_dir_all(&dir);
    Run {
        summary,
        stderr,
        samples,
        seen,
        unix,
        inet,
        contacted,
    }
}

fn assert_no_network(r: &Run) {
    assert!(r.samples > 50, "sampled {} times", r.samples);
    assert!(
        !r.unix.is_empty(),
        "sampler positive control: the relay AF_UNIX socket was seen ({:?})",
        r.seen
    );
    assert!(
        r.inet.is_empty(),
        "paper mode opened inet socket(s) {:?} (all sockets seen {:?})",
        r.inet,
        r.seen
    );
    for (n, c) in &r.contacted {
        assert_eq!(*c, 0, "{n} endpoint was contacted");
    }
}

#[test]
fn paper_binary_never_opens_an_inet_socket_nor_contacts_a_sender() {
    if !std::path::Path::new("/proc/self/net/tcp").exists() {
        eprintln!("skipped: needs Linux /proc");
        return;
    }
    let r = run("paper-no-send", false);
    assert!(
        r.stderr.contains("will not be constructed"),
        "config warns that real senders are ignored in paper: {}",
        r.stderr
    );
    assert_eq!(r.summary["mode"], "paper");
    let paper = decision(&r.summary, "paper");
    assert!(paper >= 1, "paper decisions: {}", r.summary["decisions"]);
    assert!(r.summary["paper_variants_recorded"].as_u64().unwrap() >= paper);
    assert_no_network(&r);
    eprintln!(
        "paper decisions {paper}, paper variants {}, {} samples, sockets seen {:?} (unix {:?}, inet {:?})",
        r.summary["paper_variants_recorded"], r.samples, r.seen, r.unix, r.inet
    );
}

#[test]
fn kill_file_present_at_start_stops_every_buy_of_the_binary() {
    if !std::path::Path::new("/proc/self/net/tcp").exists() {
        eprintln!("skipped: needs Linux /proc");
        return;
    }
    let r = run("paper-killed", true);
    let killed = decision(&r.summary, "skip_killed");
    assert!(killed >= 1, "decisions: {}", r.summary["decisions"]);
    assert_eq!(
        decision(&r.summary, "paper"),
        0,
        "{}",
        r.summary["decisions"]
    );
    assert_eq!(r.summary["counters"]["intents"], 0);
    assert_eq!(r.summary["paper_variants_recorded"], 0);
    assert_eq!(r.summary["cold"]["killed"], true);
    assert_eq!(r.summary["cold"]["kill_reason"], "KillFile");
    assert_no_network(&r);
}
