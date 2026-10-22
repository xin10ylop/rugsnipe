//! Dead-man check: "no real buys while launches keep flowing".
//!
//! The cold thread ticks a [`DeadMan`] once a minute with the hot thread's
//! own cumulative counters (creates seen, `fire` decisions, every decision
//! outcome); the ring keeps the last [`WINDOW_MIN`] per-minute deltas, so
//! the check is independent of the ring/ledger paths and of any single
//! skip reason. When the window is full, the process is at least
//! [`MIN_UP_S`] old, at least [`MIN_CREATES`] creates flowed through it and
//! not one real buy was sent, [`DeadMan::check`] returns a [`Verdict`] and
//! the cold thread writes one stderr line and one Telegram alert (the same
//! [`text`]). The verdict names the top decision outcomes of the window, so
//! a silent operator latch (`skip_op_latched`) or a persisted kill
//! (`skip_killed`) is named in the alert itself.
//!
//! Spacing: after [`DeadMan::alerted`] the condition is re-evaluated every
//! minute and fires again only [`REALERT_S`] later while it still holds (at
//! most one alert per hour). `last_alert_unix` is *not* cleared by a fire:
//! a buy followed by another fire-less window re-alerts only once the
//! spacing has passed. Nothing here clears, kills or resets anything.
//!
//! The mode gate (real modes only) lives in the cold thread, so the ring and
//! the gauges ([`DeadMan::minutes_since_fire`], [`DeadMan::last_fire_unix`])
//! also run in paper, where `fire` is never recorded and the gauge simply
//! reports the time since start.
//!
//! The ring is tick-based (one slot per `tick`): a wall-clock jump only
//! skews `minutes_since_fire` and the re-alert spacing (`saturating_sub`
//! keeps both non-negative), never the window itself. Pure: no I/O, no
//! clock reads.

use std::fmt::Write as _;

/// Ring slots (minutes) of the window.
pub const WINDOW_MIN: usize = 15;
/// Creates that must have flowed through the window for silence to count.
pub const MIN_CREATES: u64 = 50;
/// Process age (seconds) before the first check can pass.
pub const MIN_UP_S: u64 = 15 * 60;
/// Spacing (seconds) between two alerts.
pub const REALERT_S: u64 = 60 * 60;
/// Verdict labels listed (the top decision outcomes of the window).
pub const TOP_N: usize = 3;

/// One minute's deltas.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Minute {
    /// Creates seen (first copies, all feed sources).
    pub creates: u64,
    /// Real buys handed to the fire ring (`fire` decisions).
    pub fires: u64,
}

/// What an alert says (see [`text`]).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Verdict {
    /// Creates seen in the window.
    pub creates: u64,
    /// Minutes since the last real buy this run; `None` when there was none.
    pub minutes_since_fire: Option<u64>,
    /// At most [`TOP_N`] decision outcomes of the window: count desc, label
    /// asc on ties.
    pub top: Vec<(&'static str, u64)>,
}

