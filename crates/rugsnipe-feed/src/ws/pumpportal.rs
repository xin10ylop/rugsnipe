//! [`PumpPortal`]: `wss://pumpportal.fun/api/data` `subscribeNewToken` ->
//! partial [`crate::Payload::Hit`]s. **Optional and default off**: PumpPortal
//! allows one connection per IP and the collector normally holds it (its
//! frames reach us through [`crate::relay::CollectorRelay`] instead).

use super::net::ws_connect;
use crate::buf::FeedBuf;
use crate::clock::{mono_ns, unix_ns};
use crate::decode::FrameDecoder;
use crate::event::FeedSrc;
use crate::health::FeedHealth;
use crate::record::{RecordConfig, Recorder};
use crate::ring::{ring, RingFeed};
use crate::{Feed, FeedStatus};
use futures_util::{SinkExt, StreamExt};
use std::io;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

/// PumpPortal settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PumpPortalConfig {
    pub url: String,
    pub stale_ms: u64,
    pub ping_ms: u64,
    pub min_backoff_ms: u64,
    pub max_backoff_ms: u64,
    pub ring_capacity: usize,
    pub record: Option<RecordConfig>,
}

impl Default for PumpPortalConfig {
    fn default() -> Self {
        PumpPortalConfig {
            url: "wss://pumpportal.fun/api/data".into(),
            stale_ms: 60_000,
            ping_ms: 20_000,
            min_backoff_ms: 1_000,
            max_backoff_ms: 60_000,
            ring_capacity: 4096,
            record: None,
        }
    }
}

/// The PumpPortal adapter.
#[derive(Debug)]
pub struct PumpPortal {
    feed: RingFeed,
}

impl PumpPortal {
    /// Start the websocket thread (opens the one allowed connection).
    pub fn spawn(cfg: PumpPortalConfig) -> io::Result<PumpPortal> {
        let (mut sink, mut feed, stop) = ring(FeedSrc::PumpPortal, cfg.ring_capacity, cfg.stale_ms);
        let recorder = match &cfg.record {
            Some(rc) => Some(Recorder::spawn(rc.clone())?.0),
            None => None,
        };
        let h = std::thread::Builder::new()
            .name("feed-pumpportal".into())
            .spawn(move || {
                let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                rt.block_on(async move {
                    let health = sink.health().clone();
                    let mut dec = FrameDecoder::new();
                    let mut backoff = cfg.min_backoff_ms;
                    while !stop.load(Ordering::Acquire) {
                        let Ok(mut ws) = ws_connect(&cfg.url, Duration::from_secs(20)).await else {
                            FeedHealth::inc(&health.reconnects);
                            tokio::time::sleep(Duration::from_millis(backoff)).await;
                            backoff = (backoff * 2).min(cfg.max_backoff_ms);
                            continue;
                        };
                        backoff = cfg.min_backoff_ms;
                        health.connected.store(true, Ordering::Relaxed);
                        health.last_frame_ns.store(mono_ns(), Ordering::Relaxed);
                        if ws
                            .send(Message::Text(r#"{"method":"subscribeNewToken"}"#.into()))
                            .await
                            .is_err()
                        {
                            continue;
                        }
                        let mut ping = tokio::time::interval(Duration::from_millis(
                            cfg.ping_ms.min(cfg.stale_ms / 3).max(500),
                        ));
                        let mut check = tokio::time::interval(Duration::from_millis(250));
                        loop {
                            if stop.load(Ordering::Acquire) {
                                break;
                            }
                            tokio::select! {
                                m = ws.next() => match m {
                                    Some(Ok(Message::Text(t))) => {
                                        let recv = mono_ns();
                                        health.frame();
                                        if let Some(r) = &recorder {
                                            r.record(FeedSrc::PumpPortal, recv, unix_ns(), t.as_str());
                                        }
                                        dec.pp_frame(FeedSrc::PumpPortal, recv, t.as_str(), &health, &mut |ev| { sink.push(ev); });
                                    }
                                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                                    Some(Ok(_)) => health.last_frame_ns.store(mono_ns(), Ordering::Relaxed),
                                },
                                _ = ping.tick() => {
                                    if ws.send(Message::Ping(Vec::new().into())).await.is_err() { break; }
                                }
                                _ = check.tick() => {
                                    if health.is_stale_at(mono_ns()) {
                                        FeedHealth::inc(&health.stale_reconnects);
                                        break;
                                    }
                                }
                            }
                        }
                        health.connected.store(false, Ordering::Relaxed);
                        FeedHealth::inc(&health.reconnects);
                        tokio::time::sleep(Duration::from_millis(backoff)).await;
                    }
                });
            })?;
        feed.set_thread(h);
        Ok(PumpPortal { feed })
    }

    pub fn shutdown(&mut self) {
        self.feed.shutdown();
    }
}

impl Feed for PumpPortal {
    #[inline]
    fn poll(&mut self, out: &mut FeedBuf) -> FeedStatus {
        self.feed.poll(out)
    }

    fn src(&self) -> FeedSrc {
        FeedSrc::PumpPortal
    }

    fn health(&self) -> Option<&FeedHealth> {
        self.feed.health()
    }
}
