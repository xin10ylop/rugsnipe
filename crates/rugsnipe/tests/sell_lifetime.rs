//! A real sell must go out when the real lane's blockhash is stale
//! (2026-10-23: two durable-nonce buys could not be sold for 17 minutes,
//! `sells_no_blockhash: 2`, journal `try 1..3` then silence):
//!
//! * first choice a sell-pool durable nonce (claimed once per position,
//!   reused by its retries), second the sender thread's bounded blockhash
//!   fetch ([`BlockhashResolver`], 300 ms in the binary);
//! * with neither, the attempt is taken back and retried every slot (no try
//!   used), and after `HANDOFF_MS` the position is handed to the guard;
//! * buys pause (`skip_no_blockhash`) while no blockhash is fresh and the
//!   sell pool is empty, and resume by themselves;
//! * a position recovery re-adopts keeps the store's cost and opened slot.

mod common;

use arc_swap::ArcSwap;
use common::*;
use pump_codec::Pubkey;
use rugsnipe::hot::{HotCfg, HotLoop, HotParts, Known, Shared, NO_LIFETIME};
use rugsnipe::msgs::{ColdMsg, FireJob, HotIn};
use rugsnipe::sender::{resolve_lifetime, BlockhashResolver};
use rugsnipe::setup::{self, ArmState};
use rugsnipe::stats::HotStats;
use rugsnipe_core::config::Config;
use rugsnipe_core::risk::RiskGate;
use rugsnipe_core::rslog::{box_id, RslogWriter};
use rugsnipe_core::watchlist::Watchlist;
use rugsnipe_core::Mode;
use rugsnipe_feed::{Feed, FeedBuf, FeedSrc, FeedStatus, FiredSet};
use rugsnipe_tx::nonce::NoncePool;
use rugsnipe_tx::senders::{ApiKeys, SenderSet};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A feed that only moves the slot clock.
struct SlotFeed(u64);

impl Feed for SlotFeed {
    fn poll(&mut self, _out: &mut FeedBuf) -> FeedStatus {
        FeedStatus::Idle
    }
    fn src(&self) -> FeedSrc {
        FeedSrc::Grpc
    }
    fn slot(&self) -> u64 {
        self.0
    }
}

struct Rig {
    hot: HotLoop,
    shared: Arc<Shared>,
    fire_rx: rtrb::Consumer<FireJob>,
    cold_rx: rtrb::Consumer<ColdMsg>,
    fills_tx: rtrb::Producer<HotIn>,
    feed: SlotFeed,
    buf: FeedBuf,
    owner: Pubkey,
    cfg: Config,
    _w: RslogWriter,
}

