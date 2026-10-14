//! [`FeedHealth`]: per-feed counters shared between the adapter thread and the
//! hot loop (relaxed atomics; read with [`FeedHealth::snapshot`]).

use crate::clock::mono_ns;
use crate::event::FeedSrc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};

/// Live counters of one feed.
#[derive(Debug)]
pub struct FeedHealth {
    pub src: FeedSrc,
    pub connected: AtomicBool,
    /// Raw frames received.
    pub frames: AtomicU64,
    /// Events produced.
    pub events: AtomicU64,
    /// Creates decoded (CreateEvent found).
    pub creates: AtomicU64,
    /// Frames that looked like creates (create instruction logged) but had no decodable CreateEvent.
    pub decode_miss: AtomicU64,
    /// Event payloads that failed to decode.
    pub decode_errors: AtomicU64,
    /// Failed transactions (`err != null`), ignored.
    pub failed_tx: AtomicU64,
    /// Truncated log sets.
    pub truncated: AtomicU64,
    pub reconnects: AtomicU64,
    /// Forced reconnects by the stale detector.
    pub stale_reconnects: AtomicU64,
    /// Events dropped because the ring to the hot loop was full.
    pub ring_drops: AtomicU64,
    /// Funding lookups (getTransaction) issued / failed / dropped (queue full).
    pub rpc_calls: AtomicU64,
    pub rpc_errors: AtomicU64,
    pub rpc_drops: AtomicU64,
    /// Monotonic time of the last frame / event (0 = never).
    pub last_frame_ns: AtomicU64,
    pub last_event_ns: AtomicU64,
    pub last_slot: AtomicU64,
    /// Latest measured notification lag versus block time (ms), set by the
    /// caller that measures it (e.g. `examples/tail.rs`); `u64::MAX` = unknown.
    pub lag_ms: AtomicU64,
    /// Stale threshold in ms (0 = disabled).
    pub stale_ms: AtomicU64,
}

/// Plain copy of [`FeedHealth`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthSnapshot {
    pub src: FeedSrc,
    pub connected: bool,
    pub frames: u64,
    pub events: u64,
    pub creates: u64,
    pub decode_miss: u64,
    pub decode_errors: u64,
    pub failed_tx: u64,
    pub truncated: u64,
    pub reconnects: u64,
    pub stale_reconnects: u64,
    pub ring_drops: u64,
    pub rpc_calls: u64,
    pub rpc_errors: u64,
    pub rpc_drops: u64,
    /// ms since the last frame (`u64::MAX` = never).
    pub since_frame_ms: u64,
    pub last_slot: u64,
    pub lag_ms: u64,
    pub stale: bool,
}

impl HealthSnapshot {
    /// Decoded creates / (decoded + misses); 1.0 when nothing was seen.
    pub fn decode_success_rate(&self) -> f64 {
        let n = self.creates + self.decode_miss;
        if n == 0 {
            1.0
        } else {
            self.creates as f64 / n as f64
        }
    }
}

impl FeedHealth {
    pub fn new(src: FeedSrc, stale_ms: u64) -> FeedHealth {
        FeedHealth {
            src,
            connected: AtomicBool::new(false),
            frames: AtomicU64::new(0),
            events: AtomicU64::new(0),
            creates: AtomicU64::new(0),
            decode_miss: AtomicU64::new(0),
            decode_errors: AtomicU64::new(0),
            failed_tx: AtomicU64::new(0),
            truncated: AtomicU64::new(0),
            reconnects: AtomicU64::new(0),
            stale_reconnects: AtomicU64::new(0),
            ring_drops: AtomicU64::new(0),
            rpc_calls: AtomicU64::new(0),
            rpc_errors: AtomicU64::new(0),
            rpc_drops: AtomicU64::new(0),
            last_frame_ns: AtomicU64::new(0),
            last_event_ns: AtomicU64::new(0),
            last_slot: AtomicU64::new(0),
            lag_ms: AtomicU64::new(u64::MAX),
            stale_ms: AtomicU64::new(stale_ms),
        }
    }

    #[inline]
    pub fn inc(c: &AtomicU64) {
        c.fetch_add(1, Relaxed);
    }

    /// Record a received frame now.
    #[inline]
    pub fn frame(&self) {
        Self::inc(&self.frames);
        self.last_frame_ns.store(mono_ns(), Relaxed);
    }

    /// Record a produced event.
    #[inline]
    pub fn event(&self, slot: u64) {
        Self::inc(&self.events);
        self.last_event_ns.store(mono_ns(), Relaxed);
        if slot > self.last_slot.load(Relaxed) {
            self.last_slot.store(slot, Relaxed);
        }
    }

    /// No frame for `stale_ms` while connected (or never connected since start).
    pub fn is_stale_at(&self, now_ns: u64) -> bool {
        let stale = self.stale_ms.load(Relaxed);
        if stale == 0 {
            return false;
        }
        let last = self.last_frame_ns.load(Relaxed);
        last != 0 && now_ns.saturating_sub(last) > stale * 1_000_000
    }

    pub fn snapshot(&self) -> HealthSnapshot {
        let now = mono_ns();
        let last = self.last_frame_ns.load(Relaxed);
        HealthSnapshot {
            src: self.src,
            connected: self.connected.load(Relaxed),
            frames: self.frames.load(Relaxed),
            events: self.events.load(Relaxed),
            creates: self.creates.load(Relaxed),
            decode_miss: self.decode_miss.load(Relaxed),
            decode_errors: self.decode_errors.load(Relaxed),
            failed_tx: self.failed_tx.load(Relaxed),
            truncated: self.truncated.load(Relaxed),
            reconnects: self.reconnects.load(Relaxed),
            stale_reconnects: self.stale_reconnects.load(Relaxed),
            ring_drops: self.ring_drops.load(Relaxed),
            rpc_calls: self.rpc_calls.load(Relaxed),
            rpc_errors: self.rpc_errors.load(Relaxed),
            rpc_drops: self.rpc_drops.load(Relaxed),
            since_frame_ms: if last == 0 {
                u64::MAX
            } else {
                now.saturating_sub(last) / 1_000_000
            },
            last_slot: self.last_slot.load(Relaxed),
            lag_ms: self.lag_ms.load(Relaxed),
            stale: self.is_stale_at(now),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_detection_and_rates() {
        let h = FeedHealth::new(FeedSrc::LogsWs, 10);
        assert!(!h.is_stale_at(mono_ns()), "never received: not stale yet");
        h.frame();
        let t = h.last_frame_ns.load(Relaxed);
        assert!(!h.is_stale_at(t + 5_000_000));
        assert!(h.is_stale_at(t + 11_000_000));
        FeedHealth::inc(&h.creates);
        FeedHealth::inc(&h.creates);
        FeedHealth::inc(&h.creates);
        FeedHealth::inc(&h.decode_miss);
        h.event(42);
        h.event(41);
        let s = h.snapshot();
        assert_eq!(s.last_slot, 42);
        assert!((s.decode_success_rate() - 0.75).abs() < 1e-9);
    }
}
