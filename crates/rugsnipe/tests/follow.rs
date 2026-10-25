//! `entry.follow` (docs/16): the copied wallet follows operators by their
//! bundle wallets. At the fire, a parked create is followed when one of its
//! bundle wallets is in the set the cold thread publishes (`Shared::follow`).
//! `log` journals the verdict and parks the creates the rule skips (here an
//! off-template dev buy) only to journal theirs; `gate` skips the rule's
//! coins that are not followed; `add` buys a followed coin the rule skips.

mod common;

use arc_swap::ArcSwap;
use common::*;
use pump_codec::curve::FeeBps;
use pump_codec::Pubkey;
use rugsnipe::hot::{HotCfg, HotLoop, HotParts, Shared};
use rugsnipe::msgs::FireJob;
use rugsnipe::setup::{self, ArmState};
use rugsnipe::stats::{code, decision_label, HotStats};
use rugsnipe_core::config::{ArmSetGate, Config, Follow, ValidateEnv};
use rugsnipe_core::risk::RiskGate;
use rugsnipe_core::rslog::{box_id, Rec, RslogWriter};
use rugsnipe_core::watchlist::Watchlist;
use rugsnipe_core::IntentId;
use rugsnipe_feed::{FeedBuf, FeedEvent, FeedMux, FeedSrc, FeedStatus, FiredSet, Payload};
use rugsnipe_tx::senders::{ApiKeys, SenderSet};
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

const SLOT: u64 = 5_000;
const CI: u32 = 40;

fn live_cfg(dir: &std::path::Path, follow: Follow, ranges: &[[f64; 2]]) -> Config {
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
    c.entry.bundle_wait_ms = 0;
    c.entry.max_premium = 0.0;
    c.entry.dev_buy_ranges_sol = ranges.to_vec();
    c.entry.follow = follow;
    c.entry.follow_wallet = Pubkey::new_unique().to_string();
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

struct Outcome {
    fired: bool,
    decisions: Vec<u8>,
    follow: [u64; 6],
}

fn labels(o: &Outcome) -> Vec<&'static str> {
    o.decisions.iter().map(|d| decision_label(*d)).collect()
}

/// A plain create (its dev buy shown) as the gRPC feed delivers it.
fn the_create() -> (Vec<u8>, Vec<u8>, pump_codec::create_hit::CreateHit) {
    use rugsnipe_feed::wiremeta::{WireMeta, EMPTY_INNER, MAX_INNER};
    let fx = codec_fixture("fresh_v0alt_create_v2_buy18");
    let (tx, meta) = fixture_wire(&fx, CI);
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
    (tx, meta, hit)
}

/// The dev-buy template around the fixture's own dev buy (`on`), or one it
/// misses.
fn template(hit: &pump_codec::create_hit::CreateHit, on: bool) -> Vec<[f64; 2]> {
    let d = hit.intx_sol_in as f64 / 1e9;
    if on {
        vec![[d - 0.01, d + 0.01]]
    } else {
        vec![[d + 10.0, d + 11.0]]
    }
}

/// The create, then one bundle buy right behind it by a wallet that is in
/// the published follow set when `followed`.
fn run(follow: Follow, on_template: bool, followed: bool) -> Outcome {
    let (tx, meta, hit) = the_create();
    assert!(hit.intx_sol_in > 0);
    let dir = tmpdir("follow");
    let cfg = live_cfg(&dir, follow, &template(&hit, on_template));
    let v = cfg.validate(&good_env(&cfg));
    assert!(v.ok(), "{:?}", v.refusals);
    let senders = Arc::new(SenderSet::build(&cfg, &good_env(&cfg), &ApiKeys::default()).unwrap());
    let wallet = Arc::new(setup::throwaway_signer().unwrap());
    let owner = wallet.pubkey();
    let nonces = Arc::new(setup::paper_nonce_pool(owner, 8));
    let templates = setup::templates(&cfg, owner, &nonces).unwrap();
    let bw = Pubkey::new_unique();
    let shared = Arc::new(Shared::default());
    if followed {
        shared.follow.store(Arc::new(HashSet::from([bw])));
    }
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
        shared,
        leaders: rugsnipe::leader::empty_snapshot(),
        dumpers: rugsnipe::dumpers::snapshot_from(vec![]),
    });
    let (v0, t0) = rugsnipe::convert::v0_of(&hit);
    let sol = 1_000_000_000u64;
    let bundle = rugsnipe_feed::TradeLite {
        mint: hit.mint,
        user: bw,
        is_buy: true,
        sol_amount: sol,
        token_amount: 10_000_000_000_000,
        fee: 0,
        virtual_sol_reserves: v0 + sol,
        virtual_token_reserves: t0 - 10_000_000_000_000,
        slot: SLOT,
        sig: sig(77),
        ordinal: 0,
        index: CI + 1,
    };
    let mut feed = FeedMux::new(vec![
        Box::new(WireFeed::new(vec![(SLOT, tx, meta)])),
        Box::new(VecFeed::new(vec![FeedEvent {
            src: FeedSrc::Grpc,
            ..ev(SLOT, Payload::Trade(bundle))
        }])),
    ]);
    let mut buf = FeedBuf::with_capacity(64, 1 << 16);
    let t_start = Instant::now();
    while hot.step(&mut feed, &mut buf) != FeedStatus::Ended {
        assert!(t_start.elapsed() < Duration::from_secs(10));
    }
    let intent = IntentId::new(&hit.mint, &owner).0;
    let fired = std::iter::from_fn(|| fire_rx.pop().ok()).any(|j| j.intent == intent);
    let follow = hot.counters.follow;
    drop(hot);
    w.join().unwrap();
    let decisions = rslog_records(&dir.join("rslog"))
        .iter()
        .filter_map(|r| match r {
            Rec::Decision {
                intent: i, code, ..
            } if *i == intent => Some(*code),
            _ => None,
        })
        .collect();
    let _ = std::fs::remove_dir_all(&dir);
    Outcome {
        fired,
        decisions,
        follow,
    }
}