impl Rig {
    /// A live hot loop (every gate assembled by hand, paper senders) whose
    /// exit sells a held position two slots after its opening slot.
    fn new(dir: &std::path::Path, resolver: bool, tune: impl FnOnce(&mut HotCfg)) -> Rig {
        let mut cfg: Config = paper_cfg(dir);
        cfg.mode.mode = Mode::Live;
        cfg.tx.cu_limit = 88_000;
        cfg.tx.lads_bytes = 12_124_160;
        // paper senders only: real jobs stop at the fire ring here
        let mut pc = cfg.clone();
        pc.mode.mode = Mode::Paper;
        let senders =
            Arc::new(SenderSet::build(&pc, &Default::default(), &ApiKeys::default()).unwrap());
        let signer = Arc::new(setup::throwaway_signer().unwrap());
        let owner = signer.pubkey();
        let nonces = Arc::new(setup::paper_nonce_pool(owner, 4));
        let templates = setup::templates(&cfg, owner, &nonces).unwrap();
        let mut policy = setup::exit_policy(&cfg).unwrap();
        policy.max_hold_slots = 2;
        let set = armset(&[], &[]);
        let watch = Arc::new(Watchlist::new());
        let arm = Arc::new(ArcSwap::from_pointee(ArmState::from_armset(&set, true, "")));
        let risk = Arc::new(RiskGate::new(cfg.risk.clone()));
        let (rslog, w) =
            RslogWriter::spawn(dir.join("rslog"), "t-hot", box_id("t"), 1 << 12).unwrap();
        let (cold_tx, cold_rx) = rtrb::RingBuffer::new(4_096);
        let (fills_tx, fills_rx) = rtrb::RingBuffer::new(64);
        let (fire_tx, fire_rx) = rtrb::RingBuffer::new(256);
        let mut reg = prometheus_client::registry::Registry::default();
        let stats = Arc::new(HotStats::new(&mut reg));
        let mut hcfg = HotCfg::from_config(&cfg, policy);
        hcfg.sell_resolver = resolver;
        hcfg.tick_ms = 1;
        tune(&mut hcfg);
        let shared = Arc::new(Shared::default());
        let hot = HotLoop::new(HotParts {
            cfg: hcfg,
            watch,
            arm,
            risk,
            fired: Arc::new(FiredSet::with_capacity(1 << 10)),
            nonces,
            signer,
            templates,
            senders,
            rslog,
            cold: cold_tx,
            fills_in: fills_rx,
            fire: fire_tx,
            stats,
            shared: shared.clone(),
            leaders: rugsnipe::leader::empty_snapshot(),
            dumpers: rugsnipe::dumpers::snapshot_from(vec![]),
        });
        Rig {
            hot,
            shared,
            fire_rx,
            cold_rx,
            fills_tx,
            feed: SlotFeed(0),
            buf: FeedBuf::with_capacity(16, 1 << 12),
            owner,
            cfg,
            _w: w,
        }
    }

    fn slot(&mut self, s: u64) {
        self.feed.0 = s;
        self.hot.step(&mut self.feed, &mut self.buf);
    }

    fn jobs(&mut self) -> Vec<FireJob> {
        let mut v = vec![];
        while let Ok(j) = self.fire_rx.pop() {
            v.push(j);
        }
        v
    }

    fn cold(&mut self) -> Vec<ColdMsg> {
        let mut v = vec![];
        while let Ok(m) = self.cold_rx.pop() {
            v.push(m);
        }
        v
    }

    /// A position the wallet holds, as recovery re-adopts it.
    fn adopt(&mut self, mint: Pubkey) {
        let known = Known {
            cost: 500_000_000,
            opened_slot: 1_000,
            v_fill: 40_000_000_000,
            peak_v: 42_000_000_000,
            bundle: Default::default(),
            from_intent: true,
        };
        assert!(self.hot.adopt_known(mint, 1_000_000_000, 77, Some(known)));
    }
}

#[test]
fn recovery_adoption_keeps_the_known_cost_and_opened_slot() {
    let dir = tmpdir("sell-adopt");
    let mut r = Rig::new(&dir, false, |_| {});
    let mint = Pubkey::new_unique();
    r.adopt(mint);
    let p = r.hot.position_of(&mint).unwrap();
    assert_eq!(
        (p.cost, p.opened_slot, p.v_fill),
        (500_000_000, 1_000, 40_000_000_000)
    );
    assert_eq!(p.tokens, 1_000_000_000, "the wallet's amount");
    assert_eq!(p.origin, Some(rugsnipe_exit::Origin::Intent));
    // the last snapshot the store gets carries the known cost
    let last = r
        .cold()
        .into_iter()
        .filter_map(|m| match m {
            ColdMsg::Position(p) if p.mint == mint => Some(p),
            _ => None,
        })
        .next_back()
        .unwrap();
    assert_eq!((last.cost, last.opened_slot), (500_000_000, 1_000));
}

