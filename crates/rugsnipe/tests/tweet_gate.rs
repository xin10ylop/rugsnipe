//! The tweet check in the hot loop (`entry.tweet_gate`, docs/13): a
//! recorded mainnet create parked in live mode, its bundle run complete at
//! the run's last index, and the lane's verdict injected through the ring.
//!
//! * "on": the create's metadata request goes out before the park; the
//!   complete run waits for the verdict; `first` fires at the exact
//!   post-bundle bound, `copy` / `stale` / `no_tweet` / `error` skip as
//!   `skip_tweet_*` with no nonce or risk taken;
//! * a pass may still fire `entry.tweet_late_slots` after the create slot,
//!   unless an outside buy shows first (`skip_not_first`); no verdict by
//!   then is `skip_tweet_late`;
//! * a create the lane was not asked about (not a tweet host) is
//!   `skip_tweet_none` before the park;
//! * "log": the run fires as without the check; the verdict is journaled.

mod common;

use arc_swap::ArcSwap;
use common::*;
use pump_codec::create_hit::CreateHit;
use pump_codec::curve::{CurveState, FeeBps};
use pump_codec::Pubkey;
use rugsnipe::hot::{HotCfg, HotLoop, HotParts, Shared};
use rugsnipe::msgs::FireJob;
use rugsnipe::setup::{self, ArmState};
use rugsnipe::stats::{code, decision_label, HotStats};
use rugsnipe::tweet::{TweetKind, TweetReq, TweetVerdict};
use rugsnipe::tweetfeed::{new_posts, Post, Posts};
use rugsnipe_core::config::{ArmSetGate, Config, TweetFeed, TweetGate, ValidateEnv};
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

fn fixture_hit() -> (Vec<u8>, Vec<u8>, CreateHit) {
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
    (tx, meta, hit)
}

/// What one run of the scenario left.
struct Outcome {
    decisions: Vec<(u8, u64)>,
    fired: bool,
    requests: Vec<TweetReq>,
    nonces_free: usize,
    open: u64,
    /// The position's hold (`create + N`) after the verdict, if one opened.
    hold: Option<u16>,
    /// The bid of each fired job of the launch.
    bids: Vec<u64>,
    /// The fixture's mint.
    mint: Pubkey,
}

/// `[exit.default]` with every rule off but the hold to create + 10.
const HOLD10: &str = r#"
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
hold_to_create_slots = 10
"#;

/// The create, its bundle run (three buyers at CI+1..3, a sell by the
/// first at CI+5: the run's last index), then `after` (events of later
/// slots); the verdict `verdict` (if any) is pushed once the feed has
/// ended, after which the loop steps once more.
fn run(
    gate: TweetGate,
    tweet_host_matches: bool,
    verdict: Option<TweetKind>,
    after: &dyn Fn(&CreateHit, &mut CurveState) -> Vec<FeedEvent>,
) -> Outcome {
    run_full(gate, tweet_host_matches, verdict, false, after)
}

/// [`run`], with the verdict pushed before the create when `verdict_first`.
fn run_full(
    gate: TweetGate,
    tweet_host_matches: bool,
    verdict: Option<TweetKind>,
    verdict_first: bool,
    after: &dyn Fn(&CreateHit, &mut CurveState) -> Vec<FeedEvent>,
) -> Outcome {
    run_full_with(
        gate,
        tweet_host_matches,
        verdict,
        verdict_first,
        after,
        &|_| {},
        None,
    )
}

