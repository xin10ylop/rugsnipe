//! The hot thread does not allocate after warm-up (docs/06 section 14 item 5):
//! decisions (match, risk, nonce, patch, sign, paper send, record), paper
//! fills, exit decisions, simulated sell fills, closes and ticks, and the
//! gRPC wire path (transaction bytes + meta trailer -> `TxView` ->
//! `create_hit` with the lookup tables' addresses and inner instructions,
//! recorded mainnet creates: v0 with lookups, v1, a launcher CPI), all under
//! a counting global allocator that only counts this thread.

mod common;

use common::*;
use pump_codec::Pubkey;
use rugsnipe::alloc_count::{self, CountingAlloc};
use rugsnipe_feed::{FeedBuf, FeedEvent, FeedMux, FeedStatus, Payload};
use std::time::{Duration, Instant};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

/// One round: `k` watched creates (each later filled, dumped by its dev and
/// closed) plus unwatched, mayhem and duplicate creates, starting at `slot0`.
fn round(devs: &[Pubkey], slot0: u64, n0: u64) -> Vec<FeedEvent> {
    let mut v = vec![];
    let mut n = n0;
    for (i, dev) in devs.iter().enumerate() {
        let s = slot0 + 10 * i as u64;
        let mint = Pubkey::new_unique();
        let (h, mut c) = create(mint, *dev, s, n, 800_000_000);
        n += 1;
        let (other, _) = create(Pubkey::new_unique(), Pubkey::new_unique(), s, n, 0);
        n += 1;
        let (mut mayhem, _) = create(Pubkey::new_unique(), *dev, s, n, 0);
        mayhem.mayhem = true;
        n += 1;
        v.push(ev(s, Payload::Hit(h)));
        v.push(ev(s, Payload::Hit(other)));
        v.push(ev(s, Payload::Hit(other))); // duplicate
        v.push(ev(s, Payload::Hit(mayhem)));
        v.push(ev(
            s + 1,
            Payload::Trade(trade(
                &mut c,
                mint,
                Pubkey::new_unique(),
                s + 1,
                n,
                300_000_000,
                0,
            )),
        ));
        n += 1;
        v.push(ev(
            s + 2,
            Payload::Trade(trade(
                &mut c,
                mint,
                Pubkey::new_unique(),
                s + 2,
                n,
                100_000_000,
                0,
            )),
        ));
        n += 1;
        let dump = h.intx_tokens_out / 2;
        v.push(ev(
            s + 6,
            Payload::Trade(trade(&mut c, mint, *dev, s + 6, n, 0, dump)),
        ));
        n += 1;
        v.push(ev(
            s + 7,
            Payload::Trade(trade(
                &mut c,
                mint,
                Pubkey::new_unique(),
                s + 7,
                n,
                5_000_000,
                0,
            )),
        ));
        n += 1;
        // a later slot closes s+7, where the dump sell (sent in s+6) fills
        // (paper.exit_latency_slots = 1); a known wallet keeps the mint keys
        // (and so their nonce indices) the same as without this event
        v.push(ev(
            s + 8,
            Payload::Trade(trade(&mut c, mint, *dev, s + 8, n, 5_000_000, 0)),
        ));
        n += 1;
    }
    v
}

fn run(app: &mut rugsnipe::app::App, feed: &mut FeedMux, buf: &mut FeedBuf, want_closed: u64) {
    let t0 = Instant::now();
    loop {
        let st = app.hot.step(feed, buf);
        if st == FeedStatus::Ended && app.hot.counters.closed >= want_closed {
            return;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "{:?} {:?}",
            app.hot.counters,
            app.stats.decision_counts()
        );
        if st == FeedStatus::Ended {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

#[test]
fn hot_thread_allocates_nothing_after_warm_up() {
    let dir = tmpdir("zero-alloc");
    let devs: Vec<Pubkey> = (0..8).map(|_| Pubkey::new_unique()).collect();
    let mut cfg = paper_cfg(&dir);
    cfg.risk.max_buys_per_op_per_day = 1_000;
    cfg.risk.max_buys_per_hour = 1_000;
    let mut app = build(cfg, armset(&devs, &[]), &dir, 3, true);
    // gRPC wire creates (recorded mainnet transactions with their trailer)
    let wire = |names: &[&str], slot: u64| {
        WireFeed::new(
            names
                .iter()
                .enumerate()
                .map(|(i, n)| {
                    let (tx, meta) = fixture_wire(&codec_fixture(n), 10 + i as u32);
                    (slot + i as u64, tx, meta)
                })
                .collect(),
        )
    };
    // warm-up: the same kinds of events with other mints
    let mut buf = FeedBuf::with_capacity(64, 1 << 16);
    let mut warm = FeedMux::new(vec![
        Box::new(VecFeed::new(round(&devs[..4], 1_000, 1))),
        Box::new(wire(
            &[
                "fresh_v0alt_create_v2_buy_v3",
                "v1_create_v2_buy",
                "fresh_v1_cpi_create_v2_bexq_v2",
            ],
            1_000,
        )),
    ]);
    run(&mut app, &mut warm, &mut buf, 4);
    let wire_misses = app.stats.wire_misses.get();
    // the feed (and its dedup ring) is built before counting starts
    let counted_wire = [
        "fresh_v0alt_create_v2_inner_buy",
        "v0_create_v2_buy_v3",
        "fresh_v0alt_create_v2_buy18",
        "v1_create_v2_buy_b",
    ];
    let mut counted = FeedMux::new(vec![
        Box::new(VecFeed::new(round(&devs[4..], 2_000, 10_000))),
        Box::new(wire(&counted_wire, 2_000)),
    ]);
    let before = app.hot.counters;
    alloc_count::reset();
    alloc_count::set_counting(true);
    run(&mut app, &mut counted, &mut buf, before.closed + 4);
    alloc_count::set_counting(false);
    let (allocs, bytes) = alloc_count::counts();
    let after = app.hot.counters;
    assert_eq!(after.intents - before.intents, 4, "{after:?}");
    assert_eq!(
        app.stats.wire_misses.get(),
        wire_misses,
        "every wire create decoded"
    );
    assert!(
        after.creates - before.creates >= 4 * 3 + counted_wire.len() as u64,
        "the wire creates reached the decision path: {after:?}"
    );
    assert_eq!(after.fills - before.fills, 4);
    assert!(after.sell_fills - before.sell_fills >= 4);
    assert_eq!(after.closed - before.closed, 4);
    assert_eq!(
        allocs, 0,
        "{allocs} allocation(s), {bytes} bytes on the hot thread after warm-up"
    );
    let (hd, _, _) = app.hot.histograms();
    assert!(hd.len() >= 8);
    app.shutdown(vec![]);
    let _ = std::fs::remove_dir_all(&dir);
}
