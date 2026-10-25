//! `entry.follow` (docs/16): the copied wallet follows operators by their
//! bundle wallets, per fee routing. At the fire, a parked create is followed
//! when one of its bundle wallets, with the create's fee routing, is in the
//! set the cold thread publishes (`Shared::follow`).
//! `log` journals the verdict and parks the creates the rule skips (here an
//! off-template dev buy) only to journal theirs; `gate` skips the rule's
//! coins that are not followed; `add` buys a followed coin the rule skips.

mod common;

use arc_swap::ArcSwap;
use common::*;
use pump_codec::curve::FeeBps;
use pump_codec::Pubkey;
use rugsnipe::follow::{Fees, FollowSet};
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
use std::collections::BTreeMap;
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
    /// The size of each decision record (the fire's is the stake).
    sizes: Vec<u64>,
    follow: [u64; 8],
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
/// the published follow set when `followed`, with the create's fee routing.
fn run(follow: Follow, on_template: bool, followed: bool) -> Outcome {
    run_routed(follow, on_template, followed, true)
}

/// [`run`], the wallet followed with the create's fee routing
/// (`same_routing`) or the other one.
fn run_routed(follow: Follow, on_template: bool, followed: bool, same_routing: bool) -> Outcome {
    run_sized(
        follow,
        on_template,
        followed,
        same_routing,
        false,
        (1.0, 1.0),
    )
}

/// [`run_routed`], the followed record also hot when `hot`, with the stake
/// multipliers `(follow_hot_stake_mult, follow_cold_stake_mult)`.
fn run_sized(
    follow: Follow,
    on_template: bool,
    followed: bool,
    same_routing: bool,
    hot: bool,
    mults: (f64, f64),
) -> Outcome {
    run_full(
        follow,
        on_template,
        followed,
        same_routing,
        hot,
        mults,
        &|_| {},
        2_000_000_000,
    )
}

/// [`run_sized`], the config changed by `tweak` and the wallet at `balance`.
#[allow(clippy::too_many_arguments)]
fn run_full(
    follow: Follow,
    on_template: bool,
    followed: bool,
    same_routing: bool,
    hot: bool,
    mults: (f64, f64),
    tweak: &dyn Fn(&mut Config),
    balance: u64,
) -> Outcome {
    let (tx, meta, hit) = the_create();
    assert!(hit.intx_sol_in > 0);
    let dir = tmpdir("follow");
    let mut cfg = live_cfg(&dir, follow, &template(&hit, on_template));
    cfg.entry.follow_hot_stake_mult = mults.0;
    cfg.entry.follow_cold_stake_mult = mults.1;
    tweak(&mut cfg);
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
        let fees = Fees::of(Some(hit.dev_fees() == same_routing));
        let mut set = FollowSet::followed([(bw, fees)]);
        if hot {
            set.hot.insert((bw, fees));
        }
        shared.follow.store(Arc::new(set));
    }
    let mut set = armset(&[hit.user], &[]);
    set.gate_passed = true;
    let watch = Arc::new(Watchlist::new());
    watch.load_armset(&set);
    let arm = Arc::new(ArcSwap::from_pointee(ArmState::from_armset(&set, true, "")));
    let risk = Arc::new(RiskGate::new(cfg.risk.clone()));
    risk.set_hot_balance(balance);
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
        tip: 0,
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
    let (decisions, sizes) = rslog_records(&dir.join("rslog"))
        .iter()
        .filter_map(|r| match r {
            Rec::Decision {
                intent: i,
                code,
                size,
                ..
            } if *i == intent => Some((*code, *size)),
            _ => None,
        })
        .unzip();
    let _ = std::fs::remove_dir_all(&dir);
    Outcome {
        fired,
        decisions,
        sizes,
        follow,
    }
}

#[test]
fn log_fires_the_rule_coins_and_only_journals_the_rest() {
    let o = run(Follow::Log, true, false);
    assert!(o.fired, "{:?}", labels(&o));
    assert_eq!(
        o.follow,
        [0, 1, 0, 0, 0, 0, 0, 1],
        "a rule coin, not followed"
    );
    // off the template: parked for the verdict, followed, still skipped
    let o = run(Follow::Log, false, true);
    assert!(!o.fired);
    assert_eq!(o.decisions, vec![code::DEV_BUY], "{:?}", labels(&o));
    assert_eq!(o.follow, [0, 0, 0, 1, 0, 0, 0, 0]);
    // off: skipped at the create, no verdict
    let o = run(Follow::Off, false, true);
    assert!(!o.fired);
    assert_eq!(o.decisions, vec![code::DEV_BUY]);
    assert_eq!(o.follow, [0; 8]);
}