#[test]
fn a_stale_blockhash_sell_rides_a_sell_nonce_and_its_retries_reuse_it() {
    let dir = tmpdir("sell-nonce");
    let mut r = Rig::new(&dir, false, |_| {});
    let pool = Arc::new(NoncePool::new(
        r.owner,
        &rugsnipe_tx::sysix::pool_addresses(&r.owner, "sell", 2),
    ));
    pool.on_value(1, [0x5a; 32], 10);
    r.shared.sell_nonces.set(pool.clone()).unwrap();
    let t = setup::sell_nonce_templates(&r.cfg, r.owner, &pool).unwrap();
    r.hot.set_sell_nonce_templates(t);
    let mint = Pubkey::new_unique();
    r.adopt(mint);
    r.cold();
    for s in 1_003..1_010 {
        r.slot(s);
    }
    let jobs = r.jobs();
    assert!(
        jobs.len() >= 2,
        "a sell and its retries: {:?}",
        r.hot.counters
    );
    for j in &jobs {
        assert!(j.real);
        assert_eq!(
            j.tmpl.lifetime(),
            [0x5a; 32],
            "every try rides the one nonce value"
        );
    }
    assert_eq!(r.hot.counters.sells_no_blockhash, 0);
    assert!(r.hot.counters.sells_on_nonce >= 2);
    assert!(r.cold().iter().any(|m| matches!(
        m,
        ColdMsg::SellNonce {
            idx: 1,
            on: true,
            ..
        }
    )));
    // the sell nonce is in flight: no other free one, and no fresh blockhash
    assert_eq!(pool.free_count(), 0);
}

/// A final sell (whole balance, token account closed) fans out as one
/// variant per sender, and on a recent blockhash every copy can land: the
/// first sells, the others fail on the closed account and each pays its fee
/// (2026-10-24: three failed copies per exit). With a fresh blockhash AND a
/// free sell nonce it rides the nonce (at most one copy executes); without a
/// sell pool it still goes out on the fresh blockhash.
#[test]
fn a_final_sell_rides_the_sell_nonce_even_with_a_fresh_blockhash() {
    let dir = tmpdir("sell-nonce-first");
    let mut r = Rig::new(&dir, false, |_| {});
    let pool = Arc::new(NoncePool::new(
        r.owner,
        &rugsnipe_tx::sysix::pool_addresses(&r.owner, "sell", 2),
    ));
    pool.on_value(1, [0x5a; 32], 10);
    r.shared.sell_nonces.set(pool.clone()).unwrap();
    let t = setup::sell_nonce_templates(&r.cfg, r.owner, &pool).unwrap();
    r.hot.set_sell_nonce_templates(t);
    r.shared.set_blockhash([0x33; 32], 1_000);
    let mint = Pubkey::new_unique();
    r.adopt(mint);
    r.cold();
    for s in 1_003..1_006 {
        r.slot(s);
    }
    let jobs = r.jobs();
    assert!(!jobs.is_empty(), "{:?}", r.hot.counters);
    for j in &jobs {
        assert!(j.real && rugsnipe::sender::sell_fans_out(j), "a final sell");
        assert_eq!(
            j.tmpl.lifetime(),
            [0x5a; 32],
            "the nonce, not the fresh blockhash"
        );
    }
    assert!(r.hot.counters.sells_on_nonce >= 1);
    // no sell pool: the same final sell goes out on the fresh blockhash
    let dir2 = tmpdir("sell-bh-no-pool");
    let mut r2 = Rig::new(&dir2, false, |_| {});
    r2.shared.set_blockhash([0x33; 32], 1_000);
    r2.adopt(Pubkey::new_unique());
    r2.slot(1_003);
    let jobs = r2.jobs();
    assert_eq!(jobs.len(), 1, "{:?}", r2.hot.counters);
    assert_eq!(jobs[0].tmpl.lifetime(), [0x33; 32]);
    assert_eq!(r2.hot.counters.sells_on_nonce, 0);
}

