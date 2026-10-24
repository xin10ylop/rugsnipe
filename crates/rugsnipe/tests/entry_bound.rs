//! The real buy's on-chain bound and the create-slot rule (docs/04 section
//! 2, docs/06 section 7.1 steps 8b and 11):
//!
//! * `min_tokens_out` is computed at `V_cap = min(v_max, V_pb * sqrt(1 +
//!   entry.max_premium))` with `V_pb = V0 + entry.bundle_lamports` when the
//!   bundle run is not observed (these tests; `entry.require_bundle` binds
//!   at the observed run instead, `hot::park_trade`), so a
//!   fill at +11% over the post-bundle price (rank 3 behind the bundle)
//!   fails on chain while +1.5% lands (the operator's wide `v_max` bound
//!   alone let the +11% fill through);
//! * paper E1 applies the same bound, so on the same tape a paper fill and
//!   the real transaction's on-chain outcome agree (a bundle bigger than
//!   expected fails both);
//! * a real decision that runs after the create slot is refused
//!   (`late_slot`) and nothing is handed to the sender thread.

mod common;

use arc_swap::ArcSwap;
use common::*;
use pump_codec::curve::{CurveState, FeeBps};
use pump_codec::Pubkey;
use rugsnipe::hot::{entry_v_cap, HotCfg, HotLoop, HotParts, Shared};
use rugsnipe::msgs::{FireJob, HotIn, ShadowInfo};
use rugsnipe::setup::{self, min_tokens_out, ArmState};
use rugsnipe::stats::{code, HotStats};
use rugsnipe_core::config::{ArmSetGate, Config, ValidateEnv};
use rugsnipe_core::risk::RiskGate;
use rugsnipe_core::rslog::{box_id, Rec, RslogWriter, ShadowOutcome};
use rugsnipe_core::watchlist::Watchlist;
use rugsnipe_core::{IntentId, Mode};
use rugsnipe_feed::{FeedBuf, FeedMux, FeedSrc, FeedStatus, FiredSet, Payload};
use rugsnipe_tx::senders::{ApiKeys, SenderSet};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

const SPEND: u64 = 100_000_000;
const V_MAX: u64 = 40_000_000_000;
const BUNDLE: u64 = 2_000_000_000;

fn cfg_with_bundle(dir: &std::path::Path) -> Config {
    let mut cfg = paper_cfg(dir);
    cfg.entry.max_premium = 0.02;
    cfg.entry.bundle_lamports = BUNDLE;
    cfg
}

fn hot_cfg(cfg: &Config) -> HotCfg {
    HotCfg::from_config(cfg, setup::exit_policy(cfg).unwrap())
}

/// Does the real buy (bound `min_out`) land when it executes at the state
/// whose price is `premium_bps` above the post-bundle state `(v, t)`?
fn lands_on_chain(v: u64, t: u64, premium_bps: u32, min_out: u64, creator: Pubkey) -> bool {
    let k = v as u128 * t as u128;
    let v_land = entry_v_cap(v, premium_bps);
    let state = CurveState {
        virtual_sol: v_land,
        virtual_token: (k / v_land as u128) as u64,
        real_sol: 0,
        real_token: u64::MAX,
        complete: false,
        creator,
    };
    state
        .quote_buy_exact_sol_in(SPEND, FeeBps::DEFAULT)
        .is_some_and(|q| q.tokens >= min_out)
}

#[test]
fn v_cap_is_the_premium_in_reserve_space() {
    // price = V^2 / k: +2% price is sqrt(1.02) in V
    assert_eq!(entry_v_cap(0, 200), 0);
    assert_eq!(entry_v_cap(10_000_000_000, 0), 10_000_000_000);
    let v = entry_v_cap(30_000_000_000, 200);
    let ratio = (v as f64 / 30_000_000_000f64).powi(2);
    assert!((ratio - 1.02).abs() < 1e-9, "{ratio}");
    assert_eq!(entry_v_cap(u64::MAX, 10_000), u64::MAX, "saturates");
    let dir = tmpdir("vcap");
    let hc = hot_cfg(&cfg_with_bundle(&dir));
    assert_eq!(hc.max_premium_bps, 200);
    // the operator's v_max still caps it
    assert_eq!(hc.v_cap(39_000_000_000, V_MAX), V_MAX);
    assert_eq!(
        hc.v_cap(31_000_000_000, V_MAX),
        entry_v_cap(31_000_000_000 + BUNDLE, 200)
    );
}

