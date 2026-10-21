//! Metrics (docs/06 section 13): `prometheus-client` counters and gauges,
//! every labelled child created at startup so the hot thread only does
//! atomic adds (no allocation, no lock).

use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::registry::Registry;
use rugsnipe_feed::event::SRC_COUNT;
use rugsnipe_feed::FeedSrc;
use rugsnipe_tx::senders::SenderId;

type Labels = Vec<(String, String)>;

/// Pipeline decision codes recorded in rslog `Decision.code` next to the
/// RiskGate codes (0 fire, 0x10 | paper reason, 0x20 | skip reason).
pub mod code {
    pub const CLASS: u8 = 0x41;
    pub const NO_MATCH: u8 = 0x42;
    pub const SHADOW: u8 = 0x43;
    pub const STALE: u8 = 0x44;
    pub const TOO_HIGH: u8 = 0x45;
    pub const NO_NONCE: u8 = 0x46;
    pub const SKIP_AB: u8 = 0x47;
    pub const ALREADY_FIRED: u8 = 0x48;
    pub const BAD_OP: u8 = 0x49;
    pub const NO_POSITION_SLOT: u8 = 0x4a;
    /// A per-mint key is not the canonical PDA of its seeds (no bump
    /// matches), so the buy would fail on chain and no CU limit exists.
    pub const BAD_KEYS: u8 = 0x4b;
    /// A real mode with the post-execution feed running: a free feed's copy
    /// of a create is recorded only (the gRPC copy decides, so a late-feed
    /// paper intent can never hold the mint's slot).
    pub const FREE_FEED_DEFERS: u8 = 0x4c;
    /// A real decision that runs after the create slot (the lane's slot is
    /// already past it): the buy could not land in the creation slot, so it
    /// is not sent (docs/04 section 2: fire only in the create slot).
    pub const LATE_SLOT: u8 = 0x4d;
    /// Metrics only: a paper decision that a real mode would have skipped
    /// (rslog keeps the RiskGate code and the `would_skip` reason).
    pub const PAPER_WOULD_SKIP: u8 = 0x1f;
}

/// `(code, label)` of every decision outcome.
pub const DECISIONS: &[(u8, &str)] = &[
    (0x00, "fire"),
    (0x11, "paper"),
    (0x12, "paper_regime_frozen"),
    (0x13, "paper_op_not_armed"),
    (0x14, "paper_armset_gate"),
    (code::PAPER_WOULD_SKIP, "paper_would_skip"),
    (0x21, "skip_killed"),
    (0x22, "skip_spend_cap"),
    (0x23, "skip_bid_cap"),
    (0x24, "skip_entry_too_high"),
    (0x25, "skip_balance_low"),
    (0x26, "skip_daily_loss"),
    (0x27, "skip_op_latched"),
    (0x28, "skip_max_open_positions"),
    (0x29, "skip_max_buys_per_hour"),
    (0x2a, "skip_max_buys_per_op_per_day"),
    (0x2b, "skip_max_probes_per_day"),
    (0x2c, "skip_bad_op_slot"),
    (code::CLASS, "skip_class"),
    (code::NO_MATCH, "no_match"),
    (code::SHADOW, "shadow"),
    (code::STALE, "stale"),
    (code::TOO_HIGH, "too_high"),
    (code::NO_NONCE, "no_nonce"),
    (code::SKIP_AB, "skip_ab"),
    (code::ALREADY_FIRED, "already_fired"),
    (code::BAD_OP, "bad_op"),
    (code::NO_POSITION_SLOT, "no_position_slot"),
    (code::BAD_KEYS, "bad_keys"),
    (code::FREE_FEED_DEFERS, "free_feed_defers"),
    (code::LATE_SLOT, "late_slot"),
];

/// Label of a decision code.
pub fn decision_label(c: u8) -> &'static str {
    DECISIONS
        .iter()
        .find(|(k, _)| *k == c)
        .map(|(_, n)| *n)
        .unwrap_or("other")
}

/// Exit reason labels (index = [`crate::msgs::reason_index`]).
pub const EXIT_REASONS: [&str; 8] = [
    "bundle_sell",
    "consolidation",
    "big_buy",
    "take_profit",
    "trailing",
    "phantom_sl",
    "no_activity",
    "max_hold",
];

