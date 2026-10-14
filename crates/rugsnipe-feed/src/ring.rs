//! Thread -> hot loop transport: adapters push [`FeedEvent`]s into an `rtrb`
//! single-producer single-consumer ring; [`RingFeed`] pops them in `poll()`
//! (lock-free, allocation-free).

use crate::buf::FeedBuf;
use crate::event::{FeedEvent, FeedSrc};
use crate::health::FeedHealth;
use crate::{Feed, FeedStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

/// Producer side, owned by the adapter thread.
#[derive(Debug)]
pub struct RingSink {
    prod: rtrb::Producer<FeedEvent>,
    health: Arc<FeedHealth>,
}

impl RingSink {
    /// Push; a full ring drops the event and counts it in `ring_drops`.
    #[inline]
    pub fn push(&mut self, ev: FeedEvent) -> bool {
        match self.prod.push(ev) {
            Ok(()) => true,
            Err(_) => {
                FeedHealth::inc(&self.health.ring_drops);
                false
            }
        }
    }

    pub fn health(&self) -> &Arc<FeedHealth> {
        &self.health
    }
}

/// A feed whose events arrive from another thread through a ring.
#[derive(Debug)]
pub struct RingFeed {
    src: FeedSrc,
    cons: rtrb::Consumer<FeedEvent>,
    health: Arc<FeedHealth>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

/// A connected pair: give the [`RingSink`] to the adapter thread.
pub fn ring(src: FeedSrc, capacity: usize, stale_ms: u64) -> (RingSink, RingFeed, Arc<AtomicBool>) {
    let (prod, cons) = rtrb::RingBuffer::new(capacity.max(16));
    let health = Arc::new(FeedHealth::new(src, stale_ms));
    let stop = Arc::new(AtomicBool::new(false));
    (
        RingSink {
            prod,
            health: health.clone(),
        },
        RingFeed {
            src,
            cons,
            health,
            stop: stop.clone(),
            thread: None,
        },
        stop,
    )
}

impl RingFeed {
    /// Attach the adapter thread (joined on [`RingFeed::shutdown`]).
    pub fn set_thread(&mut self, h: JoinHandle<()>) {
        self.thread = Some(h);
    }

    pub fn health_arc(&self) -> &Arc<FeedHealth> {
        &self.health
    }

    /// Ask the adapter thread to stop and wait for it.
    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.thread.take() {
            let _ = h.join();
        }
    }
}

impl Drop for RingFeed {
    fn drop(&mut self) {
        // do not block in drop: the thread notices the flag and exits
        self.stop.store(true, Ordering::Release);
    }
}

impl Feed for RingFeed {
    fn poll(&mut self, out: &mut FeedBuf) -> FeedStatus {
        let n = out.room().min(self.cons.slots());
        let mut got = 0;
        for _ in 0..n {
            match self.cons.pop() {
                Ok(ev) => {
                    out.push(ev);
                    got += 1;
                }
                Err(_) => break,
            }
        }
        if got > 0 {
            return FeedStatus::Ready(got);
        }
        if out.is_full() {
            return FeedStatus::Full;
        }
        if self.cons.is_abandoned() {
            return FeedStatus::Ended;
        }
        if !self.health.connected.load(Ordering::Relaxed) {
            return FeedStatus::Disconnected;
        }
        if self.health.is_stale_at(crate::clock::mono_ns()) {
            return FeedStatus::Stale;
        }
        FeedStatus::Idle
    }

    fn src(&self) -> FeedSrc {
        self.src
    }

    fn health(&self) -> Option<&FeedHealth> {
        Some(&self.health)
    }
}
