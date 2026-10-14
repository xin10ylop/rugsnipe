//! The E1 path on the hot thread does not allocate after warm-up either:
//! the verdict, the open at the post-bundle state, the insider keys, the
//! replayed tape on the counterfactual curve (ticks per slot), the lead
//! exits, the simulated sell fill, the close and the failed-attempt record,
//! with the entry filter scoring every verdict (`entry.filter_model`, log
//! mode: features in, score and tertile to rslog and the cold ring).

mod common;

use common::*;
use pump_codec::curve::CurveState;
use pump_codec::Pubkey;
use rugsnipe::alloc_count::{self, CountingAlloc};
use rugsnipe::msgs::{HotIn, ShadowInfo};
use rugsnipe_core::rslog::ShadowOutcome;
use rugsnipe_core::IntentId;
use rugsnipe_exit::{Trade, Venue};
use rugsnipe_feed::{FeedBuf, FeedMux, FeedStatus, Payload};
use std::time::{Duration, Instant};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

fn hist(c: &mut CurveState, mint: Pubkey, user: Pubkey, slot: u64, sol: u64, sell: u64) -> Trade {
    let t = trade(c, mint, user, slot, slot, sol, sell);
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

/// Messages of one E1 round: `k` filled launches (TP, insider sell, idle)
/// and one failed attempt, with their creates at `s0 + 20 i`.
struct Round {
    creates: Vec<rugsnipe_feed::FeedEvent>,
    msgs: Vec<HotIn>,
}

fn round(dev: Pubkey, owner: Pubkey, s0: u64, n0: u64) -> Round {
    let mut creates = vec![];
    let mut msgs = vec![];
    for i in 0..4u64 {
        let s = s0 + 20 * i;
        let mint = Pubkey::new_unique();
        let (h, mut c) = create(mint, dev, s, n0 + i, 1_000_000_000);
        creates.push(ev(s, Payload::Hit(h)));
        let b1 = Pubkey::new_unique();
        hist(&mut c, mint, b1, s, 1_000_000_000, 0);
        let intent = IntentId::new(&mint, &owner).0;
        let info = ShadowInfo {
            create_slot: s,
            premium_bps: 110,
            post_bundle_v: c.virtual_sol,
            post_bundle_t: c.virtual_token,
            ..Default::default()
        };
        // the entry filter's features (scored on the hot thread, log mode)
        let mut feat = rugsnipe::filter::Feat::default();
        feat.x[0] = 1.0 + i as f64;
        feat.x[4] = 30.0 + i as f64;
        msgs.push(HotIn::E1Feat {
            intent,
            feat,
            op_lead_n: i as u32,
            bw_lead_n: 0,
        });
        if i == 3 {
            msgs.push(HotIn::E1Open {
                intent,
                mint,
                outcome: ShadowOutcome::Premium,
                info,
            });
            continue;
        }
        msgs.push(HotIn::E1Open {
            intent,
            mint,
            outcome: ShadowOutcome::Filled,
            info,
        });
        msgs.push(HotIn::E1Insider {
            intent,
            key: b1,
            creator: false,
        });
        msgs.push(HotIn::E1Insider {
            intent,
            key: dev,
            creator: true,
        });
        let tape = match i {
            0 => vec![hist(
                &mut c,
                mint,
                Pubkey::new_unique(),
                s + 1,
                6_000_000_000,
                0,
            )],
            1 => vec![
                hist(&mut c, mint, Pubkey::new_unique(), s + 1, 100_000_000, 0),
                hist(&mut c, mint, b1, s + 2, 0, 1_000_000_000_000),
            ],
            _ => vec![],
        };
        for t in tape {
            msgs.push(HotIn::E1Trade { intent, trade: t });
        }
        msgs.push(HotIn::E1End {
            intent,
            horizon: s + 12,
        });
    }
    Round { creates, msgs }
}

fn play(app: &mut rugsnipe::app::App, r: &Round, buf: &mut FeedBuf, feed: &mut FeedMux) {
    let want = app.hot.counters.intents + r.creates.len() as u64;
    let t0 = Instant::now();
    while app.hot.counters.intents < want {
        let st = app.hot.step(feed, buf);
        assert!(
            st != FeedStatus::Ended || app.hot.counters.intents >= want,
            "{:?} {:?}",
            app.hot.counters,
            app.stats.decision_counts()
        );
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "{:?}",
            app.hot.counters
        );
    }
    for m in &r.msgs {
        app.hot.inject(*m);
    }
}

#[test]
fn e1_replay_allocates_nothing_after_warm_up() {
    let dir = tmpdir("zero-alloc-e1");
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/paper-lead.example.toml"
    );
    let lead = rugsnipe::setup::load_config(std::path::Path::new(p)).unwrap();
    let mut cfg = paper_cfg(&dir);
    cfg.exit = lead.exit.clone();
    cfg.paper = lead.paper.clone();
    cfg.entry = lead.entry.clone();
    cfg.entry.filter_model = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/reports/strategy/entry_filter_v1.json"
    )
    .into();
    cfg.risk.max_buys_per_op_per_day = 1_000;
    cfg.risk.max_buys_per_hour = 1_000;
    // the simulated keeper burns asynchronously: a large pool keeps the two
    // rounds' mints on distinct nonces
    cfg.tx.nonce_pool = 1_024;
    let dev = Pubkey::new_unique();
    let mut app = build(cfg, armset(&[dev], &[]), &dir, 5_000, false);
    let owner = app.owner;
    let mut buf = FeedBuf::with_capacity(64, 1 << 16);
    let warm = round(dev, owner, 1_000, 1);
    let mut wf = FeedMux::new(vec![Box::new(VecFeed::new(warm.creates.clone()))]);
    play(&mut app, &warm, &mut buf, &mut wf);
    assert_eq!(app.hot.counters.closed, 3, "{:?}", app.hot.counters);
    // built before counting starts
    let counted = round(dev, owner, 2_000, 100);
    let mut cf = FeedMux::new(vec![Box::new(VecFeed::new(counted.creates.clone()))]);
    let before = app.hot.counters;
    alloc_count::reset();
    alloc_count::set_counting(true);
    play(&mut app, &counted, &mut buf, &mut cf);
    alloc_count::set_counting(false);
    let (allocs, bytes) = alloc_count::counts();
    let after = app.hot.counters;
    assert_eq!(after.fills - before.fills, 3, "{after:?}");
    assert_eq!(after.closed - before.closed, 3);
    assert_eq!(after.e1_failed - before.e1_failed, 1);
    assert!(after.e1_replayed_trades > before.e1_replayed_trades);
    assert_eq!(
        allocs, 0,
        "{allocs} allocation(s), {bytes} bytes on the hot thread during E1 replays"
    );
    let summary = app.shutdown(vec![]);
    assert_eq!(
        summary.cold.paper.filter.scored, 8,
        "every verdict was scored"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