fn run_full_with(
    gate: TweetGate,
    tweet_host_matches: bool,
    verdict: Option<TweetKind>,
    verdict_first: bool,
    after: &dyn Fn(&CreateHit, &mut CurveState) -> Vec<FeedEvent>,
    tweak: &dyn Fn(&mut Config),
    feed: Option<Posts>,
) -> Outcome {
    let (tx, meta, hit) = fixture_hit();
    let host = hit
        .tx_shape
        .uri_host()
        .expect("the fixture's uri has an http(s) host")
        .to_string();
    assert!(
        hit.tx_shape.uri_str().is_some(),
        "the fixture's uri is kept"
    );
    let dir = tmpdir("tweet-gate");
    let mut cfg = live_cfg(&dir);
    cfg.entry.tweet_gate = gate;
    cfg.entry.tweet_hosts = vec![if tweet_host_matches {
        host.clone()
    } else {
        "metadata.j7tracker.io".into()
    }];
    cfg.entry.tweet_min_dev_sol = 0.0;
    cfg.entry.tweet_late_slots = 1;
    cfg.exit = Config::from_toml_str(HOLD10).unwrap().exit;
    tweak(&mut cfg);
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
    let (req_tx, mut req_rx) = rtrb::RingBuffer::<TweetReq>::new(16);
    let (mut out_tx, out_rx) = rtrb::RingBuffer::<TweetVerdict>::new(16);
    let mut reg = prometheus_client::registry::Registry::default();
    let stats = Arc::new(HotStats::new(&mut reg));
    let hc = HotCfg::from_config(&cfg, setup::exit_policy(&cfg).unwrap());
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
    hot.set_tweet_lane(req_tx, out_rx);
    if let Some(posts) = feed {
        hot.set_tweet_feed(posts);
    }
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
    let mut evs = vec![];
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
        evs.push(FeedEvent {
            src: FeedSrc::Shreds,
            ..ev(SLOT, Payload::Trade(t))
        });
    }
    evs.extend(after(&hit, &mut c));
    let mut feed = FeedMux::new(vec![
        Box::new(WireFeed::new(vec![(SLOT, tx.clone(), meta.clone())])),
        Box::new(VecFeed::new(evs)),
    ]);
    let mut buf = FeedBuf::with_capacity(64, 1 << 16);
    let v = verdict.map(|kind| TweetVerdict {
        mint: hit.mint,
        slot: SLOT,
        kind,
        sid: 2_113_944_610_368_082_408,
        rank: if kind == TweetKind::Copy { 2 } else { 1 },
        age_s: 4,
        push_ns: 0,
        fetch_us: 90_000,
        fail: rugsnipe::tweet::FetchFail::None,
        http: 200,
        tries: 1,
    });
    if let (Some(v), true) = (v, verdict_first) {
        out_tx.push(v).ok().unwrap();
        hot.step(&mut VecFeed::new(vec![]), &mut buf);
    }
    let t_start = Instant::now();
    while hot.step(&mut feed, &mut buf) != FeedStatus::Ended {
        assert!(t_start.elapsed() < Duration::from_secs(10));
    }
    let requests: Vec<TweetReq> = std::iter::from_fn(|| req_rx.pop().ok()).collect();
    if let (Some(v), false) = (v, verdict_first) {
        out_tx.push(v).ok().unwrap();
        hot.step(&mut feed, &mut buf);
    }
    let hold = hot
        .position_of(&hit.mint)
        .and_then(|p| p.policy.hold_to_create_slot);
    let intent = IntentId::new(&hit.mint, &owner).0;
    let jobs: Vec<FireJob> = std::iter::from_fn(|| fire_rx.pop().ok())
        .filter(|j| j.intent == intent)
        .collect();
    let fired = !jobs.is_empty();
    let bids = jobs.iter().map(|j| j.bid).collect();
    let nonces_free = nonces.free_count();
    let open = risk.open_positions();
    drop(hot);
    w.join().unwrap();
    let decisions: Vec<(u8, u64)> = rslog_records(&dir.join("rslog"))
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
    Outcome {
        decisions,
        fired,
        requests,
        nonces_free,
        open,
        hold,
        bids,
        mint: hit.mint,
    }
}

fn none(_: &CreateHit, _: &mut CurveState) -> Vec<FeedEvent> {
    vec![]
}

/// An unrelated mint's trade at `slot` (moves the hot loop's slot clock).
fn tick_at(slot: u64) -> impl Fn(&CreateHit, &mut CurveState) -> Vec<FeedEvent> {
    move |_, _| {
        let mut other = CurveState::fresh(Pubkey::new_unique());
        vec![ev(
            slot,
            Payload::Trade(trade(
                &mut other,
                Pubkey::new_unique(),
                Pubkey::new_unique(),
                slot,
                500,
                10_000_000,
                0,
            )),
        )]
    }
}

fn label(o: &Outcome) -> Vec<&'static str> {
    o.decisions.iter().map(|d| decision_label(d.0)).collect()
}

