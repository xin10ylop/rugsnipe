//! `entry.copy` (docs/17 section 9): a landed buy by the copied wallet
//! (`entry.follow_wallet`) of at least `copy_min_sol` in a launch's create
//! slot or the next, on a mint we hold nothing in, is bought right behind
//! it: bound at his post-trade price plus `copy_max_premium`, sent at most
//! `copy_late_slots` after the create, past the rule's own gates. `log`
//! journals the copy and buys nothing.

mod common;

use arc_swap::ArcSwap;
use common::*;
use pump_codec::create_hit::CreateHit;
use pump_codec::curve::FeeBps;
use pump_codec::Pubkey;
use rugsnipe::copybuys::SignerBuys;
use rugsnipe::hot::{entry_v_cap, HotCfg, HotLoop, HotParts, Shared};
use rugsnipe::msgs::{ColdMsg, FireJob};
use rugsnipe::setup::{self, ArmState};
use rugsnipe::stats::{code, decision_label, HotStats};
use rugsnipe_core::config::{ArmSetGate, Config, CopyMode, ValidateEnv};
use rugsnipe_core::risk::RiskGate;
use rugsnipe_core::rslog::{box_id, Rec, RslogWriter};
use rugsnipe_core::watchlist::Watchlist;
use rugsnipe_core::IntentId;
use rugsnipe_feed::{
    FeedBuf, FeedEvent, FeedMux, FeedSrc, FeedStatus, FiredSet, Payload, TradeLite,
};
use rugsnipe_tx::senders::{ApiKeys, SenderSet};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

const SLOT: u64 = 5_000;
const CI: u32 = 40;
const SOL: u64 = 1_000_000_000;