/// Every metric of the hot box.
#[derive(Debug, Clone)]
pub struct HotStats {
    pub feed_events: [Counter; SRC_COUNT],
    pub creates: [Counter; SRC_COUNT],
    pub dups: Counter,
    pub wire_misses: Counter,
    decisions: [Option<Counter>; 256],
    decisions_other: Counter,
    pub intents: Counter,
    pub sends: [Counter; SenderId::COUNT],
    pub fills: Counter,
    pub fills_late: Counter,
    pub unfilled: Counter,
    pub positions_open: Gauge,
    pub exits: [Counter; 8],
    pub sells: Counter,
    pub sell_fills: Counter,
    pub sells_not_owner: Counter,
    pub alerts: Counter,
    pub dump_signals: Counter,
    pub realized_lamports: Gauge,
    pub trades_seen: Counter,
    pub trades_unmatched: Counter,
    pub trade_buffer_drops: Counter,
    pub ring_full: Counter,
    pub nonces_free: Gauge,
    pub rslog_dropped: Gauge,
    pub decision_p50_ns: Gauge,
    pub decision_p99_ns: Gauge,
    pub event_p99_ns: Gauge,
    pub killed: Gauge,
    pub mode: Gauge,
    pub armset_ver: Gauge,
    pub armset_age_s: Gauge,
    pub lease_owned: Gauge,
    pub funding_events: Counter,
    pub watch_dynamic: Gauge,
    pub feed_connected: [Gauge; SRC_COUNT],
    pub feed_decode_miss: [Gauge; SRC_COUNT],
    pub feed_since_frame_ms: [Gauge; SRC_COUNT],
    pub feed_reconnects: [Gauge; SRC_COUNT],
    pub state_written: Gauge,
    pub state_errors: Gauge,
}

fn child<M: Clone + Default>(f: &Family<Labels, M>, k: &str, v: &str) -> M {
    f.get_or_create(&vec![(k.to_string(), v.to_string())])
        .clone()
}

