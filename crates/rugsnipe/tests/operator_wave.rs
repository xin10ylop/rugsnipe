//! The operator-wave rule set in the hot loop (docs/09-operator-wave.md):
//!
//! * the operator table (`entry.operators_file`) sets each listed create
//!   signer's exit to `create + hold_slots`; unlisted creates keep
//!   `[exit.default] hold_to_create_slots` (paper E1, through the real hot
//!   loop, cold thread and state.db; every other exit rule off);
//! * `entry.operators_only` skips an unlisted signer (`skip_not_operator`);
//! * at the real fire point the park's bundle-wallet count is checked
//!   against `entry.bundle_wallets` (`skip_bundle_shape`), and a zero
//!   `entry.max_premium` binds the buy at the exact post-bundle quote.

mod common;

use arc_swap::ArcSwap;
use common::*;
use pump_codec::curve::{CurveState, FeeBps};
use pump_codec::Pubkey;
use rugsnipe::hot::{entry_v_cap, HotCfg, HotLoop, HotParts, Shared};
use rugsnipe::msgs::{FireJob, HotIn, ShadowInfo};
use rugsnipe::operators::{create_signer, OperatorTable};
use rugsnipe::setup::{self, min_tokens_out, min_tokens_out_at, ArmState};
use rugsnipe::stats::{code, decision_label, HotStats};
use rugsnipe_core::config::{ArmSetGate, Config, ValidateEnv};
use rugsnipe_core::risk::RiskGate;
use rugsnipe_core::rslog::{box_id, Rec, RslogWriter, ShadowOutcome};
use rugsnipe_core::watchlist::Watchlist;
use rugsnipe_core::{IntentId, Mode};
use rugsnipe_exit::{ExitReason, Trade, Venue};
use rugsnipe_feed::{FeedBuf, FeedMux, FeedSrc, FeedStatus, FiredSet, Payload};
use rugsnipe_tx::senders::{ApiKeys, SenderSet};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

const S0: u64 = 1_000;

/// `[exit.default]` of the operator wave: every LEAD rule off, the
/// unlisted-creator default hold at create + 6.
const WAVE_EXIT: &str = r#"
[exit.default]
tp = []
tp_slip_bps = 2500
trail = { activate = 0.0, retrace_bps = 0 }
phantom_sl = { after_ms = 0, drop = 0.0 }
no_activity_ms = 0
max_hold_ms = 0
max_hold_slots = 0
bundle_sell_min_lamports = 0
insider_sell = "off"
consolidation_pct = 0
big_buy_lamports = [0, 0]
sell_retry = { every_slots = 2, max_tries = 15, tip_mult = 2.0 }
hold_to_create_slots = 6
"#;

fn ops_file(dir: &std::path::Path, rows: &[(Pubkey, u16)]) -> std::path::PathBuf {
    let m: serde_json::Map<String, serde_json::Value> = rows
        .iter()
        .map(|(k, h)| {
            (
                k.to_string(),
                serde_json::json!({"hold_slots": h, "n": 5, "wins": 3, "pnl_sol": 0.3, "dev_sol": 4.03, "bundle_wallets": 3}),
            )
        })
        .collect();
    let p = dir.join("operators.json");
    std::fs::write(&p, serde_json::Value::Object(m).to_string()).unwrap();
    p
}

fn wave_cfg(dir: &std::path::Path, ops: &std::path::Path, only: bool) -> Config {
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/paper-lead.example.toml"
    );
    let lead = setup::load_config(std::path::Path::new(p)).unwrap();
    let mut cfg = paper_cfg(dir);
    cfg.paper = lead.paper.clone();
    cfg.paper.entry_premium = 0.0;
    cfg.exit = Config::from_toml_str(WAVE_EXIT).unwrap().exit;
    cfg.entry.max_premium = 0.0;
    cfg.entry.operators_file = ops.display().to_string();
    cfg.entry.operators_only = only;
    let v = cfg.validate(&ValidateEnv::default());
    assert!(v.ok(), "{:?}", v.refusals);
    cfg
}

fn decide_all(app: &mut rugsnipe::app::App, events: Vec<rugsnipe_feed::FeedEvent>, n: u64) {
    let mut feed = FeedMux::new(vec![Box::new(VecFeed::new(events))]);
    let mut buf = FeedBuf::with_capacity(64, 1 << 16);
    let t0 = Instant::now();
    while app.hot.counters.intents < n {
        if app.hot.step(&mut feed, &mut buf) == FeedStatus::Ended && app.hot.counters.intents < n {
            panic!("feed ended with {:?}", app.hot.counters);
        }
        assert!(t0.elapsed() < Duration::from_secs(10));
    }
}