fn live_cfg(dir: &std::path::Path, copy: CopyMode, ranges: &[[f64; 2]], his: Pubkey) -> Config {
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
    c.entry.copy = copy;
    c.entry.follow_wallet = his.to_string();
    // most tests have no operator table and one launch: copy any signer,
    // with no history of his on it
    c.entry.copy_listed_only = false;
    c.entry.copy_signer_min_buys = 0;
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

/// A plain create (its dev buy shown) as the gRPC feed delivers it.
fn the_create() -> (Vec<u8>, Vec<u8>, CreateHit) {
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
/// misses (the rule skips the create as `skip_dev_buy`).
fn template(hit: &CreateHit, on: bool) -> Vec<[f64; 2]> {
    let d = hit.intx_sol_in as f64 / 1e9;
    if on {
        vec![[d - 0.01, d + 0.01]]
    } else {
        vec![[d + 10.0, d + 11.0]]
    }
}

/// A landed buy of `sol` by `user` at `(slot, index)` whose post-trade
/// virtual SOL reserve is the create's plus `before + sol`.
fn buy(hit: &CreateHit, user: Pubkey, sol: u64, before: u64, slot: u64, index: u32) -> TradeLite {
    let (v0, t0) = rugsnipe::convert::v0_of(hit);
    let v = v0 + before + sol;
    TradeLite {
        mint: hit.mint,
        user,
        is_buy: true,
        sol_amount: sol,
        token_amount: t0 - (v0 as u128 * t0 as u128 / v as u128) as u64,
        fee: 0,
        virtual_sol_reserves: v,
        virtual_token_reserves: (v0 as u128 * t0 as u128 / v as u128) as u64,
        slot,
        sig: sig(1_000 + index as u64 + slot),
        ordinal: 0,
        index,
        tip: 0,
    }
}

struct Outcome {
    /// Buy jobs handed to the sender for the launch.
    fires: usize,
    /// The launch's decision records: (code, size, v_max, min_tokens).
    decisions: Vec<(u8, u64, u64, u64)>,
    copy: [u64; 4],
    /// His launch buys and the ones not copied: size or tip, unlisted, few.
    copy_his: [u64; 4],
    /// His buys the loop counted, as handed to the cold thread (signer, slot).
    his_buys: Vec<(Pubkey, u64)>,
}

impl Outcome {
    fn codes(&self) -> Vec<u8> {
        self.decisions.iter().map(|d| d.0).collect()
    }
    fn labels(&self) -> Vec<&'static str> {
        self.decisions.iter().map(|d| decision_label(d.0)).collect()
    }
}

/// The create, then `trades(hit, his wallet)` in order on the gRPC feed.
fn run(
    copy: CopyMode,
    on_template: bool,
    tweak: &dyn Fn(&mut Config),
    trades: &dyn Fn(&CreateHit, Pubkey) -> Vec<TradeLite>,
) -> (Outcome, CreateHit) {
    run_table(copy, on_template, tweak, trades, None, None)
}

/// [`run`] with an operator table listing `listed(hit)` at hold 6, and the
/// loop seeded with his buys per signer an earlier run kept (`seed`).
fn run_table(
    copy: CopyMode,
    on_template: bool,
    tweak: &dyn Fn(&mut Config),
    trades: &dyn Fn(&CreateHit, Pubkey) -> Vec<TradeLite>,
    listed: Option<&dyn Fn(&CreateHit) -> Pubkey>,
    seed: Option<&SignerBuys>,
) -> (Outcome, CreateHit) {
    let (tx, meta, hit) = the_create();
    assert!(hit.intx_sol_in > 0);
    let dir = tmpdir("copy");
    let his = Pubkey::new_unique();
    let mut cfg = live_cfg(&dir, copy, &template(&hit, on_template), his);
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
    let (cold_tx, mut cold_rx) = rtrb::RingBuffer::new(4_096);
    let (_fills_tx, fills_rx) = rtrb::RingBuffer::new(64);
    let (fire_tx, mut fire_rx) = rtrb::RingBuffer::<FireJob>::new(64);
    let mut reg = prometheus_client::registry::Registry::default();
    let stats = Arc::new(HotStats::new(&mut reg));
    let mut hc = HotCfg::from_config(&cfg, setup::exit_policy(&cfg).unwrap());
    if let Some(f) = listed {
        hc.operators = Some(Arc::new(
            rugsnipe::operators::OperatorTable::from_json(&format!(
                r#"{{"{}": {{"hold_slots": 6}}}}"#,
                f(&hit)
            ))
            .unwrap(),
        ));
    }
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
    if let Some(b) = seed {
        hot.seed_his_signers(b);
    }
    let events = trades(&hit, his)
        .into_iter()
        .map(|t| FeedEvent {
            src: FeedSrc::Grpc,
            ..ev(t.slot, Payload::Trade(t))
        })
        .collect();
    let mut feed = FeedMux::new(vec![
        Box::new(WireFeed::new(vec![(SLOT, tx, meta)])),
        Box::new(VecFeed::new(events)),
    ]);
    let mut buf = FeedBuf::with_capacity(64, 1 << 16);
    let t_start = Instant::now();
    while hot.step(&mut feed, &mut buf) != FeedStatus::Ended {
        assert!(t_start.elapsed() < Duration::from_secs(10));
    }
    // a park still waiting for the rest of its run fires once the wait is
    // over (the loop's own timer)
    let wait = Duration::from_millis(cfg.entry.bundle_wait_ms + 50);
    let t_end = Instant::now();
    while t_end.elapsed() < wait {
        hot.step(&mut feed, &mut buf);
    }
    let intent = IntentId::new(&hit.mint, &owner).0;
    let fires = std::iter::from_fn(|| fire_rx.pop().ok())
        .filter(|j| j.intent == intent && j.idx == 0)
        .count();
    let copy = hot.counters.copy;
    let copy_his = hot.counters.copy_his;
    let his_buys = std::iter::from_fn(|| cold_rx.pop().ok())
        .filter_map(|m| match m {
            ColdMsg::HisBuy { signer, slot } => Some((signer, slot)),
            _ => None,
        })
        .collect();
    drop(hot);
    w.join().unwrap();
    let decisions = rslog_records(&dir.join("rslog"))
        .iter()
        .filter_map(|r| match r {
            Rec::Decision {
                intent: i,
                code,
                size,
                v_max,
                min_tokens,
                ..
            } if *i == intent => Some((*code, *size, *v_max, *min_tokens)),
            _ => None,
        })
        .collect();
    let _ = std::fs::remove_dir_all(&dir);
    (
        Outcome {
            fires,
            decisions,
            copy,
            copy_his,
            his_buys,
        },
        hit,
    )
}

fn no_tweak(_: &mut Config) {}

#[test]
fn on_buys_his_landed_buy_on_a_coin_the_rule_skips() {
    // the rule skips the create (off its dev-buy template); his 1 SOL buy
    // lands later in the create slot behind someone else's 2 SOL
    let (o, hit) = run(CopyMode::On, false, &no_tweak, &|h, his| {
        vec![
            buy(h, Pubkey::new_unique(), 2 * SOL, 0, SLOT, CI + 60),
            buy(h, his, SOL, 2 * SOL, SLOT, CI + 120),
        ]
    });
    assert_eq!(o.fires, 1, "{:?}", o.labels());
    assert_eq!(o.codes(), vec![code::DEV_BUY, 0], "{:?}", o.labels());
    assert_eq!(o.copy, [1, 1, 0, 0]);
    // bound at his post-trade price + copy_max_premium (10%), not the exact
    // state: the rule's max_premium = 0 binds only its own fires
    let (_, size, v_max, min_tokens) = o.decisions[1];
    let (v0, _) = rugsnipe::convert::v0_of(&hit);
    let his_v = v0 + 3 * SOL;
    let want = setup::min_tokens_out(
        size,
        entry_v_cap(his_v, 1_000).min(v_max),
        hit.creator_arg,
        FeeBps::DEFAULT,
    );
    assert_eq!(min_tokens, want);
    let exact = setup::min_tokens_out(size, his_v, hit.creator_arg, FeeBps::DEFAULT);
    assert!(min_tokens < exact, "{min_tokens} {exact}");
}

#[test]
fn a_small_buy_another_wallet_or_a_late_one_is_not_copied() {
    let small = |h: &CreateHit, his| vec![buy(h, his, SOL / 2, 0, SLOT, CI + 120)];
    let (o, _) = run(CopyMode::On, false, &no_tweak, &small);
    assert_eq!((o.fires, o.codes()), (0, vec![code::DEV_BUY]));
    assert_eq!((o.copy, o.copy_his), ([0; 4], [1, 1, 0, 0]));
    let other = |h: &CreateHit, _| vec![buy(h, Pubkey::new_unique(), SOL, 0, SLOT, CI + 120)];
    let (o, _) = run(CopyMode::On, false, &no_tweak, &other);
    assert_eq!((o.fires, o.copy), (0, [0; 4]));
    // his buy two slots after the create: not his launch-time buy
    let late = |h: &CreateHit, his| vec![buy(h, his, SOL, 0, SLOT + 2, 3)];
    let (o, _) = run(CopyMode::On, false, &no_tweak, &late);
    assert_eq!((o.fires, o.copy), (0, [0; 4]));
    // the slot after the create is copied
    let next = |h: &CreateHit, his| vec![buy(h, his, SOL, 0, SLOT + 1, 3)];
    let (o, _) = run(CopyMode::On, false, &no_tweak, &next);
    assert_eq!(o.fires, 1, "{:?}", o.labels());
    assert_eq!(o.copy, [1, 1, 0, 0]);
    // off: nothing
    let (o, _) = run(CopyMode::Off, false, &no_tweak, &next);
    assert_eq!((o.fires, o.copy), (0, [0; 4]));
}

#[test]
fn a_copy_goes_out_no_later_than_copy_late_slots_after_the_create() {
    // the feed shows create + 3 before his create + 1 buy comes in
    let (o, _) = run(CopyMode::On, false, &no_tweak, &|h, his| {
        vec![
            buy(h, Pubkey::new_unique(), SOL / 10, 0, SLOT + 3, 2),
            buy(h, his, SOL, SOL / 10, SLOT + 1, 3),
        ]
    });
    assert_eq!(o.fires, 0, "{:?}", o.labels());
    assert_eq!(o.copy, [1, 1, 0, 0]);
    assert_eq!(
        o.codes(),
        vec![code::DEV_BUY, code::STALE],
        "{:?}",
        o.labels()
    );
    // a window of 4 slots takes it
    let (o, _) = run(
        CopyMode::On,
        false,
        &|c| c.entry.copy_late_slots = 4,
        &|h, his| {
            vec![
                buy(h, Pubkey::new_unique(), SOL / 10, 0, SLOT + 3, 2),
                buy(h, his, SOL, SOL / 10, SLOT + 1, 3),
            ]
        },
    );
    assert_eq!(o.fires, 1, "{:?}", o.labels());
}

#[test]
fn log_journals_the_copy_and_buys_nothing() {
    let (o, _) = run(CopyMode::Log, false, &no_tweak, &|h, his| {
        vec![
            buy(h, his, SOL, 0, SLOT, CI + 120),
            // a second buy of his on the same coin is journaled once
            buy(h, his, SOL, SOL, SLOT, CI + 130),
        ]
    });
    assert_eq!(o.fires, 0);
    assert_eq!(o.codes(), vec![code::DEV_BUY]);
    assert_eq!(o.copy, [2, 0, 1, 1]);
}

#[test]
fn a_coin_the_rule_bought_is_never_bought_again() {
    // on the template: the rule fires behind the bundle buy, his later buy
    // is passed over
    let (o, _) = run(CopyMode::On, true, &no_tweak, &|h, his| {
        vec![
            buy(h, Pubkey::new_unique(), SOL, 0, SLOT, CI + 1),
            buy(h, his, SOL, SOL, SLOT, CI + 120),
        ]
    });
    assert_eq!(o.fires, 1, "{:?}", o.labels());
    assert_eq!(o.codes(), vec![0], "{:?}", o.labels());
    assert_eq!(o.copy, [1, 0, 0, 1]);
}

#[test]
fn his_buy_ends_a_park_it_beats_and_is_copied() {
    // the park waits for the rest of the bundle run; his buy beyond the run
    // lands before the fire: the rule's buy could only be skip_not_first,
    // the copy buys behind him
    let (o, _) = run(
        CopyMode::On,
        true,
        &|c| c.entry.bundle_wait_ms = 150,
        &|h, his| {
            vec![
                buy(h, Pubkey::new_unique(), SOL, 0, SLOT, CI + 1),
                buy(h, his, SOL, SOL, SLOT, CI + 120),
            ]
        },
    );
    assert_eq!(o.fires, 1, "{:?}", o.labels());
    assert_eq!(o.codes(), vec![code::NOT_FIRST, 0], "{:?}", o.labels());
    assert_eq!(o.copy, [1, 1, 0, 0]);
    // copy off: the park ends skip_not_first, nothing is bought
    let (o, _) = run(
        CopyMode::Off,
        true,
        &|c| c.entry.bundle_wait_ms = 150,
        &|h, his| {
            vec![
                buy(h, Pubkey::new_unique(), SOL, 0, SLOT, CI + 1),
                buy(h, his, SOL, SOL, SLOT, CI + 120),
            ]
        },
    );
    assert_eq!(o.fires, 0, "{:?}", o.labels());
    assert_eq!(o.codes(), vec![code::NOT_FIRST], "{:?}", o.labels());
}

#[test]
fn a_park_whose_run_holds_his_buy_fires_behind_him_itself() {
    // his buy chained right behind the bundle is a bundle buy to the park:
    // the rule fires bound at his post-trade state, no second buy
    let (o, _) = run(
        CopyMode::On,
        true,
        &|c| c.entry.bundle_wait_ms = 150,
        &|h, his| {
            vec![
                buy(h, Pubkey::new_unique(), SOL, 0, SLOT, CI + 1),
                buy(h, his, SOL, SOL, SLOT, CI + 2),
            ]
        },
    );
    assert_eq!(o.fires, 1, "{:?}", o.labels());
    assert_eq!(o.codes(), vec![0], "{:?}", o.labels());
    assert_eq!(o.copy[0..2], [1, 0]);
}

#[test]
fn a_copy_is_not_held_to_the_rules_template_caps() {
    // the rule's own fire is skipped as too big (post-bundle above 31 SOL);
    // his buy behind it is copied all the same
    let (o, _) = run(
        CopyMode::On,
        true,
        &|c| c.entry.max_post_bundle_sol = 31.0,
        &|h, his| {
            vec![
                buy(h, Pubkey::new_unique(), SOL, 0, SLOT, CI + 1),
                buy(h, his, SOL, SOL, SLOT, CI + 120),
            ]
        },
    );
    assert_eq!(o.fires, 1, "{:?}", o.labels());
    assert_eq!(o.codes(), vec![code::TOO_BIG, 0], "{:?}", o.labels());
}

#[test]
fn copy_needs_the_wallet_the_bundle_feed_and_sane_bounds() {
    let dir = tmpdir("copy-cfg");
    let his = Pubkey::new_unique();
    let c = live_cfg(&dir, CopyMode::On, &[], his);
    assert!(c.validate(&good_env(&c)).ok());
    let mut c = live_cfg(&dir, CopyMode::Log, &[], his);
    c.entry.follow_wallet = "not-a-key".into();
    assert!(!c.validate(&good_env(&c)).ok());
    let mut c = live_cfg(&dir, CopyMode::On, &[], his);
    c.feeds.grpc.bundle_run = false;
    assert!(!c.validate(&good_env(&c)).ok());
    let mut c = live_cfg(&dir, CopyMode::On, &[], his);
    c.entry.copy_max_premium = 0.5;
    assert!(!c.validate(&good_env(&c)).ok());
    let mut c = live_cfg(&dir, CopyMode::On, &[], his);
    c.entry.copy_late_slots = 0;
    assert!(!c.validate(&good_env(&c)).ok());
    let mut c = live_cfg(&dir, CopyMode::On, &[], his);
    c.entry.copy_min_sol = 100.5;
    assert!(!c.validate(&good_env(&c)).ok());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_copy_sells_one_slot_before_his_hold_for_the_signer() {
    let dir = tmpdir("copy-exit");
    let mut cfg = live_cfg(&dir, CopyMode::On, &[], Pubkey::new_unique());
    cfg.entry.operator_holds = false;
    // [exit.default] hold_to_create_slots = 9
    let mut policy = setup::exit_policy(&cfg).unwrap();
    policy.hold_to_create_slot = Some(9);
    let hc = HotCfg::from_config(&cfg, policy);
    let rule = rugsnipe::operators::OpRule {
        hold_slots: 6,
        n: 10,
        wins: 6,
        pnl_sol: 1.0,
        dev_sol: 1.0,
        bundle_wallets: 3,
        stake_tier: 1,
    };
    // a rule fire with operator_holds off: the flat +9; a copy: his 6 - 1
    assert_eq!(hc.policy_for(Some(rule)).hold_to_create_slot, Some(9));
    assert_eq!(hc.copy_policy_for(Some(rule)).hold_to_create_slot, Some(5));
    // an unlisted signer's copy: like any position
    assert_eq!(hc.copy_policy_for(None).hold_to_create_slot, Some(9));
    let one = rugsnipe::operators::OpRule {
        hold_slots: 1,
        ..rule
    };
    assert_eq!(hc.copy_policy_for(Some(one)).hold_to_create_slot, Some(1));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn listed_only_copies_his_known_operators_only() {
    let listed_only = |c: &mut Config| {
        c.entry.copy_listed_only = true;
        c.entry.operators_file = "/unused/operators.json".into();
    };
    let his_buy = |h: &CreateHit, his| vec![buy(h, his, SOL, 0, SLOT, CI + 120)];
    // the create's signer is in the table: copied
    let (o, _) = run_table(
        CopyMode::On,
        false,
        &listed_only,
        &his_buy,
        Some(&|h: &CreateHit| rugsnipe::operators::create_signer(h)),
        None,
    );
    assert_eq!(o.fires, 1, "{:?}", o.labels());
    assert_eq!(o.copy, [1, 1, 0, 0]);
    // another signer is: not a candidate
    let (o, _) = run_table(
        CopyMode::On,
        false,
        &listed_only,
        &his_buy,
        Some(&|_: &CreateHit| Pubkey::new_unique()),
        None,
    );
    assert_eq!((o.fires, o.copy), (0, [0; 4]), "{:?}", o.labels());
    assert_eq!(o.copy_his, [1, 0, 1, 0]);
    // listed_only needs the table
    let dir = tmpdir("copy-listed");
    let mut c = live_cfg(&dir, CopyMode::On, &[], Pubkey::new_unique());
    c.entry.copy_listed_only = true;
    assert!(!c.validate(&good_env(&c)).ok());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_tip_floor_copies_his_high_conviction_buys_only() {
    let tip_floor = |c: &mut Config| {
        c.entry.copy_min_sol = 0.0;
        c.entry.copy_min_tip_lamports = 4_000_000;
    };
    let tipped = |tip: u64| {
        move |h: &CreateHit, his: Pubkey| {
            let mut t = buy(h, his, SOL / 2, 0, SLOT, CI + 120);
            t.tip = tip;
            vec![t]
        }
    };
    // a 0.5 SOL buy with a 5M tip: copied (no size floor)
    let (o, _) = run(CopyMode::On, false, &tip_floor, &tipped(5_000_000));
    assert_eq!(o.fires, 1, "{:?}", o.labels());
    assert_eq!(o.copy, [1, 1, 0, 0]);
    // a 3M tip: not a candidate
    let (o, _) = run(CopyMode::On, false, &tip_floor, &tipped(3_000_000));
    assert_eq!((o.fires, o.copy), (0, [0; 4]), "{:?}", o.labels());
    assert_eq!(o.copy_his, [1, 1, 0, 0]);
    // the floor is at most 1 SOL
    let dir = tmpdir("copy-tip");
    let mut c = live_cfg(&dir, CopyMode::On, &[], Pubkey::new_unique());
    c.entry.copy_min_tip_lamports = 2_000_000_000;
    assert!(!c.validate(&good_env(&c)).ok());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_signer_he_is_not_trading_now_is_not_copied() {
    // one earlier buy of his on the signer needed: his first buy only counts
    let once = |c: &mut Config| c.entry.copy_signer_min_buys = 1;
    let (o, _) = run(CopyMode::On, false, &once, &|h, his| {
        vec![buy(h, his, SOL, 0, SLOT, CI + 120)]
    });
    assert_eq!((o.fires, o.copy), (0, [0; 4]), "{:?}", o.labels());
    assert_eq!(o.copy_his, [1, 0, 0, 1]);
    // his second buy on the signer's launch is copied
    let (o, _) = run(CopyMode::On, false, &once, &|h, his| {
        vec![
            buy(h, his, SOL, 0, SLOT, CI + 120),
            buy(h, his, SOL, SOL, SLOT, CI + 130),
        ]
    });
    assert_eq!(o.fires, 1, "{:?}", o.labels());
    assert_eq!(o.copy, [1, 1, 0, 0]);
    // an earlier buy within a 1 h window counts too
    let (o, _) = run(
        CopyMode::On,
        false,
        &|c: &mut Config| {
            c.entry.copy_signer_min_buys = 1;
            c.entry.copy_signer_window_h = 1;
        },
        &|h, his| {
            vec![
                buy(h, his, SOL, 0, SLOT, CI + 120),
                buy(h, his, SOL, SOL, SLOT + 1, 5),
            ]
        },
    );
    assert_eq!(o.fires, 1, "same window: {:?}", o.labels());
    let dir = tmpdir("copy-signer");
    let mut c = live_cfg(&dir, CopyMode::On, &[], Pubkey::new_unique());
    c.entry.copy_signer_min_buys = 9;
    assert!(!c.validate(&good_env(&c)).ok());
    let mut c = live_cfg(&dir, CopyMode::On, &[], Pubkey::new_unique());
    c.entry.copy_signer_window_h = 0;
    assert!(!c.validate(&good_env(&c)).ok());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_restart_keeps_his_buys_per_signer() {
    let two = |c: &mut Config| c.entry.copy_signer_min_buys = 2;
    // his first two buys on the signer's launches: counted, not copied
    let (o, hit) = run(CopyMode::On, false, &two, &|h, his| {
        vec![
            buy(h, his, SOL, 0, SLOT, CI + 120),
            buy(h, his, SOL, SOL, SLOT, CI + 130),
        ]
    });
    assert_eq!(o.fires, 0, "{:?}", o.labels());
    let signer = rugsnipe::operators::create_signer(&hit);
    assert_eq!(o.his_buys, vec![(signer, SLOT), (signer, SLOT)]);
    // the cold thread keeps them and saves them; the restart loads them
    let mut kept = SignerBuys::default();
    for &(s, slot) in &o.his_buys {
        kept.note(s, slot);
    }
    let dir = tmpdir("copy-restart");
    let path = dir.join("copy_signers.json");
    kept.save(&path).unwrap();
    let mut loaded = SignerBuys::default();
    assert_eq!(loaded.load(&path).unwrap(), 1);
    // after the restart his third buy on the signer is copied
    let third = |h: &CreateHit, his| vec![buy(h, his, SOL, 0, SLOT, CI + 120)];
    let (o, _) = run_table(CopyMode::On, false, &two, &third, None, Some(&loaded));
    assert_eq!(o.fires, 1, "{:?}", o.labels());
    assert_eq!((o.copy, o.copy_his), ([1, 1, 0, 0], [1, 0, 0, 0]));
    // a start without them counts from empty: not copied
    let (o, _) = run(CopyMode::On, false, &two, &third);
    assert_eq!((o.fires, o.copy), (0, [0; 4]), "{:?}", o.labels());
    assert_eq!(o.copy_his, [1, 0, 0, 1]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rule_buys_off_buys_his_copies_only() {
    let copy_only = |c: &mut Config| c.entry.rule_buys = false;
    // on the template: the rule would fire behind the bundle buy, but it is
    // off; his buy is copied
    let (o, _) = run(CopyMode::On, true, &copy_only, &|h, his| {
        vec![
            buy(h, Pubkey::new_unique(), SOL, 0, SLOT, CI + 1),
            buy(h, his, SOL, SOL, SLOT, CI + 120),
        ]
    });
    assert_eq!(o.fires, 1, "{:?}", o.labels());
    assert_eq!(o.codes(), vec![code::RULE_OFF, 0], "{:?}", o.labels());
    assert_eq!(o.copy, [1, 1, 0, 0]);
    // no buy of his: nothing is bought
    let (o, _) = run(CopyMode::On, true, &copy_only, &|h, _| {
        vec![buy(h, Pubkey::new_unique(), SOL, 0, SLOT, CI + 1)]
    });
    assert_eq!(o.fires, 0, "{:?}", o.labels());
    assert_eq!(o.labels(), vec!["skip_rule_off"]);
    // the rule off and the copy not buying: valid, with a warning
    let dir = tmpdir("copy-rule-off");
    let mut c = live_cfg(&dir, CopyMode::Log, &[], Pubkey::new_unique());
    c.entry.rule_buys = false;
    let v = c.validate(&good_env(&c));
    assert!(v.ok(), "{:?}", v.refusals);
    assert!(
        v.warnings.iter().any(|w| w.contains("buys nothing")),
        "{:?}",
        v.warnings
    );
    let _ = std::fs::remove_dir_all(&dir);
}
