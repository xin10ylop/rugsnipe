//! # rugsnipe-feed
//!
//! Feed adapters for `rugsnipe` (docs/06-architecture.md sections 3 and 7.1).
//! Every adapter implements
//!
//! ```text
//! trait Feed { fn poll(&mut self, out: &mut FeedBuf) -> FeedStatus }
//! ```
//!
//! and produces [`FeedEvent`]`{src, recv_ns, slot, bank_id, payload}` where the
//! payload is wire bytes in the [`FeedBuf`] arena ([`Payload::Wire`]) or a
//! heap-free [`CreateHitLite`] ([`Payload::Hit`]), plus funding transfers and
//! watched-mint trades.
//!
//! | adapter | source | notes |
//! |---|---|---|
//! | [`ws::LogsWs`] | `logsSubscribe(mentions: TSLvdd…)` at `processed` | free, paper only; reconnect + backoff, ping, stale detector; per-address watches for funders (cold `getTransaction`) and held mints |
//! | [`ws::PumpPortal`] | `subscribeNewToken` | optional, **default off** (one connection per IP) |
//! | [`relay::CollectorRelay`] | collector frames on a Unix socket | optional |
//! | [`replay::Replay`] | recorded JSONL frames | tests and benchmarks; no network |
//!
//! Around them: [`FeedMux`] (cross-feed dedup by signature over a 65,536-entry
//! xxh3 ring, duplicates turned into [`Payload::Dup`]), [`FiredSet`]
//! (first-wins per mint and class, atomics), [`FeedHealth`] (lag, last
//! event, reconnects, decode misses) and [`record::Recorder`] (bounded raw
//! frame recording for later replay).
//!
//! ## Threads and allocation
//!
//! Websocket adapters run on their own thread (a current-thread tokio
//! runtime) and push `Copy` events into an `rtrb` ring; [`Feed::poll`] only
//! pops from the ring (or copies prepared replay events) into a [`FeedBuf`]
//! whose capacities are fixed, so the hot loop is allocation-free after
//! warm-up (checked by `tests/alloc.rs` with a counting allocator). JSON
//! parsing and log decoding happen on the adapter thread (or at replay load).
//!
//! Without the default `ws` feature the crate has no tokio/TLS dependency
//! (replay, relay, dedup, FiredSet, health only).
//!
//! ## Decoders used
//!
//! `pump_codec::events::{scan_program_data_logs, decode_event_payload}` for
//! the `Program data:` lines. Local helpers (not in pump-codec): the
//! `logsNotification` / PumpPortal / `getTransaction(jsonParsed)` JSON
//! decoders in [`decode`].
//!
//! ## Example
//!
//! ```
//! use rugsnipe_feed::{Feed, FeedBuf, FeedMux, FeedStatus, Payload, replay::Replay};
//! let frames = r#"{"recv_ns":1,"src":"pumpportal","frame":{"signature":"5J7T55JQzepJ1UQF5rsgjav1SXWoKr8jzSJx2dDBTcidSy3nXUV8PBcqveo8X859JYUQesZh7TFUXd7bAPxJe5rg","mint":"J7SgVKDNGFSQwmC9PR5FRa1Y3EJd9B3HWSVGLzekpump","txType":"create","solAmount":0.5}}"#;
//! let mut mux = FeedMux::new(vec![Box::new(Replay::from_text(frames, false))]);
//! let mut buf = FeedBuf::with_capacity(256, 1 << 16);
//! assert_eq!(mux.poll(&mut buf), FeedStatus::Ready(1));
//! assert!(matches!(buf.events()[0].payload, Payload::Hit(h) if h.intx_sol_in == 500_000_000));
//! ```

#![forbid(unsafe_op_in_unsafe_fn)]
#![warn(missing_debug_implementations)]

pub mod buf;
pub mod clock;
pub mod config;
pub mod decode;
pub mod dedup;
pub mod event;
pub mod fired;
pub mod health;
pub mod mux;
pub mod record;
pub mod relay;
pub mod replay;
pub mod ring;
#[cfg(feature = "ws")]
pub mod ws;

pub use buf::FeedBuf;
pub use dedup::{Seen, SigDedup};
pub use event::{
    hit_flags, CreateHitLite, DupInfo, FeedEvent, FeedSrc, FundingEvent, Payload, Sig, TradeLite,
    WireRef,
};
pub use fired::{Fired, FiredSet};
pub use health::{FeedHealth, HealthSnapshot};
pub use mux::FeedMux;

/// Result of one [`Feed::poll`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedStatus {
    /// `n` events were appended.
    Ready(usize),
    /// Nothing new.
    Idle,
    /// Connected but no frame for `stale_ms` (a reconnect is under way).
    Stale,
    /// Not connected (reconnecting with backoff).
    Disconnected,
    /// The output buffer is full: drain it and poll again.
    Full,
    /// No more events (replay finished, or the adapter thread is gone).
    Ended,
}

/// A source of feed events. `poll` must not block and must not allocate
/// after warm-up.
pub trait Feed {
    /// Append available events to `out` (up to its free capacity).
    fn poll(&mut self, out: &mut FeedBuf) -> FeedStatus;
    /// The adapter's source id.
    fn src(&self) -> FeedSrc;
    /// Live counters, if the adapter keeps them.
    fn health(&self) -> Option<&FeedHealth> {
        None
    }
}
