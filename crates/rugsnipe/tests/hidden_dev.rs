//! A create whose dev buy the shreds copy cannot see (`entry.hidden_dev_buy`,
//! 2026-10-25): m.rapidlaunch.io buys for the dev by CPI from its own
//! program, so the first copy (shreds, no meta) reads a 0 SOL dev buy and
//! the pre-execution quotes of the bundle are priced on a curve without it.
//! "skip" judges the 0 (every such launch fails the template); "grpc" parks
//! it, takes the dev buy and the run from the executed trades only and
//! checks the template at the fire.

mod common;

use arc_swap::ArcSwap;
use common::*;
use pump_codec::curve::{CurveState, FeeBps};
use pump_codec::Pubkey;
use rugsnipe::hot::{dev_hidden, HotCfg, HotLoop, HotParts, Shared};
use rugsnipe::msgs::FireJob;
use rugsnipe::setup::{self, ArmState};
use rugsnipe::stats::{code, decision_label, HotStats};
use rugsnipe_core::config::{ArmSetGate, Config, HiddenDevBuy, ValidateEnv};
use rugsnipe_core::risk::RiskGate;
use rugsnipe_core::rslog::{box_id, Rec, RslogWriter};
use rugsnipe_core::watchlist::Watchlist;
use rugsnipe_core::IntentId;
use rugsnipe_feed::wiremeta::{encode, MetaIn};
use rugsnipe_feed::{FeedBuf, FeedEvent, FeedMux, FeedSrc, FeedStatus, FiredSet, Payload};
use rugsnipe_tx::senders::{ApiKeys, SenderSet};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

const SLOT: u64 = 5_000;
const CI: u32 = 40;
const DEV: u64 = 2_962_962_962;