impl HotStats {
    /// Create and register every metric.
    pub fn new(reg: &mut Registry) -> Self {
        let fam_c = |reg: &mut Registry, name: &str, help: &str| {
            let f = Family::<Labels, Counter>::default();
            reg.register(name, help, f.clone());
            f
        };
        let fam_g = |reg: &mut Registry, name: &str, help: &str| {
            let f = Family::<Labels, Gauge>::default();
            reg.register(name, help, f.clone());
            f
        };
        let c = |reg: &mut Registry, name: &str, help: &str| {
            let m = Counter::default();
            reg.register(name, help, m.clone());
            m
        };
        let g = |reg: &mut Registry, name: &str, help: &str| {
            let m = Gauge::default();
            reg.register(name, help, m.clone());
            m
        };
        let fe = fam_c(reg, "rugsnipe_feed_events", "Feed events by source");
        let cr = fam_c(
            reg,
            "rugsnipe_creates",
            "Creates seen (first copy) by source",
        );
        let fc = fam_g(reg, "rugsnipe_feed_connected", "Feed connected");
        let fm = fam_g(
            reg,
            "rugsnipe_feed_decode_miss",
            "Creates without a decodable CreateEvent",
        );
        let fs = fam_g(
            reg,
            "rugsnipe_feed_since_frame_ms",
            "Milliseconds since the feed's last frame",
        );
        let fr = fam_g(reg, "rugsnipe_feed_reconnects", "Feed reconnects");
        let dec = fam_c(reg, "rugsnipe_decisions", "Create decisions by outcome");
        let snd = fam_c(reg, "rugsnipe_sends", "Variants handed to senders");
        let ex = fam_c(reg, "rugsnipe_exits", "Sell fills by exit reason");
        let src = |f: &Family<Labels, Counter>| -> [Counter; SRC_COUNT] {
            std::array::from_fn(|i| child(f, "src", FeedSrc::ALL[i].name()))
        };
        let srcg = |f: &Family<Labels, Gauge>| -> [Gauge; SRC_COUNT] {
            std::array::from_fn(|i| child(f, "src", FeedSrc::ALL[i].name()))
        };
        let mut decisions: [Option<Counter>; 256] = std::array::from_fn(|_| None);
        for (k, n) in DECISIONS {
            decisions[*k as usize] = Some(child(&dec, "outcome", n));
        }
        let senders = SenderId::ALL;
        Self {
            feed_events: src(&fe),
            creates: src(&cr),
            dups: c(reg, "rugsnipe_dups", "Duplicate events (cross-feed)"),
            wire_misses: c(
                reg,
                "rugsnipe_wire_decode_errors",
                "Wire payloads that failed to decode",
            ),
            decisions,
            decisions_other: child(&dec, "outcome", "other"),
            intents: c(reg, "rugsnipe_intents", "Intents (paper or real)"),
            sends: std::array::from_fn(|i| child(&snd, "sender", senders[i].name())),
            fills: c(reg, "rugsnipe_paper_fills", "Paper buys filled (E2)"),
            fills_late: c(
                reg,
                "rugsnipe_paper_fills_late",
                "Paper fills from the first trade seen after slot+1",
            ),
            unfilled: c(
                reg,
                "rugsnipe_paper_unfilled",
                "Paper buys unfilled (E2 above v_max)",
            ),
            positions_open: g(reg, "rugsnipe_positions_open", "Open positions"),
            exits: std::array::from_fn(|i| child(&ex, "reason", EXIT_REASONS[i])),
            sells: c(reg, "rugsnipe_sell_attempts", "Sell attempts built"),
            sell_fills: c(reg, "rugsnipe_sell_fills", "Sell fills (paper simulated)"),
            sells_not_owner: c(
                reg,
                "rugsnipe_sells_not_owner",
                "Sells suppressed: exits lease not held",
            ),
            alerts: c(reg, "rugsnipe_alerts", "Critical exit alerts"),
            dump_signals: c(reg, "rugsnipe_dump_signals", "Dump signals"),
            realized_lamports: g(
                reg,
                "rugsnipe_realized_lamports",
                "Realized P&L of closed positions (lamports)",
            ),
            trades_seen: c(reg, "rugsnipe_trades", "Trade events on watched mints"),
            trades_unmatched: c(reg, "rugsnipe_trades_unmatched", "Trades with no position"),
            trade_buffer_drops: c(
                reg,
                "rugsnipe_trade_buffer_drops",
                "Trades dropped from a pending position's buffer",
            ),
            ring_full: c(
                reg,
                "rugsnipe_ring_full",
                "Messages dropped on a full inter-thread ring",
            ),
            nonces_free: g(reg, "rugsnipe_nonces_free", "Free buy nonces"),
            rslog_dropped: g(
                reg,
                "rugsnipe_rslog_dropped",
                "rslog records dropped (ring full)",
            ),
            decision_p50_ns: g(
                reg,
                "rugsnipe_decision_p50_ns",
                "Create event -> primary send, p50 (ns)",
            ),
            decision_p99_ns: g(
                reg,
                "rugsnipe_decision_p99_ns",
                "Create event -> primary send, p99 (ns)",
            ),
            event_p99_ns: g(
                reg,
                "rugsnipe_event_p99_ns",
                "Per-event hot loop time, p99 (ns)",
            ),
            killed: g(reg, "rugsnipe_killed", "Kill latch set"),
            mode: g(reg, "rugsnipe_mode", "0 paper, 1 probe, 2 live"),
            armset_ver: g(reg, "rugsnipe_armset_ver", "Loaded ArmSet version"),
            armset_age_s: g(reg, "rugsnipe_armset_age_s", "ArmSet age"),
            lease_owned: g(
                reg,
                "rugsnipe_exits_lease_owned",
                "This process holds the exits lease",
            ),
            funding_events: c(
                reg,
                "rugsnipe_funding_events",
                "Funding transfers from watched funders",
            ),
            watch_dynamic: g(reg, "rugsnipe_watch_dynamic", "Dynamic watchlist entries"),
            feed_connected: srcg(&fc),
            feed_decode_miss: srcg(&fm),
            feed_since_frame_ms: srcg(&fs),
            feed_reconnects: srcg(&fr),
            state_written: g(reg, "rugsnipe_state_written", "state.db statements written"),
            state_errors: g(reg, "rugsnipe_state_errors", "state.db write errors"),
        }
    }

    /// Count one decision outcome.
    #[inline]
    pub fn decision(&self, code: u8) {
        match &self.decisions[code as usize] {
            Some(c) => {
                c.inc();
            }
            None => {
                self.decisions_other.inc();
            }
        }
    }

    /// Current count of a decision outcome.
    pub fn decision_count(&self, code: u8) -> u64 {
        self.decisions[code as usize]
            .as_ref()
            .map(|c| c.get())
            .unwrap_or(0)
    }

    /// `(label, count)` of every non-zero decision outcome.
    pub fn decision_counts(&self) -> Vec<(&'static str, u64)> {
        let mut v: Vec<(&'static str, u64)> = DECISIONS
            .iter()
            .map(|(k, n)| (*n, self.decision_count(*k)))
            .filter(|(_, n)| *n > 0)
            .collect();
        if self.decisions_other.get() > 0 {
            v.push(("other", self.decisions_other.get()));
        }
        v
    }
}

/// Text exposition of a registry (OpenMetrics).
pub fn encode(reg: &Registry) -> String {
    let mut s = String::new();
    let _ = prometheus_client::encoding::text::encode(&mut s, reg);
    s
}