#[test]
fn a_fill_at_plus_11_percent_fails_the_bound_and_plus_1_5_lands() {
    let dir = tmpdir("bound11");
    let hc = hot_cfg(&cfg_with_bundle(&dir));
    let (dev, mint, b1) = (
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    );
    // the create with a 1 SOL dev buy (V0), then the expected 2 SOL bundle
    // in separate transactions of the same Jito bundle
    let (_hit, c0) = create(mint, dev, 1_000, 1, 1_000_000_000);
    let mut c = c0;
    trade(&mut c, mint, b1, 1_000, 2, BUNDLE, 0);
    let min_out = min_tokens_out(SPEND, hc.v_cap(c0.virtual_sol, V_MAX), dev, hc.fees);
    assert!(lands_on_chain(
        c.virtual_sol,
        c.virtual_token,
        150,
        min_out,
        dev
    ));
    assert!(!lands_on_chain(
        c.virtual_sol,
        c.virtual_token,
        1_100,
        min_out,
        dev
    ));
    // the defect: the operator's wide v_max bound let the +11% fill through
    let wide = min_tokens_out(SPEND, V_MAX, dev, hc.fees);
    assert!(lands_on_chain(
        c.virtual_sol,
        c.virtual_token,
        1_100,
        wide,
        dev
    ));
}

#[test]
fn paper_e1_and_the_real_bound_agree_on_the_same_tape() {
    let dir = tmpdir("bound-agree");
    let dev = Pubkey::new_unique();
    let cfg = cfg_with_bundle(&dir);
    let hc = hot_cfg(&cfg);
    let mut app = build(cfg, armset(&[dev], &[]), &dir, 5_000, false);
    // (bundle SOL outside the create tx, premium bps): expected bundle at
    // +1.5% / +11%, a bundle 2 SOL bigger than expected, a smaller one
    let cases: [(u64, u32); 4] = [
        (BUNDLE, 150),
        (BUNDLE, 1_100),
        (BUNDLE + 2_000_000_000, 110),
        (1_000_000_000, 110),
    ];
    let mut events = vec![];
    let mut tapes = vec![];
    for (i, (bundle, premium)) in cases.iter().enumerate() {
        let mint = Pubkey::new_unique();
        let (hit, c0) = create(mint, dev, 1_000 + i as u64, 10 + i as u64, 1_000_000_000);
        let mut c = c0;
        trade(
            &mut c,
            mint,
            Pubkey::new_unique(),
            1_000,
            50 + i as u64,
            *bundle,
            0,
        );
        events.push(ev(1_000 + i as u64, Payload::Hit(hit)));
        tapes.push((mint, c0, c, *premium));
    }
    let mut feed = FeedMux::new(vec![Box::new(VecFeed::new(events))]);
    let mut buf = FeedBuf::with_capacity(64, 1 << 16);
    let t0 = Instant::now();
    while app.hot.counters.intents < cases.len() as u64 {
        assert!(
            app.hot.step(&mut feed, &mut buf) != FeedStatus::Ended
                || app.hot.counters.intents == cases.len() as u64,
            "{:?}",
            app.hot.counters
        );
        assert!(t0.elapsed() < Duration::from_secs(10));
    }
    let owner = app.owner;
    let mut verdicts = vec![];
    for (mint, c0, c, premium) in &tapes {
        let before = app.hot.counters.fills;
        app.hot.inject(HotIn::E1Open {
            intent: IntentId::new(mint, &owner).0,
            mint: *mint,
            outcome: ShadowOutcome::Filled,
            info: ShadowInfo {
                create_slot: 1_000,
                create_index: Some(100),
                block_txs: Some(1_000),
                bundle_trades: 2,
                bundle_wallets: 1,
                our_index: Some(102),
                premium_bps: *premium,
                post_bundle_v: c.virtual_sol,
                post_bundle_t: c.virtual_token,
                ..ShadowInfo::default()
            },
        });
        let paper_filled = app.hot.counters.fills > before;
        // the bound the real transaction would carry for this create
        let min_out = min_tokens_out(SPEND, hc.v_cap(c0.virtual_sol, V_MAX), dev, hc.fees);
        let chain = lands_on_chain(c.virtual_sol, c.virtual_token, *premium, min_out, dev);
        verdicts.push((paper_filled, chain));
    }
    assert_eq!(
        verdicts,
        vec![(true, true), (false, false), (false, false), (true, true)],
        "paper E1 and the on-chain bound agree"
    );
    let summary = app.shutdown(vec![]);
    let p = &summary.cold.paper;
    assert_eq!(p.outcomes.get("slippage"), Some(&2), "{:?}", p.outcomes);
    // the decisions recorded the bound at V_cap
    let recs = rslog_records(&dir.join("rslog"));
    let mins: Vec<u64> = recs
        .iter()
        .filter_map(|r| match r {
            Rec::Decision { min_tokens, .. } if *min_tokens > 0 => Some(*min_tokens),
            _ => None,
        })
        .collect();
    let want = min_tokens_out(SPEND, hc.v_cap(tapes[0].1.virtual_sol, V_MAX), dev, hc.fees);
    assert_eq!(mins.len(), 4);
    assert!(mins.iter().all(|m| *m == want), "{mins:?} vs {want}");
}