#[test]
fn without_a_nonce_the_sender_resolves_a_blockhash_within_its_budget() {
    let dir = tmpdir("sell-resolve");
    let mut r = Rig::new(&dir, true, |_| {});
    let mint = Pubkey::new_unique();
    r.adopt(mint);
    r.slot(1_003);
    let mut jobs = r.jobs();
    assert_eq!(jobs.len(), 1, "{:?}", r.hot.counters);
    assert_eq!(
        jobs[0].tmpl.lifetime(),
        NO_LIFETIME,
        "handed over without one"
    );
    assert_eq!(r.hot.counters.sells_resolve, 1);
    // a working chain: the job gets the fetched blockhash, the cache too
    let shared = Arc::new(Shared::default());
    let ok = BlockhashResolver::spawn(
        shared.clone(),
        Arc::new(|| Some(([0x77; 32], 0))),
        Duration::from_millis(300),
    )
    .unwrap();
    let mut job = jobs.remove(0);
    assert!(resolve_lifetime(&mut job.clone(), Some(&ok)));
    let mut j2 = job.clone();
    assert!(resolve_lifetime(&mut j2, Some(&ok)));
    assert_eq!(j2.tmpl.lifetime(), [0x77; 32]);
    assert_eq!(shared.fresh_blockhash(), Some([0x77; 32]));
    // a hung chain: refused inside the budget, never blocks for the RPC's timeout
    let slow = BlockhashResolver::spawn(
        Arc::new(Shared::default()),
        Arc::new(|| {
            std::thread::sleep(Duration::from_secs(2));
            Some(([0x78; 32], 0))
        }),
        Duration::from_millis(300),
    )
    .unwrap();
    let t0 = Instant::now();
    assert!(!resolve_lifetime(&mut job, Some(&slow)));
    assert!(
        t0.elapsed() < Duration::from_millis(900),
        "{:?}",
        t0.elapsed()
    );
    // the hot loop is told and takes the attempt back (retried next slot)
    r.fills_tx
        .push(HotIn::SellUnsent {
            intent: jobs_intent(&job),
            idx: job.idx,
        })
        .unwrap();
    r.slot(1_004);
    assert_eq!(r.hot.counters.sells_unsent, 1);
    let again = r.jobs();
    assert_eq!(again.len(), 1, "re-issued on the next slot");
    assert_eq!(again[0].idx, job.idx, "same try");
}

fn jobs_intent(j: &FireJob) -> u64 {
    j.intent
}

#[test]
fn with_no_lifetime_at_all_sells_retry_every_slot_buys_pause_and_the_guard_gets_it() {
    let dir = tmpdir("sell-none");
    let mut r = Rig::new(&dir, false, |c| c.max_blockhash_age_ms = 200);
    let mint = Pubkey::new_unique();
    r.adopt(mint);
    r.cold();
    let t0 = Instant::now();
    let mut s = 1_003;
    let mut handoff = None;
    let mut paused_seen = false;
    while t0.elapsed() < Duration::from_millis(5_600) {
        r.slot(s);
        s += 1;
        paused_seen |= r.hot.sell_blocked();
        for m in r.cold() {
            if let ColdMsg::GuardHandoff {
                mint: m, unsold_ms, ..
            } = m
            {
                handoff = Some((m, unsold_ms));
            }
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    let slots = s - 1_003;
    let c = r.hot.counters;
    assert!(
        r.jobs().is_empty(),
        "nothing can be signed without a lifetime"
    );
    assert!(
        c.sells_no_blockhash + 2 >= slots,
        "one attempt every slot ({} attempts in {slots} slots), not 3 then silence",
        c.sells_no_blockhash
    );
    let p = r.hot.position_of(&mint).unwrap();
    assert_eq!(p.sell_spend, 0, "unsent attempts cost nothing");
    assert!(p
        .pending
        .iter()
        .flatten()
        .all(|o| o.order.try_no == 1 && !o.exhausted));
    let (m, unsold) = handoff.expect("handed to the guard after ~5 s");
    assert_eq!(m, mint);
    assert!(unsold >= rugsnipe::hot::HANDOFF_MS);
    // buys paused while no blockhash and no sell nonce; a fresh one clears it
    assert!(paused_seen && r.hot.sell_blocked());
    assert!(r.shared.blockhash_age_ms.load(Relaxed) > 200);
    r.shared.set_blockhash([0x42; 32], s);
    r.slot(s + 1);
    assert!(!r.hot.sell_blocked());
    assert_eq!(r.shared.blockhash_age_ms.load(Relaxed), 0);
    let jobs = r.jobs();
    assert!(
        !jobs.is_empty(),
        "the next slot's attempt goes out on the fresh blockhash"
    );
    assert_eq!(jobs[0].tmpl.lifetime(), [0x42; 32]);
}