#[test]
fn log_fires_the_rule_coins_and_only_journals_the_rest() {
    let o = run(Follow::Log, true, false);
    assert!(o.fired, "{:?}", labels(&o));
    assert_eq!(o.follow, [0, 1, 0, 0, 0, 0], "a rule coin, not followed");
    // off the template: parked for the verdict, followed, still skipped
    let o = run(Follow::Log, false, true);
    assert!(!o.fired);
    assert_eq!(o.decisions, vec![code::DEV_BUY], "{:?}", labels(&o));
    assert_eq!(o.follow, [0, 0, 0, 1, 0, 0]);
    // off: skipped at the create, no verdict
    let o = run(Follow::Off, false, true);
    assert!(!o.fired);
    assert_eq!(o.decisions, vec![code::DEV_BUY]);
    assert_eq!(o.follow, [0; 6]);
}

#[test]
fn gate_skips_a_rule_coin_no_followed_wallet_bundled() {
    let o = run(Follow::Gate, true, false);
    assert!(!o.fired);
    assert_eq!(o.decisions, vec![code::NOT_FOLLOWED], "{:?}", labels(&o));
    assert_eq!(o.follow, [0, 1, 1, 0, 0, 0]);
    let o = run(Follow::Gate, true, true);
    assert!(o.fired, "{:?}", labels(&o));
    assert_eq!(o.follow, [1, 0, 0, 0, 0, 0]);
    // gate alone parks no skip
    let o = run(Follow::Gate, false, true);
    assert!(!o.fired);
    assert_eq!(o.follow, [0; 6]);
}

#[test]
fn add_buys_a_followed_coin_the_rule_skips() {
    let o = run(Follow::Add, false, true);
    assert!(o.fired, "{:?}", labels(&o));
    assert_eq!(o.follow, [0, 0, 0, 1, 0, 1]);
    let o = run(Follow::Add, false, false);
    assert!(!o.fired);
    assert_eq!(o.decisions, vec![code::DEV_BUY], "{:?}", labels(&o));
    assert_eq!(o.follow, [0, 0, 0, 0, 1, 0]);
    // add does not gate the rule's coins; on does
    assert!(run(Follow::Add, true, false).fired);
    assert!(!run(Follow::On, true, false).fired);
    assert!(run(Follow::On, false, true).fired);
}

#[test]
fn follow_needs_the_wallet_and_the_bundle_feed() {
    let dir = tmpdir("follow-cfg");
    let mut c = live_cfg(&dir, Follow::Log, &[]);
    c.entry.follow_wallet = "not-a-key".into();
    assert!(!c.validate(&good_env(&c)).ok());
    let mut c = live_cfg(&dir, Follow::Log, &[]);
    c.feeds.grpc.bundle_run = false;
    assert!(!c.validate(&good_env(&c)).ok());
    let mut c = live_cfg(&dir, Follow::On, &[]);
    c.entry.require_bundle = false;
    assert!(!c.validate(&good_env(&c)).ok());
    let mut c = live_cfg(&dir, Follow::Log, &[]);
    c.entry.follow_min_rate = 0.0;
    assert!(!c.validate(&good_env(&c)).ok());
    let _ = std::fs::remove_dir_all(&dir);
}
