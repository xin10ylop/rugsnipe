//! The hot loop's slot clock advances on the feed's slot updates, not only on
//! events: a held mint that goes quiet still has its hold timers evaluated at
//! every slot close (2026-10-22: with the clock driven by events alone, sells
//! on quiet mints triggered 3-17 slots late, when the next create anywhere, or
//! the dump on the mint itself, finally moved the clock).

mod common;

use common::*;
use pump_codec::Pubkey;
use rugsnipe_feed::{Feed, FeedBuf, FeedSrc, FeedStatus, Payload};
use std::time::{Duration, Instant};

/// Events once, then idle while the slot updates keep coming.
struct ClockFeed {
    inner: VecFeed,
    slot: u64,
    step: u64,
}

impl Feed for ClockFeed {
    fn poll(&mut self, out: &mut FeedBuf) -> FeedStatus {
        let st = self.inner.poll(out);
        if self.inner.pos >= self.inner.events.len() {
            self.slot += self.step;
        }
        st
    }
    fn src(&self) -> FeedSrc {
        FeedSrc::Replay
    }
    fn slot(&self) -> u64 {
        self.slot
    }
}

/// The lead policy's `[exit]` (max_hold_slots = 5, docs/04 section 2).
fn lead_exit() -> toml::Table {
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/paper-lead.example.toml"
    );
    let lead = rugsnipe::setup::load_config(std::path::Path::new(p)).unwrap();
    let hold = lead.exit["default"]["max_hold_slots"].as_integer();
    assert_eq!(hold, Some(5), "the lead policy's hard hold");
    lead.exit.clone()
}

/// A create, a paper fill, one trade, then silence: the max-hold sell must
/// trigger when the feed's slot reaches `opened + max_hold_slots`, with no
/// event on any mint moving the clock.
fn run(step: u64, with_clock: bool) -> (u64, u64, u64) {
    let dir = tmpdir("slot-clock");
    let dev = Pubkey::new_unique();
    let mut cfg = paper_cfg(&dir);
    cfg.exit = lead_exit();
    let mut app = build(cfg, armset(&[dev], &[]), &dir, 1, true);
    let mint = Pubkey::new_unique();
    let (h, mut c) = create(mint, dev, 100, 1, 800_000_000);
    let events = vec![
        ev(100, Payload::Hit(h)),
        ev(
            101,
            Payload::Trade(trade(
                &mut c,
                mint,
                Pubkey::new_unique(),
                101,
                2,
                300_000_000,
                0,
            )),
        ),
    ];
    let mut inner = VecFeed::new(events);
    inner.end_when_empty = false;
    let mut feed = ClockFeed {
        inner,
        slot: 101,
        step: if with_clock { step } else { 0 },
    };
    let mut buf = FeedBuf::with_capacity(64, 1 << 12);
    let t0 = Instant::now();
    let mut steps = 0u64;
    while app.hot.counters.closed == 0 && t0.elapsed() < Duration::from_millis(1500) {
        app.hot.step(&mut feed, &mut buf);
        steps += 1;
        std::thread::sleep(Duration::from_micros(200));
    }
    let closed = app.hot.counters.closed;
    let sells = app.hot.counters.sells;
    let slot = feed.slot;
    app.shutdown(vec![]);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = steps;
    (closed, sells, slot)
}

#[test]
fn a_quiet_mint_is_sold_at_max_hold_from_the_feed_slot_alone() {
    let (closed, sells, slot) = run(1, true);
    assert!(sells >= 1, "the max-hold sell was issued ({sells})");
    assert_eq!(closed, 1, "the position closed on the paper fill");
    // opened at the fill (slot 101 + fill timeout 1 = 102), max hold 5 ->
    // the sell triggers at the close of 107 and its paper fill lands a slot
    // later: the clock stopped right there
    assert!((108..=112).contains(&slot), "feed slot {slot}");
}

#[test]
fn without_slot_updates_the_quiet_mint_is_never_evaluated() {
    // the old behaviour, kept as the contrast: no event, no clock, no sell
    let (closed, sells, _) = run(1, false);
    assert_eq!(sells, 0);
    assert_eq!(closed, 0);
}
