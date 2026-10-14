//! [`CollectorRelay`]: frames relayed by the Python collector over a Unix
//! socket (`<recv_unix_ns>\t<frame>\n`, normally PumpPortal `subscribeNewToken`
//! frames; logs notifications are decoded too). Optional, default off
//! (`[feeds] relay = { enabled = false, socket = "/run/rugsnipe/pp.sock" }`).

use crate::clock::mono_ns;
use crate::decode::FrameDecoder;
use crate::event::FeedSrc;
use crate::health::FeedHealth;
use crate::ring::{ring, RingFeed};
use std::io::{BufRead, BufReader, ErrorKind};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// Relay settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayConfig {
    pub socket: PathBuf,
    pub ring_capacity: usize,
    pub stale_ms: u64,
    pub max_backoff_ms: u64,
}

impl RelayConfig {
    pub fn new(socket: impl Into<PathBuf>) -> RelayConfig {
        RelayConfig {
            socket: socket.into(),
            ring_capacity: 4096,
            stale_ms: 30_000,
            max_backoff_ms: 30_000,
        }
    }
}

/// Collector relay adapter.
#[derive(Debug)]
pub struct CollectorRelay;

impl CollectorRelay {
    /// Start the reader thread; returns the feed.
    pub fn spawn(cfg: RelayConfig) -> std::io::Result<RingFeed> {
        let (mut sink, mut feed, stop) = ring(FeedSrc::Relay, cfg.ring_capacity, cfg.stale_ms);
        let h = std::thread::Builder::new()
            .name("feed-relay".into())
            .spawn(move || {
                let health = sink.health().clone();
                let mut dec = FrameDecoder::new();
                let mut backoff = 250u64;
                let mut line = String::with_capacity(4096);
                while !stop.load(Ordering::Acquire) {
                    let stream = match UnixStream::connect(&cfg.socket) {
                        Ok(s) => s,
                        Err(_) => {
                            sleep_unless(&stop, backoff);
                            backoff = (backoff * 2).min(cfg.max_backoff_ms);
                            continue;
                        }
                    };
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                    health.connected.store(true, Ordering::Relaxed);
                    backoff = 250;
                    let mut r = BufReader::new(stream);
                    loop {
                        if stop.load(Ordering::Acquire) {
                            break;
                        }
                        line.clear();
                        match r.read_line(&mut line) {
                            Ok(0) => break,
                            Ok(_) => {
                                let recv = mono_ns();
                                health.frame();
                                let frame = line
                                    .trim_end()
                                    .split_once('\t')
                                    .map(|(_, f)| f)
                                    .unwrap_or(line.trim_end());
                                dec.any_frame(
                                    FeedSrc::Relay,
                                    recv,
                                    frame,
                                    false,
                                    &health,
                                    &mut |ev| {
                                        sink.push(ev);
                                    },
                                );
                            }
                            Err(e)
                                if e.kind() == ErrorKind::WouldBlock
                                    || e.kind() == ErrorKind::TimedOut =>
                            {
                                continue
                            }
                            Err(_) => break,
                        }
                    }
                    health.connected.store(false, Ordering::Relaxed);
                    FeedHealth::inc(&health.reconnects);
                }
            })?;
        feed.set_thread(h);
        Ok(feed)
    }
}

fn sleep_unless(stop: &std::sync::atomic::AtomicBool, ms: u64) {
    let mut left = ms;
    while left > 0 && !stop.load(Ordering::Acquire) {
        let d = left.min(100);
        std::thread::sleep(Duration::from_millis(d));
        left -= d;
    }
}