#[test]
fn on_the_complete_run_waits_for_the_verdict_and_fires_on_the_first_coin() {
    // no verdict yet: parked, nothing sent, the request out with the uri
    let o = run(TweetGate::On, true, None, &none);
    assert!(!o.fired && o.decisions.is_empty(), "{:?}", label(&o));
    assert_eq!(o.requests.len(), 1);
    let (_, _, hit) = fixture_hit();
    let r = &o.requests[0];
    assert_eq!((r.mint, r.slot, r.index), (hit.mint, SLOT, CI));
    assert_eq!(Some(r.uri()), hit.tx_shape.uri_str());
    // first: fired at the exact post-bundle bound
    let o = run(TweetGate::On, true, Some(TweetKind::First), &none);
    assert!(o.fired, "{:?}", label(&o));
    assert_eq!(label(&o), ["fire"]);
    assert_eq!((o.nonces_free, o.open), (7, 1));
}

#[test]
fn verdicts_that_do_not_pass_skip_with_no_nonce_or_risk_taken() {
    for (k, want) in [
        (TweetKind::Copy, code::TWEET_COPY),
        (TweetKind::Stale, code::TWEET_STALE),
        (TweetKind::NoTweet, code::TWEET_NONE),
        (TweetKind::Error, code::TWEET_ERROR),
    ] {
        let o = run(TweetGate::On, true, Some(k), &none);
        assert!(!o.fired, "{k:?}");
        assert_eq!(label(&o), [decision_label(want)], "{k:?}");
        assert_eq!((o.nonces_free, o.open), (8, 0), "{k:?}");
    }
}

#[test]
fn a_pass_may_fire_a_slot_late_and_no_verdict_by_then_is_late() {
    // slot + 1: still waiting; the pass fires there
    let o = run(
        TweetGate::On,
        true,
        Some(TweetKind::First),
        &tick_at(SLOT + 1),
    );
    assert!(o.fired, "{:?}", label(&o));
    assert_eq!(label(&o), ["fire"]);
    // slot + 1 then slot + 2 with no verdict: skip_tweet_late
    let o = run(
        TweetGate::On,
        true,
        None,
        &|h: &CreateHit, c: &mut CurveState| {
            let mut v = tick_at(SLOT + 1)(h, c);
            v.extend(tick_at(SLOT + 2)(h, c));
            v
        },
    );
    assert!(!o.fired);
    assert_eq!(label(&o), ["skip_tweet_late"]);
    assert_eq!((o.nonces_free, o.open), (8, 0));
}

#[test]
fn an_outside_buy_in_the_next_slot_before_the_verdict_is_not_first() {
    let o = run(
        TweetGate::On,
        true,
        Some(TweetKind::First),
        &|h: &CreateHit, c: &mut CurveState| {
            let mut t = trade(
                c,
                h.mint,
                Pubkey::new_unique(),
                SLOT + 1,
                700,
                200_000_000,
                0,
            );
            t.index = 3;
            vec![FeedEvent {
                src: FeedSrc::Grpc,
                ..ev(SLOT + 1, Payload::Trade(t))
            }]
        },
    );
    assert!(!o.fired);
    assert_eq!(label(&o), ["skip_not_first"]);
}

#[test]
fn a_create_the_lane_was_not_asked_about_is_skipped_before_the_park() {
    let o = run(TweetGate::On, false, None, &none);
    assert!(o.requests.is_empty());
    assert!(!o.fired);
    assert_eq!(label(&o), ["skip_tweet_none"]);
}

#[test]
fn log_mode_trades_as_without_the_check() {
    let o = run(TweetGate::Log, true, None, &none);
    assert_eq!(o.requests.len(), 1, "log still asks");
    assert!(o.fired, "{:?}", label(&o));
    assert_eq!(label(&o), ["fire"]);
    // a copy verdict after the fire changes nothing
    let o = run(TweetGate::Log, true, Some(TweetKind::Copy), &none);
    assert!(o.fired);
    assert_eq!(label(&o), ["fire"]);
}

