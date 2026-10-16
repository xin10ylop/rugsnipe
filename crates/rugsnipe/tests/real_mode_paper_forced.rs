//! A decision forced to paper inside a real mode (ArmSet gate, regime
//! freeze, operator not armed, or a free feed such as the logs path or the
//! relay) must never reach a real sender: not the
//! primary variant, not the secondary variant handed to the sender thread,
//! and not the sells of its simulated position. Its real nonce is released
//! (nothing was sent with it) and its P&L never touches the RiskGate.
//!
//! The process refuses probe/live in this build, so the hot loop is
//! assembled by hand in `Mode::Live` with every validation gate passing.
//! With `--features real-senders` a Jito sender points at a local mock
//! that counts connections; a `real` job is the positive control.

mod common;

use arc_swap::ArcSwap;
use common::*;
use pump_codec::Pubkey;
use rugsnipe::hot::{HotCfg, HotLoop, HotParts, Shared};
use rugsnipe::msgs::FireJob;
use rugsnipe::setup::{self, ArmState};
use rugsnipe::stats::HotStats;
use rugsnipe_core::armset::{ArmSet, OpState};
use rugsnipe_core::config::{ArmSetGate, Config, ValidateEnv};
use rugsnipe_core::risk::RiskGate;
use rugsnipe_core::rslog::{box_id, RslogWriter};
use rugsnipe_core::watchlist::Watchlist;
use rugsnipe_core::Mode;
use rugsnipe_feed::{FeedBuf, FeedSrc, FeedStatus, FiredSet, Payload};
use rugsnipe_tx::nonce::NoncePool;
use rugsnipe_tx::senders::{ApiKeys, SenderSet};
use rugsnipe_tx::signer::HotSigner;
use std::collections::BTreeMap;
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::*};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A local HTTP endpoint that counts the HTTP requests it receives and
/// answers 200. (A real mode warms its sender connections at start: a bare
/// TCP connect without a request is not a send, so requests are counted.)
struct Mock {
    port: u16,
    accepted: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
}