/// A historical trade (reserves without us).
fn hist(c: &mut CurveState, mint: Pubkey, user: Pubkey, slot: u64, sol: u64, sell: u64) -> Trade {
    let t = trade(c, mint, user, slot, slot * 100 + sol % 97, sol, sell);
    Trade {
        venue: Venue::Curve,
        mint,
        user,
        is_buy: t.is_buy,
        sol_amount: t.sol_amount,
        token_amount: t.token_amount,
        fee_lamports: t.fee,
        reserve_sol: t.virtual_sol_reserves,
        reserve_token: t.virtual_token_reserves,
        slot,
    }
}

fn info(c: &CurveState) -> ShadowInfo {
    ShadowInfo {
        create_slot: S0,
        create_index: Some(100),
        block_txs: Some(1_000),
        bundle_trades: 2,
        bundle_wallets: 1,
        our_index: Some(102),
        tp_ahead: 0,
        tp_behind: 0,
        s0_failed: 0,
        premium_bps: 0,
        post_bundle_v: c.virtual_sol,
        post_bundle_t: c.virtual_token,
        post_bundle_mcap: 0,
    }
}

/// A listed operator (hold 10) and an unlisted creator (default 6) through
/// the paper E1 hot loop with every other exit rule off: each sells all at
/// its create + H (the decision slot; the paper sell fills one slot later),
/// and nothing sells before, not the dev dumping at +2 nor a +30% pump.
#[test]
fn hold_to_create_exits_each_launch_at_its_operators_slot() {
    let dir = tmpdir("wave-hold");
    let (dev_a, dev_b) = (Pubkey::new_unique(), Pubkey::new_unique());
    let ops = ops_file(&dir, &[(dev_a, 10)]);
    let cfg = wave_cfg(&dir, &ops, false);
    let mut app = build(cfg, armset(&[dev_a, dev_b], &[]), &dir, 5_000, false);
    let (m1, m2) = (Pubkey::new_unique(), Pubkey::new_unique());
    let (h1, mut c1) = create(m1, dev_a, S0, 1, 1_000_000_000);
    let (h2, mut c2) = create(m2, dev_b, S0, 2, 1_000_000_000);
    decide_all(
        &mut app,
        vec![ev(S0, Payload::Hit(h1)), ev(S0, Payload::Hit(h2))],
        2,
    );
    let owner = app.owner;
    // the per-position policies: the listed signer's hold, the default for the other
    let p1 = app.hot.position_of(&m1).unwrap();
    let p2 = app.hot.position_of(&m2).unwrap();
    assert_eq!(
        (p1.policy.hold_to_create_slot, p1.create_slot),
        (Some(10), S0)
    );
    assert_eq!(
        (p2.policy.hold_to_create_slot, p2.create_slot),
        (Some(6), S0)
    );
    assert_eq!(
        (p1.hold_to_slot(), p2.hold_to_slot()),
        (Some(S0 + 10), Some(S0 + 6))
    );
    for (m, c, dev) in [(m1, &mut c1, dev_a), (m2, &mut c2, dev_b)] {
        hist(c, m, Pubkey::new_unique(), S0, 700_000_000, 0);
        let intent = IntentId::new(&m, &owner).0;
        // zero premium: we fill at the exact post-bundle quote (our own
        // impact is in the quote, not counted as premium)
        app.hot.inject(HotIn::E1Open {
            intent,
            mint: m,
            outcome: ShadowOutcome::Filled,
            info: info(c),
        });
        app.hot.inject(HotIn::E1Insider {
            intent,
            key: dev,
            creator: true,
        });
        for s in S0 + 1..=S0 + 12 {
            let t = match s - S0 {
                2 => hist(c, m, dev, s, 0, 20_000_000_000_000), // the dev dumps
                3 => hist(c, m, Pubkey::new_unique(), s, 9_000_000_000, 0), // a pump
                _ => hist(c, m, Pubkey::new_unique(), s, 20_000_000, 0),
            };
            app.hot.inject(HotIn::E1Trade { intent, trade: t });
        }
        app.hot.inject(HotIn::E1End {
            intent,
            horizon: S0 + 14,
        });
    }
    assert_eq!(app.hot.counters.fills, 2, "{:?}", app.hot.counters);
    assert_eq!(app.hot.counters.closed, 2, "{:?}", app.hot.counters);
    std::thread::sleep(Duration::from_millis(200));
    let summary = app.shutdown(vec![]);
    let conn = rusqlite::Connection::open(dir.join("state.db")).unwrap();
    let exits_of = |m: Pubkey| -> Vec<(String, i64)> {
        let mut st = conn
            .prepare("SELECT reason, slot FROM exits WHERE mint = ?1")
            .unwrap();
        st.query_map([m.to_string()], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    // decided at create + H, filled at the end of the next slot
    assert_eq!(
        exits_of(m1),
        vec![("hold_to".to_string(), (S0 + 11) as i64)]
    );
    assert_eq!(exits_of(m2), vec![("hold_to".to_string(), (S0 + 7) as i64)]);
    assert_eq!(summary.cold.paper.exits_by_reason.get("hold_to"), Some(&2));
    let fired: Vec<u8> = rslog_records(&dir.join("rslog"))
        .iter()
        .filter_map(|r| match r {
            Rec::ExitFired { reason, .. } => Some(*reason),
            _ => None,
        })
        .collect();
    // every sell sent is a hold_to (reason code 9); the paper fill model
    // also records the retry that falls due in the fill slot
    assert!(
        fired.len() >= 2 && fired.iter().all(|r| *r == 9),
        "{fired:?}"
    );
    assert_eq!(rugsnipe::msgs::reason_code(ExitReason::HoldTo), 9);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `entry.operators_only`: a create whose signer is not in the table is
/// skipped at the decision (`skip_not_operator`); a listed one goes on.
#[test]
fn operators_only_skips_an_unlisted_signer_at_the_decision() {
    let dir = tmpdir("wave-only");
    let (dev_a, dev_b) = (Pubkey::new_unique(), Pubkey::new_unique());
    let ops = ops_file(&dir, &[(dev_a, 10)]);
    let cfg = wave_cfg(&dir, &ops, true);
    let mut app = build(cfg, armset(&[dev_a, dev_b], &[]), &dir, 5_000, false);
    let (m1, m2) = (Pubkey::new_unique(), Pubkey::new_unique());
    let (h1, _) = create(m1, dev_a, S0, 1, 1_000_000_000);
    let (h2, _) = create(m2, dev_b, S0, 2, 1_000_000_000);
    // the unlisted create first: once the listed one decided, both were seen
    decide_all(
        &mut app,
        vec![ev(S0, Payload::Hit(h2)), ev(S0, Payload::Hit(h1))],
        1,
    );
    assert!(app.hot.position_of(&m1).is_some());
    assert!(app.hot.position_of(&m2).is_none());
    let owner = app.owner;
    let _ = app.shutdown(vec![]);
    let codes = |m: Pubkey| -> Vec<u8> {
        let i = IntentId::new(&m, &owner).0;
        rslog_records(&dir.join("rslog"))
            .iter()
            .filter_map(|r| match r {
                Rec::Decision { intent, code, .. } if *intent == i => Some(*code),
                _ => None,
            })
            .collect()
    };
    assert_eq!(codes(m2), vec![code::NOT_OPERATOR]);
    assert_eq!(codes(m1).len(), 1);
    assert_ne!(codes(m1)[0], code::NOT_OPERATOR);
    assert_eq!(decision_label(code::NOT_OPERATOR), "skip_not_operator");
    assert_eq!(decision_label(code::BUNDLE_SHAPE), "skip_bundle_shape");
    assert_eq!((code::NOT_OPERATOR, code::BUNDLE_SHAPE), (0x54, 0x55));
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- the real fire point (a hand-assembled live hot loop) ----

fn live_cfg(dir: &std::path::Path) -> Config {
    let mut c = paper_cfg(dir);
    c.mode.mode = Mode::Live;
    c.tx.cu_limit = 65_000;
    c.tx.lads_bytes = 12_124_160;
    c.feeds.grpc.enabled = true;
    c.feeds.grpc.url = "https://grpc.invalid".into();
    c.feeds.grpc.bundle_run = true;
    c.secrets.hot_keypair = "hot-keypair".into();
    c.secrets.probe_keypair = "probe-keypair".into();
    c.secrets.api_keys = "api-keys".into();
    c.secrets.armset_pubkey = Pubkey::new_unique().to_string();
    c.secrets.cold_wallet = Pubkey::new_unique().to_string();
    c.senders.rpcsend.enabled = false;
    c.entry.require_bundle = true;
    // a large wait: the park fires on the run's last index (create + 5)
    c.entry.bundle_wait_ms = 200;
    c.entry.max_premium = 0.0;
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

/// A recorded mainnet create (gRPC copy, index 40) parked in live mode; the
/// bundle run is three distinct wallets buying at 41-43 and the first of
/// them selling at 45 (the run's last index: the park fires there).
/// * `bundle_wallets = [3]`, the signer listed with hold 3: sent, bound at
///   the exact post-bundle quote (`max_premium = 0`), the position's exit at
///   create + 3;
/// * `bundle_wallets = [2, 4]`: `skip_bundle_shape`, nothing sent, no nonce,
///   the risk reservation never taken;
/// * `operators_only` with the signer not listed: `skip_not_operator`
///   before the park.
#[test]
fn bundle_shape_operator_gate_and_zero_premium_bound_at_the_real_fire_point() {
    const SLOT: u64 = 5_000;
    const CI: u32 = 40;
    let fx = codec_fixture("fresh_v0alt_create_v2_buy18");
    let (tx, meta) = fixture_wire(&fx, CI);
    let hit = {
        use rugsnipe_feed::wiremeta::{WireMeta, EMPTY_INNER, MAX_INNER};
        let m = WireMeta::parse(&meta).unwrap();
        let mut loaded = vec![Pubkey::default(); 256];
        let mut inner = [EMPTY_INNER; MAX_INNER];
        let (nl, ni) = (m.loaded_into(&mut loaded), m.inner_into(&mut inner));
        let view = pump_codec::tx::TxView::parse(&tx).unwrap();
        pump_codec::create_hit(&view, Some(&loaded[..nl]), &inner[..ni], FeeBps::DEFAULT)
            .unwrap()
            .unwrap()
    };
    let signer = create_signer(&hit);
    assert_eq!(
        Some(signer),
        hit.fee_payer,
        "the wire path keys on the fee payer"
    );
    assert_eq!(signer, hit.signers.as_slice()[0], "= the first signer");
    // (bundle_wallets, signer listed, operators_only) -> expected decision
    for (shape, listed, only, want) in [
        (vec![3u32], true, false, code::FIRE),
        (vec![2, 4], true, false, code::BUNDLE_SHAPE),
        (vec![], false, true, code::NOT_OPERATOR),
    ] {
        let dir = tmpdir("wave-real");
        let mut cfg = live_cfg(&dir);
        cfg.entry.bundle_wallets = shape.clone();
        cfg.entry.operators_only = only;
        cfg.entry.operators_file = "/var/lib/rugsnipe/data/operators/ahw-2026-10-23.json".into();
        let v = cfg.validate(&good_env(&cfg));
        assert!(v.ok(), "{:?}", v.refusals);
        let senders =
            Arc::new(SenderSet::build(&cfg, &good_env(&cfg), &ApiKeys::default()).unwrap());
        let wallet = Arc::new(setup::throwaway_signer().unwrap());
        let owner = wallet.pubkey();
        let nonces = Arc::new(setup::paper_nonce_pool(owner, 8));
        let templates = setup::templates(&cfg, owner, &nonces).unwrap();
        let mut set = armset(&[hit.user], &[]);
        set.gate_passed = true;
        let watch = Arc::new(Watchlist::new());
        watch.load_armset(&set);
        let arm = Arc::new(ArcSwap::from_pointee(ArmState::from_armset(&set, true, "")));
        let risk = Arc::new(RiskGate::new(cfg.risk.clone()));
        risk.set_hot_balance(2_000_000_000);
        let (rslog, w) =
            RslogWriter::spawn(dir.join("rslog"), "t-hot", box_id("t"), 1 << 12).unwrap();
        let (cold_tx, _cold_rx) = rtrb::RingBuffer::new(4_096);
        let (_fills_tx, fills_rx) = rtrb::RingBuffer::new(64);
        let (fire_tx, mut fire_rx) = rtrb::RingBuffer::<FireJob>::new(64);
        let mut reg = prometheus_client::registry::Registry::default();
        let stats = Arc::new(HotStats::new(&mut reg));
        let mut hc = HotCfg::from_config(&cfg, setup::exit_policy(&cfg).unwrap());
        assert_eq!(hc.max_premium_bps, 0);
        let listed = if listed { signer } else { Pubkey::new_unique() };
        hc.operators = Some(Arc::new(
            OperatorTable::from_json(&format!(r#"{{"{listed}": {{"hold_slots": 3}}}}"#)).unwrap(),
        ));
        let mut hot = HotLoop::new(HotParts {
            cfg: hc,
            watch,
            arm,
            risk: risk.clone(),
            fired: Arc::new(FiredSet::with_capacity(1 << 10)),
            nonces: nonces.clone(),
            signer: wallet,
            templates,
            senders,
            rslog,
            cold: cold_tx,
            fills_in: fills_rx,
            fire: fire_tx,
            stats,
            shared: Arc::new(Shared::default()),
            leaders: rugsnipe::leader::empty_snapshot(),
            dumpers: rugsnipe::dumpers::snapshot_from(vec![]),
        });
        // the run on the create's own state: three bundle buyers, then a
        // sell by the first at the run's last index
        let (v0, t0) = rugsnipe::convert::v0_of(&hit);
        let mut c = CurveState {
            virtual_sol: v0,
            virtual_token: t0,
            real_sol: 0,
            real_token: u64::MAX,
            complete: false,
            creator: hit.creator_arg,
        };
        let ws = [
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        ];
        let mut run = vec![];
        for (k, (u, sol, sell)) in [
            (ws[0], 1_000_000_000u64, 0u64),
            (ws[1], 500_000_000, 0),
            (ws[2], 300_000_000, 0),
            (ws[0], 0, 1_000_000_000_000),
        ]
        .into_iter()
        .enumerate()
        {
            let mut t = trade(&mut c, hit.mint, u, SLOT, 90 + k as u64, sol, sell);
            t.index = if k < 3 { CI + 1 + k as u32 } else { CI + 5 };
            run.push(rugsnipe_feed::FeedEvent {
                src: FeedSrc::Shreds,
                ..ev(SLOT, Payload::Trade(t))
            });
        }
        let (v_pb, t_pb) = (c.virtual_sol, c.virtual_token);
        let mut feed = FeedMux::new(vec![
            Box::new(WireFeed::new(vec![(SLOT, tx.clone(), meta.clone())])),
            Box::new(VecFeed::new(run)),
        ]);
        let mut buf = FeedBuf::with_capacity(64, 1 << 16);
        let t_start = Instant::now();
        while hot.step(&mut feed, &mut buf) != FeedStatus::Ended {
            assert!(t_start.elapsed() < Duration::from_secs(10));
        }
        let cn = hot.counters;
        let sent = want == code::FIRE;
        assert_eq!(
            cn.parked,
            (want != code::NOT_OPERATOR) as u64,
            "{shape:?}: {cn:?}"
        );
        assert_eq!(cn.intents, sent as u64, "{shape:?}: {cn:?}");
        let intent = IntentId::new(&hit.mint, &owner).0;
        let jobs: Vec<u64> = std::iter::from_fn(|| fire_rx.pop().ok())
            .map(|j| j.intent)
            .collect();
        assert_eq!(jobs.contains(&intent), sent, "{shape:?}");
        assert_eq!(nonces.free_count(), if sent { 7 } else { 8 }, "{shape:?}");
        assert_eq!(risk.open_positions(), sent as u64, "{shape:?}");
        if sent {
            let p = hot.position_of(&hit.mint).unwrap();
            assert_eq!(p.policy.hold_to_create_slot, Some(3), "the operator's hold");
            assert_eq!(p.hold_to_slot(), Some(SLOT + 3));
        }
        drop(hot);
        w.join().unwrap();
        let decs: Vec<(u8, u64, u64)> = rslog_records(&dir.join("rslog"))
            .iter()
            .filter_map(|r| match r {
                Rec::Decision {
                    intent: i,
                    code,
                    size,
                    min_tokens,
                    ..
                } if *i == intent => Some((*code, *size, *min_tokens)),
                _ => None,
            })
            .collect();
        assert_eq!(decs.len(), 1, "{shape:?}: {decs:?}");
        assert_eq!(decs[0].0, want, "{shape:?}");
        if sent {
            // max_premium = 0: min_tokens_out is the exact quote at the
            // observed post-bundle state, before our own trade
            let (_, spend, min_out) = decs[0];
            let exact = min_tokens_out_at(spend, v_pb, t_pb, hit.creator_arg, FeeBps::DEFAULT);
            assert_eq!(min_out, exact);
            let at = |v: u64, t: u64| {
                CurveState {
                    virtual_sol: v,
                    virtual_token: t,
                    real_sol: 0,
                    real_token: u64::MAX,
                    complete: false,
                    creator: hit.creator_arg,
                }
                .quote_buy_exact_sol_in(spend, FeeBps::DEFAULT)
                .unwrap()
                .tokens
            };
            assert!(at(v_pb, t_pb) >= min_out, "first behind the bundle: lands");
            let mut ahead = c;
            let tok = ahead.tokens_for_sol(10_000_000);
            ahead.apply_buy(tok, 10_000_000);
            assert!(
                at(ahead.virtual_sol, ahead.virtual_token) < min_out,
                "a 0.01 SOL buy ahead of ours fails it"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// `entry.max_premium = 0` on the paths without an observed run: `V_cap` is
/// the expected post-bundle reserve itself (no division, no refusal of our
/// own fill), and the bound still lands at that state.
#[test]
fn zero_premium_cap_is_the_post_bundle_reserve() {
    assert_eq!(entry_v_cap(31_000_000_000, 0), 31_000_000_000);
    assert_eq!(entry_v_cap(0, 0), 0);
    let dir = tmpdir("wave-zero");
    let mut cfg = paper_cfg(&dir);
    cfg.entry.max_premium = 0.0;
    cfg.entry.bundle_lamports = 2_000_000_000;
    assert!(cfg.validate(&ValidateEnv::default()).ok());
    let hc = HotCfg::from_config(&cfg, setup::exit_policy(&cfg).unwrap());
    assert_eq!(hc.max_premium_bps, 0);
    assert_eq!(hc.v_cap(30_000_000_000, 40_000_000_000), 32_000_000_000);
    // a fresh curve after a 2 SOL bundle: our buy at that state passes the
    // K-based bound at V0 + bundle_lamports, one more buy ahead fails it
    let dev = Pubkey::new_unique();
    let (_, c0) = create(Pubkey::new_unique(), dev, 1, 1, 0);
    let mut c = c0;
    let tok = c.tokens_for_sol(2_000_000_000);
    c.apply_buy(tok, 2_000_000_000);
    let min_out = min_tokens_out(
        100_000_000,
        hc.v_cap(c0.virtual_sol, 40_000_000_000),
        dev,
        FeeBps::DEFAULT,
    );
    let q = c
        .quote_buy_exact_sol_in(100_000_000, FeeBps::DEFAULT)
        .unwrap();
    assert!(q.tokens >= min_out, "{} < {min_out}", q.tokens);
    let tok = c.tokens_for_sol(50_000_000);
    c.apply_buy(tok, 50_000_000);
    let q = c
        .quote_buy_exact_sol_in(100_000_000, FeeBps::DEFAULT)
        .unwrap();
    assert!(q.tokens < min_out);
    // nothing to buy on an empty state
    assert_eq!(min_tokens_out_at(1, 0, 0, dev, FeeBps::DEFAULT), u64::MAX);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `config/probe-a.example.toml` parses as shipped (the wave off) and with
/// its commented operator-wave lines switched on.
#[test]
fn probe_a_example_parses_with_the_wave_lines_on_and_off() {
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/probe-a.example.toml"
    );
    let text = std::fs::read_to_string(p).unwrap();
    let off = Config::from_toml_str(&text).unwrap();
    assert!(off.entry.operators_file.is_empty() && !off.entry.operators_only);
    assert!(off.entry.bundle_wallets.is_empty());
    assert_eq!(setup::exit_policy(&off).unwrap().hold_to_create_slot, None);
    let on: String = text
        .lines()
        .map(|l| {
            for k in [
                "operators_file",
                "operators_only",
                "bundle_wallets",
                "hold_to_create_slots",
            ] {
                if let Some(rest) = l.strip_prefix("# ") {
                    if rest.starts_with(&format!("{k} = ")) {
                        return rest.to_string();
                    }
                }
            }
            l.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let on = Config::from_toml_str(&on).unwrap();
    assert_eq!(
        on.entry.operators_file,
        "/var/lib/rugsnipe/data/operators/ahw-2026-10-23.json"
    );
    assert!(on.entry.operators_only);
    assert_eq!(on.entry.bundle_wallets, vec![3]);
    assert_eq!(
        setup::exit_policy(&on).unwrap().hold_to_create_slot,
        Some(6)
    );
}