#[test]
fn exit_mode_fires_without_waiting_and_a_copy_verdict_cuts_the_hold() {
    // no verdict: fired at the run, the default hold (create + 10) kept
    let o = run(TweetGate::Exit, true, None, &none);
    assert_eq!(o.requests.len(), 1, "exit still asks");
    assert!(o.fired, "{:?}", label(&o));
    assert_eq!(label(&o), ["fire"]);
    assert_eq!(o.hold, Some(10));
    // a copy (or stale, or no tweet) after the fire: sold at this slot's close
    for k in [TweetKind::Copy, TweetKind::Stale, TweetKind::NoTweet] {
        let o = run(TweetGate::Exit, true, Some(k), &none);
        assert!(o.fired, "{k:?}");
        assert_eq!(
            o.hold,
            Some(1),
            "{k:?}: cut to create + 1 (the verdict came in the create slot)"
        );
    }
    // the first coin, or a failed fetch, keeps its hold
    for k in [TweetKind::First, TweetKind::Error] {
        assert_eq!(
            run(TweetGate::Exit, true, Some(k), &none).hold,
            Some(10),
            "{k:?}"
        );
    }
}

#[test]
fn exit_mode_skips_a_copy_whose_verdict_came_before_the_buy() {
    let o = run_full(TweetGate::Exit, true, Some(TweetKind::Copy), true, &none);
    assert!(!o.fired);
    assert_eq!(label(&o), ["skip_tweet_copy"]);
    assert_eq!((o.nonces_free, o.open), (8, 0));
    // an early first verdict changes nothing: fired, hold kept
    let o = run_full(TweetGate::Exit, true, Some(TweetKind::First), true, &none);
    assert!(o.fired);
    assert_eq!(o.hold, Some(10));
}

#[test]
fn the_bid_ab_sends_the_odd_mints_on_the_second_arm() {
    let run = |ab: Vec<u64>| {
        run_full_with(
            TweetGate::Off,
            true,
            None,
            false,
            &none,
            &move |c: &mut Config| {
                c.bid.lamports = 3_000_000;
                c.bid.ab_by_tier = ab.clone();
            },
            None,
        )
    };
    let base = run(vec![]);
    assert!(
        base.fired && base.bids.iter().all(|&b| b == 3_000_000),
        "{:?}",
        base.bids
    );
    let ab = run(vec![1_500_000, 3_000_000, 3_000_000]);
    let odd = ab.mint.as_ref()[31] & 1 == 1;
    let want = if odd { 1_500_000 } else { 3_000_000 };
    assert!(
        ab.fired && ab.bids.iter().all(|&b| b == want),
        "odd {odd}: {:?}",
        ab.bids
    );
}

/// One post received 4 s ago whose blob is `key`, already taken by an earlier
/// coin or not.
fn feed_with(key: &[u8], taken: bool) -> Posts {
    let now = rugsnipe::now_ms();
    let pub_ms = now - 5_000;
    let p = Post {
        id: (pub_ms - 1_288_834_974_657) << 22,
        pub_ms,
        rx_ms: now - 4_000,
        blob: key.to_vec().into_boxed_slice(),
        used: std::sync::atomic::AtomicBool::new(taken),
    };
    let posts = new_posts();
    posts.store(Arc::new(vec![Arc::new(p)]));
    posts
}

#[test]
fn the_tweet_feed_on_skips_a_copy_and_log_only_journals() {
    let (_, _, hit) = fixture_hit();
    let key = if hit.tx_shape.sym_key().len() >= 3 {
        hit.tx_shape.sym_key().to_vec()
    } else {
        hit.tx_shape.name_key().to_vec()
    };
    assert!(key.len() >= 3, "the fixture has a ticker or name to match");
    let run = |mode: TweetFeed, taken: bool| {
        run_full_with(
            TweetGate::Off,
            true,
            None,
            false,
            &none,
            &move |c: &mut Config| c.entry.tweet_feed = mode,
            Some(feed_with(&key, taken)),
        )
    };
    // on: the post was taken by an earlier coin, so this one is a copy
    let o = run(TweetFeed::On, true);
    assert!(
        !o.fired && o.decisions.iter().any(|d| d.0 == code::TWEETFEED_COPY),
        "{:?}",
        label(&o)
    );
    // on: a fresh post makes it the first coin, which fires
    assert!(run(TweetFeed::On, false).fired);
    // log: a copy call never stops the buy
    assert!(run(TweetFeed::Log, true).fired);
}