fn live_cfg(dir: &std::path::Path) -> Config {
    let mut c = paper_cfg(dir);
    c.mode.mode = rugsnipe_core::Mode::Live;
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

/// The rapidlaunch create as the shreds feed delivers it: the transaction
/// and a trailer with its block index only (no inner instructions).
fn shreds_create() -> (Vec<u8>, Vec<u8>, pump_codec::create_hit::CreateHit) {
    let fx = codec_fixture("rapidlaunch_create_v2_cpi_dev_buy");
    let (tx, _) = fixture_wire(&fx, CI);
    let mut meta = vec![];
    encode(
        &MetaIn {
            tx_index: Some(CI),
            failed: false,
            fee: 5_000,
            loaded_writable: vec![],
            loaded_readonly: vec![],
            inner: None,
        },
        &mut meta,
    );
    let view = pump_codec::tx::TxView::parse(&tx).unwrap();
    let hit = pump_codec::create_hit(&view, Some(&[]), &[], FeeBps::DEFAULT)
        .unwrap()
        .unwrap();
    (tx, meta, hit)
}

struct Outcome {
    fired: bool,
    decisions: Vec<(u8, u64)>,
}

/// The create (shreds), then per bundle buyer an executed trade (gRPC) on
/// the true curve and, when `quotes`, its pre-execution quote (shreds) on
/// a curve without the dev buy; `dev_trade`: the create's own executed buy.
fn run(mode: HiddenDevBuy, ranges: Vec<[f64; 2]>, dev_trade: bool, quotes: bool) -> Outcome {
    let (tx, meta, hit) = shreds_create();
    assert!(dev_hidden(&hit), "the shreds copy hides the dev buy");
    let dir = tmpdir("hidden-dev");
    let mut cfg = live_cfg(&dir);
    cfg.entry.hidden_dev_buy = mode;
    cfg.entry.dev_buy_ranges_sol = ranges;
    let v = cfg.validate(&good_env(&cfg));
    assert!(v.ok(), "{:?}", v.refusals);
    let senders = Arc::new(SenderSet::build(&cfg, &good_env(&cfg), &ApiKeys::default()).unwrap());
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
    let (rslog, w) = RslogWriter::spawn(dir.join("rslog"), "t-hot", box_id("t"), 1 << 12).unwrap();
    let (cold_tx, _cold_rx) = rtrb::RingBuffer::new(4_096);
    let (_fills_tx, fills_rx) = rtrb::RingBuffer::new(64);
    let (fire_tx, mut fire_rx) = rtrb::RingBuffer::<FireJob>::new(64);
    let mut reg = prometheus_client::registry::Registry::default();
    let stats = Arc::new(HotStats::new(&mut reg));
    let hc = HotCfg::from_config(&cfg, setup::exit_policy(&cfg).unwrap());
    let mut hot = HotLoop::new(HotParts {
        cfg: hc,
        watch,
        arm,
        risk,
        fired: Arc::new(FiredSet::with_capacity(1 << 10)),
        nonces,
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
    // the true curve: the dev's buy, then the bundle; the quotes' curve
    // misses the dev's buy
    let mut real = CurveState::fresh(hit.creator_arg);
    let mut blind = CurveState::fresh(hit.creator_arg);
    let mut evs = vec![];
    let dev = trade(&mut real, hit.mint, hit.user, SLOT, 80, DEV, 0);
    if dev_trade {
        let mut d = dev;
        d.index = CI;
        evs.push(FeedEvent {
            src: FeedSrc::Grpc,
            ..ev(SLOT, Payload::Trade(d))
        });
    }
    let ws = [
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    ];
    for (k, (u, sol, sell)) in [
        (ws[0], 1_000_000_000u64, 0u64),
        (ws[1], 500_000_000, 0),
        (ws[2], 300_000_000, 0),
        (ws[0], 0, 1_000_000_000_000),
    ]
    .into_iter()
    .enumerate()
    {
        let index = if k < 3 { CI + 1 + k as u32 } else { CI + 5 };
        if quotes {
            let mut q = trade(&mut blind, hit.mint, u, SLOT, 190 + k as u64, sol, sell);
            q.index = index;
            evs.push(FeedEvent {
                src: FeedSrc::Shreds,
                ..ev(SLOT, Payload::Trade(q))
            });
        }
        let mut t = trade(&mut real, hit.mint, u, SLOT, 90 + k as u64, sol, sell);
        t.index = index;
        evs.push(FeedEvent {
            src: FeedSrc::Grpc,
            ..ev(SLOT, Payload::Trade(t))
        });
    }
    let mut wire = WireFeed::new(vec![(SLOT, tx, meta)]);
    wire.items[0].0.src = FeedSrc::Shreds;
    let mut feed = FeedMux::new(vec![Box::new(wire), Box::new(VecFeed::new(evs))]);
    let mut buf = FeedBuf::with_capacity(64, 1 << 16);
    let t_start = Instant::now();
    while hot.step(&mut feed, &mut buf) != FeedStatus::Ended {
        assert!(t_start.elapsed() < Duration::from_secs(10));
    }
    let intent = IntentId::new(&hit.mint, &owner).0;
    let fired = std::iter::from_fn(|| fire_rx.pop().ok()).any(|j| j.intent == intent);
    drop(hot);
    w.join().unwrap();
    let decisions = rslog_records(&dir.join("rslog"))
        .iter()
        .filter_map(|r| match r {
            Rec::Decision {
                intent: i,
                code,
                min_tokens,
                ..
            } if *i == intent => Some((*code, *min_tokens)),
            _ => None,
        })
        .collect();
    let _ = std::fs::remove_dir_all(&dir);
    Outcome { fired, decisions }
}

fn labels(o: &Outcome) -> Vec<&'static str> {
    o.decisions.iter().map(|d| decision_label(d.0)).collect()
}

const TEMPLATE: [[f64; 2]; 2] = [[0.95, 1.05], [2.95, 4.05]];

#[test]
fn skip_judges_the_zero_the_shreds_copy_shows() {
    let o = run(HiddenDevBuy::Skip, TEMPLATE.to_vec(), true, true);
    assert!(!o.fired);
    assert!(
        o.decisions.iter().any(|d| d.0 == code::DEV_BUY),
        "{:?}",
        labels(&o)
    );
}

#[test]
fn grpc_fires_on_the_executed_dev_buy_and_ignores_the_blind_quotes() {
    let with_quotes = run(HiddenDevBuy::Grpc, TEMPLATE.to_vec(), true, true);
    assert!(with_quotes.fired, "{:?}", labels(&with_quotes));
    let without = run(HiddenDevBuy::Grpc, TEMPLATE.to_vec(), true, false);
    assert!(without.fired, "{:?}", labels(&without));
    let bound = |o: &Outcome| o.decisions.iter().map(|d| d.1).max().unwrap_or(0);
    assert!(bound(&without) > 0);
    assert_eq!(
        bound(&with_quotes),
        bound(&without),
        "the buy is bound on the executed run, the quotes change nothing"
    );
}

#[test]
fn grpc_checks_the_template_at_the_fire_and_skips_an_unseen_dev_buy() {
    // 2.963 SOL is outside a 1 SOL template
    let o = run(HiddenDevBuy::Grpc, vec![[0.95, 1.05]], true, true);
    assert!(!o.fired);
    assert!(
        o.decisions.iter().any(|d| d.0 == code::DEV_BUY),
        "{:?}",
        labels(&o)
    );
    // no executed dev buy came: unknown, skipped
    let o = run(HiddenDevBuy::Grpc, TEMPLATE.to_vec(), false, true);
    assert!(!o.fired);
    assert!(
        o.decisions.iter().any(|d| d.0 == code::DEV_BUY),
        "{:?}",
        labels(&o)
    );
}