impl Mock {
    fn start() -> Mock {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.set_nonblocking(true).unwrap();
        let port = l.local_addr().unwrap().port();
        let accepted = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (a, s) = (accepted.clone(), stop.clone());
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            while !s.load(Acquire) {
                match l.accept() {
                    Ok((mut c, _)) => {
                        let a = a.clone();
                        let s = s.clone();
                        std::thread::spawn(move || {
                            let _ = c.set_read_timeout(Some(Duration::from_millis(50)));
                            let mut b = [0u8; 4096];
                            while !s.load(Acquire) {
                                match c.read(&mut b) {
                                    Ok(0) => return,
                                    Ok(n) => {
                                        if b[..n].starts_with(b"POST ") {
                                            a.fetch_add(1, AcqRel);
                                        }
                                        let _ = c.write_all(
                                            b"HTTP/1.1 200 OK\r\nContent-Length: 36\r\n\r\n{\"jsonrpc\":\"2.0\",\"result\":\"x\",\"id\":1}",
                                        );
                                    }
                                    Err(_) => {}
                                }
                            }
                        });
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        Mock {
            port,
            accepted,
            stop,
        }
    }
    fn accepted(&self) -> u64 {
        self.accepted.load(Acquire)
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.stop.store(true, Release);
    }
}

fn live_cfg(dir: &std::path::Path, _mock_port: u16) -> Config {
    let mut c = paper_cfg(dir);
    c.mode.mode = Mode::Live;
    c.tx.cu_limit = 88_000;
    c.tx.lads_bytes = 12_124_160;
    c.secrets.hot_keypair = "hot-keypair".into();
    c.secrets.probe_keypair = "probe-keypair".into();
    c.secrets.api_keys = "api-keys".into();
    c.secrets.armset_pubkey = Pubkey::new_unique().to_string();
    c.secrets.cold_wallet = Pubkey::new_unique().to_string();
    c.senders.rpcsend.enabled = false;
    #[cfg(feature = "real-senders")]
    {
        c.senders.jito.enabled = true;
        c.senders.jito.url = format!("http://127.0.0.1:{_mock_port}");
        c.senders.jito.max_rps = 100;
    }
    c
}

fn good_env(c: &Config) -> ValidateEnv {
    let mut creds = BTreeMap::new();
    for n in [
        &c.secrets.hot_keypair,
        &c.secrets.probe_keypair,
        &c.secrets.api_keys,
    ] {
        creds.insert(n.clone(), Some(0o400));
    }
    ValidateEnv {
        clock_offset_ms: Some(0.5),
        post_exec_feed_healthy: true,
        armset: ArmSetGate::Verified {
            gate_passed: true,
            probe_allowed: true,
        },
        credential_modes: creds,
        live_cli_flag: true,
        arm_live_token: true,
        armset_probe_stage: None,
    }
}

struct Rig {
    hot: HotLoop,
    fire_rx: rtrb::Consumer<FireJob>,
    senders: Arc<SenderSet>,
    nonces: Arc<NoncePool>,
    risk: Arc<RiskGate>,
    signer: Arc<HotSigner>,
    stats: Arc<HotStats>,
    _cold_rx: rtrb::Consumer<rugsnipe::msgs::ColdMsg>,
    _fills_tx: rtrb::Producer<rugsnipe::msgs::HotIn>,
    _w: RslogWriter,
}

fn rig(cfg: &Config, set: &ArmSet, verified: bool, dir: &std::path::Path) -> Rig {
    let v = cfg.validate(&good_env(cfg));
    assert!(v.ok(), "every live gate passes: {:?}", v.refusals);
    let senders = Arc::new(
        SenderSet::build(cfg, &good_env(cfg), &ApiKeys::default()).expect("live sender set"),
    );
    assert_eq!(senders.mode, cfg.mode.mode);
    assert_eq!(
        senders.real().len(),
        cfg!(feature = "real-senders") as usize,
        "the mock Jito sender is real (feature real-senders)"
    );
    let signer = Arc::new(setup::throwaway_signer().unwrap());
    let owner = signer.pubkey();
    let nonces = Arc::new(setup::paper_nonce_pool(owner, 8));
    let templates = setup::templates(cfg, owner, &nonces).unwrap();
    let policy = setup::exit_policy(cfg).unwrap();
    let watch = Arc::new(Watchlist::new());
    watch.load_armset(set);
    let arm = Arc::new(ArcSwap::from_pointee(ArmState::from_armset(
        set, verified, "",
    )));
    let risk = Arc::new(RiskGate::new(cfg.risk.clone()));
    risk.set_hot_balance(2_000_000_000);
    let (rslog, w) = RslogWriter::spawn(dir.join("rslog"), "t-hot", box_id("t"), 1 << 12).unwrap();
    let (cold_tx, cold_rx) = rtrb::RingBuffer::new(4_096);
    let (fills_tx, fills_rx) = rtrb::RingBuffer::new(64);
    let (fire_tx, fire_rx) = rtrb::RingBuffer::new(64);
    let mut reg = prometheus_client::registry::Registry::default();
    let stats = Arc::new(HotStats::new(&mut reg));
    let mut hcfg = HotCfg::from_config(cfg, policy);
    hcfg.fill_timeout_slots = 3;
    let hot = HotLoop::new(HotParts {
        cfg: hcfg,
        watch,
        arm,
        risk: risk.clone(),
        fired: Arc::new(FiredSet::with_capacity(1 << 10)),
        nonces: nonces.clone(),
        signer: signer.clone(),
        templates,
        senders: senders.clone(),
        rslog,
        cold: cold_tx,
        fills_in: fills_rx,
        fire: fire_tx,
        stats: stats.clone(),
        shared: Arc::new(Shared::default()),
    });
    Rig {
        hot,
        fire_rx,
        senders,
        nonces,
        risk,
        signer,
        stats,
        _cold_rx: cold_rx,
        _fills_tx: fills_tx,
        _w: w,
    }
}

fn drain(hot: &mut HotLoop, feed: &mut VecFeed) {
    let mut buf = FeedBuf::with_capacity(64, 1 << 16);
    let t0 = Instant::now();
    while hot.step(feed, &mut buf) != FeedStatus::Ended {
        assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", hot.counters);
    }
    // a few idle ticks for the exit timers
    for _ in 0..3 {
        hot.step(feed, &mut buf);
        std::thread::sleep(Duration::from_millis(110));
    }
}

/// Run the sender thread over whatever the hot loop handed it; returns the
/// number of (sender, result) pairs it produced.
fn run_sender(r: Rig, dir: &std::path::Path) -> (u64, Arc<SenderSet>) {
    let Rig {
        hot,
        fire_rx,
        senders,
        signer,
        stats,
        ..
    } = r;
    drop(hot); // abandons the fire ring: the sender exits once it is empty
    let (rs, w) =
        RslogWriter::spawn(dir.join("rslog-send"), "t-send", box_id("t"), 1 << 10).unwrap();
    let (cold_tx, _cold_rx) = std::sync::mpsc::channel();
    let h = rugsnipe::sender::spawn(
        fire_rx,
        senders.clone(),
        signer,
        rs,
        cold_tx,
        stats,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    let n = h.join().unwrap();
    drop(w);
    (n, senders)
}

/// `ev` with an explicit feed source (the class decides paper-only feeds).
fn ev_from(src: FeedSrc, slot: u64, payload: Payload) -> rugsnipe_feed::FeedEvent {
    rugsnipe_feed::FeedEvent {
        src,
        ..ev(slot, payload)
    }
}

#[test]
fn live_mode_paper_decisions_never_reach_a_real_sender() {
    // (case, feed source, ArmSet verified, regime frozen, operator state, fires)
    let cases = [
        (
            "armset_unverified",
            FeedSrc::Grpc,
            false,
            false,
            OpState::Armed,
            false,
        ),
        (
            "regime_frozen",
            FeedSrc::Grpc,
            true,
            true,
            OpState::Armed,
            false,
        ),
        (
            "op_not_armed",
            FeedSrc::Grpc,
            true,
            false,
            OpState::Shadow,
            false,
        ),
        // free feeds (logs path / relay) are paper only whatever the gates say
        (
            "free_feed_logsws",
            FeedSrc::LogsWs,
            true,
            false,
            OpState::Armed,
            false,
        ),
        (
            "free_feed_relay",
            FeedSrc::Relay,
            true,
            false,
            OpState::Armed,
            false,
        ),
        // positive control: a live-class feed with every gate open fires for real
        (
            "live_class_fire",
            FeedSrc::Grpc,
            true,
            false,
            OpState::Armed,
            true,
        ),
    ];
    // live needs a gate-passed ArmSet: only the case's own reason may hold it back
    let cases = cases.map(|(case, src, verified, frozen, op_state, fires)| {
        (
            case,
            src,
            Mode::Live,
            verified,
            frozen,
            op_state,
            (true, false),
            fires,
        )
    });
    run_cases(&cases);
}

/// The signed ArmSet flags at run time (F3): live without `gate_passed` and
/// probe without `probe_allowed` are forced to paper in the hot loop itself
/// (not only by startup validation, e.g. after an ArmSet reload); probe with
/// `probe_allowed` fires without `gate_passed`.
#[test]
fn armset_flags_gate_probe_and_live_in_the_hot_loop() {
    let cases = [
        // (case, src, mode, verified, frozen, op state, (gate_passed, probe_allowed), fires)
        (
            "live_without_gate_passed",
            FeedSrc::Grpc,
            Mode::Live,
            true,
            false,
            OpState::Armed,
            (false, true),
            false,
        ),
        (
            "probe_without_probe_allowed",
            FeedSrc::Grpc,
            Mode::Probe,
            true,
            false,
            OpState::Armed,
            (true, false),
            false,
        ),
        (
            "probe_allowed_fires_without_the_gate",
            FeedSrc::Grpc,
            Mode::Probe,
            true,
            false,
            OpState::Armed,
            (false, true),
            true,
        ),
    ];
    run_cases(&cases);
}

type Case = (
    &'static str,
    FeedSrc,
    Mode,
    bool,
    bool,
    OpState,
    (bool, bool),
    bool,
);

fn run_cases(cases: &[Case]) {
    for &(case, src, mode, verified, frozen, op_state, (gate_passed, probe_allowed), fires) in cases
    {
        let mock = Mock::start();
        let dir = tmpdir(&format!("rmpf-{case}"));
        let mut cfg = live_cfg(&dir, mock.port);
        cfg.mode.mode = mode;
        if frozen {
            cfg.mode.regime_freeze_slots = vec![[900, 2_000]];
        }
        let dev = Pubkey::new_unique();
        let mut set = armset(&[dev], &[]);
        set.gate_passed = gate_passed;
        set.probe_allowed = probe_allowed;
        set.operators[0].state = op_state;
        if mode == Mode::Probe {
            // within max_probe_lamports
            set.operators[0].size_lamports = cfg.risk.max_probe_lamports;
        }
        let mut r = rig(&cfg, &set, verified, &dir);
        // create (1 SOL dev buy) -> trades -> deadline fill -> the dev dumps -> sell + close
        let a = Pubkey::new_unique();
        let (hit, mut curve) = create(a, dev, 1000, 1, 1_000_000_000);
        let (x, y) = (Pubkey::new_unique(), Pubkey::new_unique());
        let t1 = trade(&mut curve, a, x, 1001, 10, 500_000_000, 0);
        let t2 = trade(&mut curve, a, y, 1002, 11, 200_000_000, 0);
        let (other, _) = create(Pubkey::new_unique(), Pubkey::new_unique(), 1004, 5, 0);
        let dump = trade(&mut curve, a, dev, 1005, 12, 0, hit.intx_tokens_out / 3);
        let after = trade(&mut curve, a, Pubkey::new_unique(), 1006, 13, 10_000_000, 0);
        // the dump sell (sent in 1005) fills at the end of slot 1006
        // (paper.exit_latency_slots = 1): the first event of a later slot closes it
        let later = trade(&mut curve, a, Pubkey::new_unique(), 1008, 14, 10_000_000, 0);
        let mut feed = VecFeed::new(vec![
            ev_from(src, 1000, Payload::Hit(hit)),
            ev_from(src, 1001, Payload::Trade(t1)),
            ev_from(src, 1002, Payload::Trade(t2)),
            ev_from(src, 1004, Payload::Hit(other)),
            ev_from(src, 1005, Payload::Trade(dump)),
            ev_from(src, 1006, Payload::Trade(after)),
            ev_from(src, 1008, Payload::Trade(later)),
        ]);
        drain(&mut r.hot, &mut feed);
        let c = r.hot.counters;
        assert_eq!(
            c.intents, 1,
            "{case}: the decision ran to the send step {c:?}"
        );
        if fires {
            // primary + secondary, both real; the nonce stays claimed; caps reserved;
            // no simulated fill (a real position opens on its own TradeEvent only),
            // so no sell job either
            assert_eq!(c.fills, 0, "{case}: {c:?}");
            assert_eq!(r.fire_rx.slots(), 2, "{case}");
            assert!(r.fire_rx.peek().unwrap().real, "{case}");
            assert_eq!(r.nonces.free_count(), 7, "{case}: nonce in flight");
            assert_eq!(r.risk.open_positions(), 1, "{case}");
            assert_eq!(
                r.senders.paper.total(),
                0,
                "{case}: nothing recorded as paper"
            );
            let (pairs, _) = run_sender(r, &dir);
            // both variants recorded as paper copies; Jito takes the
            // pay-on-success primary only (1 req/s per IP and region)
            assert_eq!(pairs, 2 + cfg!(feature = "real-senders") as u64, "{case}");
            if cfg!(feature = "real-senders") {
                let t0 = Instant::now();
                while mock.accepted() == 0 && t0.elapsed() < Duration::from_secs(3) {
                    std::thread::sleep(Duration::from_millis(10));
                }
                assert!(mock.accepted() >= 1, "{case}: the mock saw the real sends");
            }
            let _ = std::fs::remove_dir_all(&dir);
            continue;
        }
        assert_eq!(c.fills, 1, "{case}: the paper position filled {c:?}");
        assert!(
            c.sells >= 1 && c.closed == 1,
            "{case}: dump sell + close {c:?}"
        );
        // the only job handed to the sender thread is the paper secondary variant
        assert_eq!(r.fire_rx.slots(), 1, "{case}: no real buy or sell job");
        let job = r.fire_rx.peek().unwrap();
        assert!(
            !job.real && job.idx == 1,
            "{case}: secondary variant is paper"
        );
        // the real nonce went back to Free (nothing was sent with it)
        assert_eq!(r.nonces.free_count(), 8, "{case}: nonce released");
        // the RiskGate never saw the paper position
        assert_eq!(r.risk.open_positions(), 0, "{case}");
        assert_eq!(r.risk.realized_today(rugsnipe::now_unix()), 0, "{case}");
        assert_eq!(r.risk.op_consec_losses(0), 0, "{case}");
        let paper_before = r.senders.paper.total();
        assert!(paper_before >= 2, "{case}: primary buy + sell recorded");
        let (pairs, senders) = run_sender(r, &dir);
        assert_eq!(
            pairs, 1,
            "{case}: the secondary went to the paper recorder only"
        );
        assert_eq!(senders.paper.total(), paper_before + 1, "{case}");
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            mock.accepted(),
            0,
            "{case}: the real sender was never contacted"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Positive control: a `real` job does reach the (mock) real sender, so
/// the zero above is the gate, not a deaf mock.
#[cfg(feature = "real-senders")]
#[test]
fn a_real_job_reaches_the_mock_sender() {
    let mock = Mock::start();
    let dir = tmpdir("rmpf-control");
    let cfg = live_cfg(&dir, mock.port);
    let dev = Pubkey::new_unique();
    let set = armset(&[dev], &[]);
    let r = rig(&cfg, &set, true, &dir);
    let owner = r.signer.pubkey();
    let pool = setup::paper_nonce_pool(owner, 1);
    let t = setup::templates(&cfg, owner, &pool).unwrap();
    let (tx, rx) = rtrb::RingBuffer::new(4);
    let mut tx = tx;
    tx.push(FireJob {
        tmpl: t.buys[0].clone(),
        intent: 1,
        idx: 0,
        prio: 1_000,
        tip: 1_000_000,
        tip_account: t.tip_account,
        bid: 1_000_000,
        real: true,
    })
    .unwrap();
    drop(tx);
    let Rig {
        hot,
        fire_rx,
        senders,
        signer,
        stats,
        ..
    } = r;
    drop((hot, fire_rx));
    let (rs, _w) = RslogWriter::spawn(dir.join("rslog-send"), "t-send", box_id("t"), 64).unwrap();
    let (cold_tx, _cold_rx) = std::sync::mpsc::channel();
    let n = rugsnipe::sender::spawn(rx, senders, signer, rs, cold_tx, stats, Arc::default())
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(n, 2, "paper + jito");
    let t0 = Instant::now();
    while mock.accepted() == 0 && t0.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(mock.accepted(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}