/// The ring and the alert state. Fed with cumulative counters; stores deltas.
#[derive(Clone, Debug)]
pub struct DeadMan {
    /// Per-minute deltas.
    ring: [Minute; WINDOW_MIN],
    /// Next slot to write.
    head: usize,
    /// Filled slots; saturates at [`WINDOW_MIN`] (full iff `len == WINDOW_MIN`).
    len: usize,
    /// Per-minute decision deltas (non-zero labels only), same `head`/`len`.
    dec_ring: [Vec<(&'static str, u64)>; WINDOW_MIN],
    /// Cumulative values at the previous tick.
    prev_creates: u64,
    prev_fires: u64,
    prev_dec: Vec<(&'static str, u64)>,
    started_unix: u64,
    last_fire_unix: Option<u64>,
    last_alert_unix: Option<u64>,
}

impl DeadMan {
    /// Seed with the counters' current values at `now` (process start), so
    /// the first tick's delta is the first minute only, not the process
    /// total.
    pub fn new(
        now: u64,
        creates_total: u64,
        fires_total: u64,
        dec: Vec<(&'static str, u64)>,
    ) -> Self {
        Self {
            ring: [Minute::default(); WINDOW_MIN],
            head: 0,
            len: 0,
            dec_ring: std::array::from_fn(|_| Vec::new()),
            prev_creates: creates_total,
            prev_fires: fires_total,
            prev_dec: dec,
            started_unix: now,
            last_fire_unix: None,
            last_alert_unix: None,
        }
    }

    /// A real buy was sent at `now` (the precise stamp; the ring's fire delta
    /// still comes from the counter). Idempotent; never moves the stamp back.
    pub fn on_fire(&mut self, now: u64) {
        self.last_fire_unix = Some(self.last_fire_unix.map_or(now, |t| t.max(now)));
    }

    /// One minute passed: push the deltas since the previous tick.
    pub fn tick(
        &mut self,
        now: u64,
        creates_total: u64,
        fires_total: u64,
        dec: Vec<(&'static str, u64)>,
    ) {
        let creates = creates_total.saturating_sub(self.prev_creates);
        let fires = fires_total.saturating_sub(self.prev_fires);
        let deltas: Vec<(&'static str, u64)> = dec
            .iter()
            .map(|(label, n)| {
                let prev = self
                    .prev_dec
                    .iter()
                    .find(|(l, _)| l == label)
                    .map_or(0, |(_, p)| *p);
                (*label, n.saturating_sub(prev))
            })
            .filter(|(_, d)| *d > 0)
            .collect();
        self.ring[self.head] = Minute { creates, fires };
        self.dec_ring[self.head] = deltas;
        self.head = (self.head + 1) % WINDOW_MIN;
        self.len = (self.len + 1).min(WINDOW_MIN);
        self.prev_creates = creates_total;
        self.prev_fires = fires_total;
        self.prev_dec = dec;
        // no precise stamp arrived for this minute's fire(s): the tick's time
        if fires > 0
            && self
                .last_fire_unix
                .is_none_or(|t| t < now.saturating_sub(60))
        {
            self.last_fire_unix = Some(now);
        }
    }

    /// Filled slots (minutes ticked, at most [`WINDOW_MIN`]).
    pub fn filled(&self) -> usize {
        self.len
    }

    /// `true` once [`WINDOW_MIN`] minutes have been ticked.
    pub fn is_full(&self) -> bool {
        self.len == WINDOW_MIN
    }

    /// Sums over the window.
    pub fn window(&self) -> Minute {
        self.ring[..self.len]
            .iter()
            .fold(Minute::default(), |a, m| Minute {
                creates: a.creates + m.creates,
                fires: a.fires + m.fires,
            })
    }

    /// Decision outcomes of the window: count desc, label asc on ties.
    pub fn window_decisions(&self) -> Vec<(&'static str, u64)> {
        let mut sum: Vec<(&'static str, u64)> = Vec::new();
        for slot in &self.dec_ring[..self.len] {
            for (label, n) in slot {
                match sum.iter_mut().find(|(l, _)| l == label) {
                    Some((_, s)) => *s += n,
                    None => sum.push((label, *n)),
                }
            }
        }
        sum.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        sum
    }

    /// Unix time of the last real buy this run (`None`: none yet).
    pub fn last_fire_unix(&self) -> Option<u64> {
        self.last_fire_unix
    }

    /// Minutes since the last real buy, or since start when none this run
    /// (never 0 just because nothing was bought: a box that never bought
    /// since its restart is the case to catch; [`Self::last_fire_unix`]
    /// disambiguates).
    pub fn minutes_since_fire(&self, now: u64) -> u64 {
        now.saturating_sub(self.last_fire_unix.unwrap_or(self.started_unix)) / 60
    }

    /// The condition: a full window, process age >= [`MIN_UP_S`], at least
    /// [`MIN_CREATES`] creates and no fire in the window, and no alert in
    /// the last [`REALERT_S`] seconds.
    pub fn check(&self, now: u64) -> Option<Verdict> {
        if !self.is_full() || now.saturating_sub(self.started_unix) < MIN_UP_S {
            return None;
        }
        let w = self.window();
        if w.creates < MIN_CREATES || w.fires > 0 {
            return None;
        }
        if self
            .last_alert_unix
            .is_some_and(|t| now.saturating_sub(t) < REALERT_S)
        {
            return None;
        }
        let mut top = self.window_decisions();
        top.truncate(TOP_N);
        Some(Verdict {
            creates: w.creates,
            minutes_since_fire: self.last_fire_unix.map(|t| now.saturating_sub(t) / 60),
            top,
        })
    }

    /// An alert went out at `now`; nothing else changes.
    pub fn alerted(&mut self, now: u64) {
        self.last_alert_unix = Some(now);
    }
}

/// The alert line (stderr and Telegram receive the same string), e.g.
/// `dead-man on fra1 (probe): no buys in the last 15 min, 1204 creates seen;
/// last fire 212 min ago; decisions: skip_op_latched 611, no_match 430,
/// late_slot 71; killed: no`. `kill` is the kill reason's name (with the
/// trigger's detail when one latched it) or `None` for "killed: no".
pub fn text(box_id: &str, mode: &str, v: &Verdict, kill: Option<&str>) -> String {
    let mut s = format!(
        "dead-man on {box_id} ({mode}): no buys in the last {WINDOW_MIN} min, {} creates seen; last fire ",
        v.creates
    );
    match v.minutes_since_fire {
        Some(m) => {
            let _ = write!(s, "{m} min ago");
        }
        None => s.push_str("none this run"),
    }
    s.push_str("; decisions: ");
    if v.top.is_empty() {
        s.push_str("none");
    }
    for (i, (label, n)) in v.top.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        let _ = write!(s, "{label} {n}");
    }
    let _ = write!(s, "; killed: {}", kill.unwrap_or("no"));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_761_100_000;

    /// `ticks` minutes `step_s` apart from `T0`: `creates(i)` and `fires(i)`
    /// are the cumulative totals at tick `i` (1-based).
    fn run(
        d: &mut DeadMan,
        ticks: std::ops::RangeInclusive<usize>,
        step_s: u64,
        creates: impl Fn(usize) -> u64,
        fires: impl Fn(usize) -> u64,
    ) {
        for i in ticks {
            d.tick(T0 + step_s * i as u64, creates(i), fires(i), vec![]);
        }
    }

    #[test]
    fn ring_keeps_the_last_fifteen_minutes() {
        // the seeded baseline (1_000 creates, 3 fires) must not count
        let mut d = DeadMan::new(T0, 1_000, 3, vec![]);
        assert_eq!(d.window(), Minute::default());
        assert_eq!(d.filled(), 0);
        assert_eq!(d.minutes_since_fire(T0 + 600), 10, "since start when none");
        assert_eq!(d.last_fire_unix(), None);
        let fires = |i: usize| if i >= 3 { 4 } else { 3 };
        for i in 1..=20usize {
            d.tick(T0 + 60 * i as u64, 1_000 + 10 * i as u64, fires(i), vec![]);
            match i {
                1 => assert_eq!(
                    d.window(),
                    Minute {
                        creates: 10,
                        fires: 0
                    }
                ),
                3 => assert_eq!(
                    d.window(),
                    Minute {
                        creates: 30,
                        fires: 1
                    }
                ),
                14 => assert!(!d.is_full()),
                15 => {
                    assert_eq!(d.filled(), 15);
                    assert!(d.is_full());
                    assert_eq!(
                        d.window(),
                        Minute {
                            creates: 150,
                            fires: 1
                        }
                    );
                }
                17 => assert_eq!(d.window().fires, 1, "tick 3 is still the oldest slot"),
                18 => assert_eq!(d.window().fires, 0, "tick 3 fell out at 3 + 15"),
                _ => {}
            }
        }
        assert_eq!(d.filled(), 15);
        assert_eq!(d.window().creates, 150, "the 5 oldest minutes dropped");
        assert_eq!(d.window().fires, 0);
        // no precise stamp: the fire is dated by the tick that saw it
        assert_eq!(d.last_fire_unix(), Some(T0 + 180));
        assert_eq!(d.minutes_since_fire(T0 + 20 * 60), 17);

        // a precise stamp before the next tick wins over the tick's minute
        let mut d = DeadMan::new(T0, 1_000, 3, vec![]);
        run(&mut d, 1..=3, 60, |i| 1_000 + 10 * i as u64, fires);
        assert_eq!(d.last_fire_unix(), Some(T0 + 180));
        d.on_fire(T0 + 200);
        d.on_fire(T0 + 200);
        assert_eq!(d.last_fire_unix(), Some(T0 + 200), "idempotent");
        d.on_fire(T0 + 190);
        assert_eq!(d.last_fire_unix(), Some(T0 + 200), "never moves back");
        run(&mut d, 4..=20, 60, |i| 1_000 + 10 * i as u64, fires);
        assert_eq!(d.last_fire_unix(), Some(T0 + 200));
        assert_eq!(d.minutes_since_fire(T0 + 20 * 60), 16);

        // the usual order: the stamp arrives, then the tick sees the delta
        let mut d = DeadMan::new(T0, 1_000, 3, vec![]);
        run(&mut d, 1..=2, 60, |i| 1_000 + 10 * i as u64, fires);
        d.on_fire(T0 + 170);
        d.tick(T0 + 180, 1_030, 4, vec![]);
        assert_eq!(
            d.last_fire_unix(),
            Some(T0 + 170),
            "a stamp within the minute is kept"
        );

        // counters that went backwards (restart of a source) do not underflow
        let mut d = DeadMan::new(T0, 1_000, 3, vec![]);
        d.tick(T0 + 60, 900, 2, vec![]);
        assert_eq!(d.window(), Minute::default());
    }

    #[test]
    fn alert_needs_a_full_window_of_launches_and_no_fire() {
        // 14 minutes: never, however many creates
        let mut d = DeadMan::new(T0, 0, 0, vec![]);
        run(&mut d, 1..=14, 60, |i| 100 * i as u64, |_| 0);
        assert!(d.check(T0 + 14 * 60).is_none());
        assert!(
            d.check(T0 + 3_600).is_none(),
            "len < 15 regardless of uptime"
        );

        // 15 minutes, 49 creates in the window: no
        let mut d = DeadMan::new(T0, 0, 0, vec![]);
        run(
            &mut d,
            1..=15,
            60,
            |i| if i < 15 { 3 * i as u64 } else { 49 },
            |_| 0,
        );
        assert!(d.is_full());
        assert_eq!(d.window().creates, 49);
        assert!(d.check(T0 + 900).is_none());

        // 50 creates, no fire, up 900 s: yes (started = now, 15 ticks 60 s apart)
        let mut d = DeadMan::new(T0, 0, 0, vec![]);
        run(
            &mut d,
            1..=15,
            60,
            |i| if i < 15 { 3 * i as u64 } else { 50 },
            |_| 0,
        );
        assert!(d.check(T0 + 899).is_none(), "MIN_UP_S guard");
        let v = d.check(T0 + 900).expect("full window, 50 creates, no fire");
        assert_eq!(v.creates, 50);
        assert_eq!(v.minutes_since_fire, None, "none this run");
        assert!(v.top.is_empty());

        // a window that fills faster than wall-clock 15 min stays silent
        let mut d = DeadMan::new(T0, 0, 0, vec![]);
        run(&mut d, 1..=15, 1, |i| 10 * i as u64, |_| 0);
        assert!(d.is_full());
        assert!(d.check(T0 + 15).is_none(), "MIN_UP_S guard");

        // any fire in the window: no (even the oldest minute)
        for fire_at in [1usize, 8, 15] {
            let mut d = DeadMan::new(T0, 0, 0, vec![]);
            run(
                &mut d,
                1..=15,
                60,
                |i| 10 * i as u64,
                |i| u64::from(i >= fire_at),
            );
            assert_eq!(d.window().fires, 1);
            assert!(d.check(T0 + 900).is_none(), "fire at tick {fire_at}");
        }

        // spacing: alerted(now) silences until now + 3600 while the condition holds
        let mut d = DeadMan::new(T0, 0, 0, vec![]);
        run(&mut d, 1..=15, 60, |i| 10 * i as u64, |_| 0);
        let a = T0 + 900;
        assert!(d.check(a).is_some());
        d.alerted(a);
        assert!(d.check(a).is_none());
        assert!(d.check(a + 3_599).is_none());
        let v = d.check(a + 3_600).expect("re-alert after REALERT_S");
        assert_eq!(v.creates, 150);

        // a fire, then 15 fire-less minutes: Some only once the spacing passed
        // (last_alert_unix is not cleared by the fire)
        d.tick(T0 + 16 * 60, 160, 1, vec![]);
        assert_eq!(d.last_fire_unix(), Some(T0 + 16 * 60));
        run(&mut d, 17..=31, 60, |i| 10 * i as u64, |_| 1);
        assert_eq!(d.window().fires, 0, "the fire fell out of the window");
        assert_eq!(d.window().creates, 150);
        assert!(d.check(T0 + 31 * 60).is_none(), "960 s after the alert");
        run(&mut d, 32..=74, 60, |i| 10 * i as u64, |_| 1);
        assert!(d.check(T0 + 74 * 60).is_none(), "3540 s after the alert");
        run(&mut d, 75..=75, 60, |i| 10 * i as u64, |_| 1);
        let v = d.check(T0 + 75 * 60).expect("3600 s after the alert");
        assert_eq!(v.minutes_since_fire, Some(59));
    }

    #[test]
    fn text_names_creates_last_fire_top_outcomes_and_kill() {
        // deltas by label, the seeded baseline excluded, sorted count desc
        let mut d = DeadMan::new(T0, 0, 0, vec![("skip_op_latched", 5), ("stale", 1)]);
        d.tick(
            T0 + 60,
            1_204,
            0,
            vec![
                ("stale", 3),
                ("skip_op_latched", 616),
                ("late_slot", 71),
                ("no_match", 430),
                ("other", 0),
            ],
        );
        assert_eq!(
            d.window_decisions(),
            vec![
                ("skip_op_latched", 611),
                ("no_match", 430),
                ("late_slot", 71),
                ("stale", 2)
            ]
        );
        // decisions missing from a later tick (a label went quiet) still sum
        d.tick(
            T0 + 120,
            1_300,
            0,
            vec![("late_slot", 72), ("skip_op_latched", 616)],
        );
        assert_eq!(
            d.window_decisions(),
            vec![
                ("skip_op_latched", 611),
                ("no_match", 430),
                ("late_slot", 72),
                ("stale", 2)
            ]
        );
        // ties: label asc
        let mut t = DeadMan::new(T0, 0, 0, vec![]);
        t.tick(
            T0 + 60,
            1,
            0,
            vec![("stale", 2), ("late_slot", 2), ("no_match", 5)],
        );
        assert_eq!(
            t.window_decisions(),
            vec![("no_match", 5), ("late_slot", 2), ("stale", 2)]
        );

        let top: Vec<_> = d.window_decisions().into_iter().take(TOP_N).collect();
        let v = Verdict {
            creates: 1_204,
            minutes_since_fire: Some(212),
            top,
        };
        let s = text("fra1", "probe", &v, None);
        assert_eq!(
            s,
            "dead-man on fra1 (probe): no buys in the last 15 min, 1204 creates seen; \
             last fire 212 min ago; decisions: skip_op_latched 611, no_match 430, late_slot 72; \
             killed: no"
        );
        let creates = s.find("1204 creates").unwrap();
        let fire = s.find("last fire 212 min ago").unwrap();
        let dec = s
            .find("skip_op_latched 611, no_match 430, late_slot 72")
            .unwrap();
        assert!(creates < fire && fire < dec, "{s}");
        assert!(!s.contains("stale"), "only the top {TOP_N}: {s}");

        let v = Verdict {
            creates: 980,
            minutes_since_fire: None,
            top: vec![("skip_killed", 980)],
        };
        let s = text("fra1", "probe", &v, Some("decode_miss"));
        assert_eq!(
            s,
            "dead-man on fra1 (probe): no buys in the last 15 min, 980 creates seen; \
             last fire none this run; decisions: skip_killed 980; killed: decode_miss"
        );
        let s = text("fra1", "live", &v, Some("disk (disk: 91% used)"));
        assert!(s.ends_with("; killed: disk (disk: 91% used)"), "{s}");
        assert!(s.contains("(live)"), "{s}");
        let empty = Verdict {
            creates: 60,
            minutes_since_fire: Some(0),
            top: vec![],
        };
        assert!(
            text("b", "probe", &empty, None).contains("; decisions: none; killed: no"),
            "{}",
            text("b", "probe", &empty, None)
        );

        // check() truncates the verdict to the top three
        let mut d = DeadMan::new(T0, 0, 0, vec![]);
        for i in 1..=15usize {
            let n = i as u64;
            d.tick(
                T0 + 60 * n,
                10 * n,
                0,
                vec![
                    ("no_match", 6 * n),
                    ("stale", n),
                    ("late_slot", 2 * n),
                    ("skip_class", 3 * n),
                ],
            );
        }
        let v = d.check(T0 + 900).unwrap();
        assert_eq!(
            v.top,
            vec![("no_match", 90), ("skip_class", 45), ("late_slot", 30)]
        );
    }
}