// ---- the create-slot rule (a hand-assembled live hot loop) ----

fn live_cfg(dir: &std::path::Path) -> Config {
    let mut c = cfg_with_bundle(dir);
    c.mode.mode = Mode::Live;
    c.tx.cu_limit = 65_000;
    c.tx.lads_bytes = 12_124_160;
    c.feeds.grpc.enabled = true;
    c.feeds.grpc.url = "https://grpc.invalid".into();
    c.secrets.hot_keypair = "hot-keypair".into();
    c.secrets.probe_keypair = "probe-keypair".into();
    c.secrets.api_keys = "api-keys".into();
    c.secrets.armset_pubkey = Pubkey::new_unique().to_string();
    c.secrets.cold_wallet = Pubkey::new_unique().to_string();
    c.senders.rpcsend.enabled = false;
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

#[test]
fn a_real_decision_after_the_create_slot_is_refused() {
    let dir = tmpdir("late-slot");
    let cfg = live_cfg(&dir);
    let v = cfg.validate(&good_env(&cfg));
    assert!(v.ok(), "{:?}", v.refusals);
    let senders = Arc::new(SenderSet::build(&cfg, &good_env(&cfg), &ApiKeys::default()).unwrap());
    let signer = Arc::new(setup::throwaway_signer().unwrap());
    let owner = signer.pubkey();
    let nonces = Arc::new(setup::paper_nonce_pool(owner, 8));
    let templates = setup::templates(&cfg, owner, &nonces).unwrap();
    let dev = Pubkey::new_unique();
    let mut set = armset(&[dev], &[]);
    set.gate_passed = true;
    let watch = Arc::new(Watchlist::new());
    watch.load_armset(&set);
    let arm = Arc::new(ArcSwap::from_pointee(ArmState::from_armset(&set, true, "")));
    let risk = Arc::new(RiskGate::new(cfg.risk.clone()));
    risk.set_hot_balance(2_000_000_000);
    let (rslog, w) = RslogWriter::spawn(dir.join("rslog"), "t-hot", box_id("t"), 1 << 12).unwrap();
    let (cold_tx, _cold_rx) = rtrb::RingBuffer::new(4_096);
    let (_fills_tx, fills_rx) = rtrb::RingBuffer::new(64);
    let (fire_tx, fire_rx) = rtrb::RingBuffer::<FireJob>::new(64);
    let mut reg = prometheus_client::registry::Registry::default();
    let stats = Arc::new(HotStats::new(&mut reg));
    let mut hot = HotLoop::new(HotParts {
        cfg: hot_cfg(&cfg),
        watch,
        arm,
        risk: risk.clone(),
        fired: Arc::new(FiredSet::with_capacity(1 << 10)),
        nonces: nonces.clone(),
        signer,
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
    let grpc = |slot, payload| rugsnipe_feed::FeedEvent {
        src: FeedSrc::Grpc,
        ..ev(slot, payload)
    };
    // the lane is at slot 1001 (an unwatched create) when the watched
    // create of slot 1000 arrives: within max_age_slots, but late
    let (late, _) = create(Pubkey::new_unique(), dev, 1_000, 1, 1_000_000_000);
    let (other, _) = create(Pubkey::new_unique(), Pubkey::new_unique(), 1_001, 2, 0);
    // control: a watched create of the current slot fires
    let (now, _) = create(Pubkey::new_unique(), dev, 1_001, 3, 1_000_000_000);
    let mut feed = VecFeed::new(vec![
        grpc(1_001, Payload::Hit(other)),
        grpc(1_000, Payload::Hit(late)),
        grpc(1_001, Payload::Hit(now)),
    ]);
    let mut buf = FeedBuf::with_capacity(64, 1 << 16);
    let t0 = Instant::now();
    while hot.step(&mut feed, &mut buf) != FeedStatus::Ended {
        assert!(t0.elapsed() < Duration::from_secs(10));
    }
    assert_eq!(hot.counters.intents, 1, "{:?}", hot.counters);
    assert!(fire_rx.slots() >= 1);
    let jobs: Vec<u64> = {
        let mut rx = fire_rx;
        std::iter::from_fn(|| rx.pop().ok())
            .map(|j| j.intent)
            .collect()
    };
    let late_intent = IntentId::new(&late.mint, &owner).0;
    let now_intent = IntentId::new(&now.mint, &owner).0;
    assert!(
        !jobs.contains(&late_intent),
        "nothing of the late decision reaches the sender"
    );
    assert!(jobs.contains(&now_intent));
    assert_eq!(nonces.free_count(), 7, "only the control holds a nonce");
    assert_eq!(
        risk.open_positions(),
        1,
        "the late decision released its reservation"
    );
    drop(hot); // the rslog producer: the writer flushes and exits
    w.join().unwrap();
    let recs = rslog_records(&dir.join("rslog"));
    let codes: Vec<u8> = recs
        .iter()
        .filter_map(|r| match r {
            Rec::Decision { intent, code, .. } if *intent == late_intent => Some(*code),
            _ => None,
        })
        .collect();
    assert_eq!(codes, vec![code::LATE_SLOT]);
    assert_eq!(
        rugsnipe::stats::decision_label(code::LATE_SLOT),
        "late_slot"
    );
}

// ---- the pre-send entry gate at the real fire point ----

/// A one-split model on `bsol` (feature 1, the bundle wallets' SOL outside
/// the creator set): P = 0.12 at or below 0.5 SOL, 0.88 above; t1 0.3, t2 0.6.
fn bsol_stump() -> Arc<rugsnipe::filter::Model> {
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/reports/strategy/entry_filter_v1.json"
    );
    let mut v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    v["name"] = "bsol_stump".into();
    v["model"]["baseline"] = 0.0.into();
    v["model"]["trees"] = serde_json::json!([[
        {"leaf": false, "feature": 1, "threshold": 0.5, "left": 1, "right": 2, "missing_left": true},
        {"leaf": true, "value": -2.0},
        {"leaf": true, "value": 2.0}
    ]]);
    v["tertiles"] = serde_json::json!({"t1": 0.3, "t2": 0.6});
    Arc::new(rugsnipe::filter::Model::from_json(&v.to_string()).unwrap())
}

/// `entry.require_bundle` in live mode: a recorded mainnet create (the gRPC
/// copy, transaction index 40) is parked and fires on a bundle buy at index
/// 41 (a shreds quote, pre-execution). The pre-send gate scores it at that
/// point: in `skip` mode a 0.3 SOL bundle (below t2) is never handed to the
/// sender thread (no nonce, the risk reservation released, decision
/// `skip_presend`) and a 1 SOL bundle is; `log` mode sends the 0.3 SOL one.
/// Every fire writes its `Presend` cold message with the run's features.
#[test]
fn the_presend_gate_decides_at_the_real_fire_point() {
    use rugsnipe::msgs::ColdMsg;
    use rugsnipe_core::config::{PresendCut, PresendMode};
    use rugsnipe_feed::wiremeta::{WireMeta, EMPTY_INNER, MAX_INNER};
    const SLOT: u64 = 5_000;
    const CI: u32 = 40;
    let fx = codec_fixture("fresh_v0alt_create_v2_buy18");
    let (tx, meta) = fixture_wire(&fx, CI);
    // the create as the hot loop decodes it (signer for the watchlist)
    let hit = {
        let m = WireMeta::parse(&meta).unwrap();
        let mut loaded = vec![Pubkey::default(); 256];
        let mut inner = [EMPTY_INNER; MAX_INNER];
        let (nl, ni) = (m.loaded_into(&mut loaded), m.inner_into(&mut inner));
        let view = pump_codec::tx::TxView::parse(&tx).unwrap();
        pump_codec::create_hit(&view, Some(&loaded[..nl]), &inner[..ni], FeeBps::DEFAULT)
            .unwrap()
            .unwrap()
    };
    for (mode, bundle_sol, sent) in [
        (PresendMode::Skip, 300_000_000u64, false),
        (PresendMode::Skip, 1_000_000_000, true),
        (PresendMode::Log, 300_000_000, true),
    ] {
        let dir = tmpdir("presend-real");
        let mut cfg = live_cfg(&dir);
        cfg.entry.require_bundle = true;
        cfg.entry.bundle_wait_ms = 0;
        cfg.entry.presend_mode = mode;
        cfg.entry.presend_cut = PresendCut::T2;
        // (the hot loop gets the stump below; the config names a model)
        cfg.entry.presend_model = "data/reports/strategy/entry_filter_v1.json".into();
        let senders =
            Arc::new(SenderSet::build(&cfg, &good_env(&cfg), &ApiKeys::default()).unwrap());
        let signer = Arc::new(setup::throwaway_signer().unwrap());
        let owner = signer.pubkey();
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
        let (cold_tx, mut cold_rx) = rtrb::RingBuffer::new(4_096);
        let (_fills_tx, fills_rx) = rtrb::RingBuffer::new(64);
        let (fire_tx, mut fire_rx) = rtrb::RingBuffer::<FireJob>::new(64);
        let mut reg = prometheus_client::registry::Registry::default();
        let stats = Arc::new(HotStats::new(&mut reg));
        let mut hc = hot_cfg(&cfg);
        hc.presend = Some(bsol_stump());
        let mut hot = HotLoop::new(HotParts {
            cfg: hc,
            watch,
            arm,
            risk: risk.clone(),
            fired: Arc::new(FiredSet::with_capacity(1 << 10)),
            nonces: nonces.clone(),
            signer,
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
        let (v0, t0) = rugsnipe::convert::v0_of(&hit);
        let bundle = rugsnipe_feed::TradeLite {
            mint: hit.mint,
            user: Pubkey::new_unique(),
            is_buy: true,
            sol_amount: bundle_sol,
            token_amount: 10_000_000_000_000,
            fee: 0,
            virtual_sol_reserves: v0 + bundle_sol,
            virtual_token_reserves: t0 - 10_000_000_000_000,
            slot: SLOT,
            sig: sig(77),
            ordinal: 0,
            index: CI + 1,
        };
        let mut feed = FeedMux::new(vec![
            Box::new(WireFeed::new(vec![(SLOT, tx.clone(), meta.clone())])),
            Box::new(VecFeed::new(vec![rugsnipe_feed::FeedEvent {
                src: FeedSrc::Shreds,
                ..ev(SLOT, Payload::Trade(bundle))
            }])),
        ]);
        let mut buf = FeedBuf::with_capacity(64, 1 << 16);
        let t_start = Instant::now();
        while hot.step(&mut feed, &mut buf) != FeedStatus::Ended {
            assert!(t_start.elapsed() < Duration::from_secs(10));
        }
        let c = hot.counters;
        assert_eq!((c.parked, c.bundle_fired), (1, 1), "{c:?}");
        let skip = !sent;
        assert_eq!(
            (c.presend_scored, c.presend_skipped, c.intents),
            (1, skip as u64, sent as u64),
            "{mode:?} {bundle_sol}: {c:?}"
        );
        let intent = IntentId::new(&hit.mint, &owner).0;
        let jobs: Vec<u64> = std::iter::from_fn(|| fire_rx.pop().ok())
            .map(|j| j.intent)
            .collect();
        assert_eq!(jobs.contains(&intent), sent, "{mode:?} {bundle_sol}");
        assert_eq!(nonces.free_count(), if sent { 7 } else { 8 });
        assert_eq!(risk.open_positions(), sent as u64);
        // the cold message: the run's features as known at the fire point
        let pre: Vec<(bool, bool, f64, f64)> = std::iter::from_fn(|| cold_rx.pop().ok())
            .filter_map(|m| match m {
                ColdMsg::Presend {
                    intent: i,
                    scored,
                    skipped,
                    feat,
                    ..
                } if i == intent => Some((scored, skipped, feat.x[0], feat.x[1])),
                _ => None,
            })
            .collect();
        assert_eq!(pre, vec![(true, skip, 1.0, bundle_sol as f64 / 1e9)]);
        drop(hot);
        w.join().unwrap();
        let codes: Vec<u8> = rslog_records(&dir.join("rslog"))
            .iter()
            .filter_map(|r| match r {
                Rec::Decision {
                    intent: i, code, ..
                } if *i == intent => Some(*code),
                _ => None,
            })
            .collect();
        assert_eq!(
            codes,
            vec![if sent { code::FIRE } else { code::PRESEND }],
            "{mode:?} {bundle_sol}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
    assert_eq!(
        rugsnipe::stats::decision_label(code::PRESEND),
        "skip_presend"
    );
}