#[test]
fn gate_skips_a_rule_coin_no_followed_wallet_bundled() {
    let o = run(Follow::Gate, true, false);
    assert!(!o.fired);
    assert_eq!(o.decisions, vec![code::NOT_FOLLOWED], "{:?}", labels(&o));
    assert_eq!(o.follow, [0, 1, 1, 0, 0, 0, 0, 0]);
    let o = run(Follow::Gate, true, true);
    assert!(o.fired, "{:?}", labels(&o));
    assert_eq!(o.follow, [1, 0, 0, 0, 0, 0, 0, 1]);
    // gate alone parks no skip
    let o = run(Follow::Gate, false, true);
    assert!(!o.fired);
    assert_eq!(o.follow, [0; 8]);
}

#[test]
fn add_buys_a_followed_coin_the_rule_skips() {
    let o = run(Follow::Add, false, true);
    assert!(o.fired, "{:?}", labels(&o));
    assert_eq!(o.follow, [0, 0, 0, 1, 0, 1, 0, 1]);
    let o = run(Follow::Add, false, false);
    assert!(!o.fired);
    assert_eq!(o.decisions, vec![code::DEV_BUY], "{:?}", labels(&o));
    assert_eq!(o.follow, [0, 0, 0, 0, 1, 0, 0, 0]);
    // the wallet is followed for the other fee routing only: not followed
    let o = run_routed(Follow::Add, false, true, false);
    assert!(!o.fired);
    assert_eq!(o.follow, [0, 0, 0, 0, 1, 0, 0, 0]);
    // add does not gate the rule's coins; on does
    assert!(run(Follow::Add, true, false).fired);
    assert!(!run(Follow::On, true, false).fired);
    assert!(run(Follow::On, false, true).fired);
}

#[test]
fn a_hot_coin_keeps_its_stake_and_a_cold_one_takes_the_cold_multiplier() {
    // the multipliers at 1.0 (the defaults): the stake of a followed coin
    let base = run_sized(Follow::On, true, true, true, false, (1.0, 1.0));
    assert!(base.fired, "{:?}", labels(&base));
    let stake = *base.sizes.last().unwrap();
    assert!(stake > 0);
    assert_eq!(base.follow[6..], [0, 1], "cold");
    // cold at 0.5: half the stake
    let cold = run_sized(Follow::On, true, true, true, false, (1.5, 0.5));
    assert!(cold.fired, "{:?}", labels(&cold));
    assert_eq!(*cold.sizes.last().unwrap(), stake / 2);
    // hot: never below the stake, and the cold multiplier does not apply
    let hot = run_sized(Follow::On, true, true, true, true, (1.5, 0.5));
    assert!(hot.fired, "{:?}", labels(&hot));
    assert_eq!(hot.follow[6..], [1, 0], "hot");
    let h = *hot.sizes.last().unwrap();
    assert!(h >= stake && h <= stake * 3 / 2, "{h} vs {stake}");
}

#[test]
fn a_stake_above_the_free_balance_fits_it_instead_of_being_refused() {
    let base = run_sized(Follow::On, true, true, true, false, (1.0, 1.0));
    let stake = *base.sizes.last().unwrap();
    let reserve = 100_000_000u64;
    let fits = |on: bool| {
        move |c: &mut Config| {
            c.risk.min_hot_balance_lamports = reserve;
            c.entry.stake_fits_balance = on;
            c.entry.stake_min_sol = 0.01;
        }
    };
    // the wallet holds the reserve, half the stake and some change
    let wallet = reserve + stake / 2 + 20_000_000;
    let off = run_full(
        Follow::On,
        true,
        true,
        true,
        false,
        (1.0, 1.0),
        &fits(false),
        wallet,
    );
    assert!(!off.fired);
    assert_eq!(off.decisions, vec![0x25], "{:?}", labels(&off));
    let on = run_full(
        Follow::On,
        true,
        true,
        true,
        false,
        (1.0, 1.0),
        &fits(true),
        wallet,
    );
    assert!(on.fired, "{:?}", labels(&on));
    let s = *on.sizes.last().unwrap();
    assert!(
        s < stake && s > stake / 2 && s + reserve < wallet,
        "{s} of {stake}, wallet {wallet}"
    );
    // below stake_min_sol: refused as before
    let tiny = run_full(
        Follow::On,
        true,
        true,
        true,
        false,
        (1.0, 1.0),
        &|c: &mut Config| {
            c.risk.min_hot_balance_lamports = reserve;
            c.entry.stake_fits_balance = true;
            c.entry.stake_min_sol = 10.0;
        },
        wallet,
    );
    assert_eq!(tiny.decisions, vec![0x25], "{:?}", labels(&tiny));
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
