//! [`Replay`]: a feed over recorded frames, for tests and benchmarks.
//!
//! Input is JSONL, one frame per line, in any of these forms:
//!
//! * recorder lines: `{"recv_ns":N,"recv_unix_ns":W,"src":"logsws","frame":{...raw notification...}}`
//!   (`src` may be `logsws`, `pumpportal`, `relay`, ...);
//! * wire lines: `{"recv_ns":N,"src":"grpc","slot":S,"wire_b64":"<base64 transaction>"}`;
//! * collector relay lines: `<recv_unix_ns>\t<PumpPortal frame>`.
//!
//! Frames are decoded once at load; `poll()` only copies prepared events (and
//! wire bytes) into the caller's [`FeedBuf`], so it never allocates.

use crate::buf::FeedBuf;
use crate::clock::mono_ns;
use crate::decode::FrameDecoder;
use crate::event::{FeedEvent, FeedSrc, Payload, WireRef};
use crate::health::FeedHealth;
use crate::{Feed, FeedStatus};
use base64::Engine as _;
use serde::Deserialize;
use serde_json::value::RawValue;
use std::io::{self, BufRead};
use std::path::Path;

/// Replay pacing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pace {
    /// Emit as fast as the caller polls.
    AsFast,
    /// Respect recorded `recv_ns` gaps divided by `speed`.
    Realtime { speed: f64 },
}

#[derive(Deserialize)]
struct RecLine<'a> {
    #[serde(default)]
    recv_ns: u64,
    #[serde(borrow, default)]
    src: Option<&'a str>,
    #[serde(borrow, default)]
    frame: Option<&'a RawValue>,
    #[serde(borrow, default)]
    frame_text: Option<std::borrow::Cow<'a, str>>,
    #[serde(default)]
    slot: u64,
    #[serde(borrow, default)]
    wire_b64: Option<&'a str>,
}

/// Recorded frames as a feed.
#[derive(Debug)]
pub struct Replay {
    events: Vec<FeedEvent>,
    /// Wire bytes of the prepared events (`WireRef`s index this arena).
    wire: Vec<u8>,
    pos: usize,
    pace: Pace,
    /// (mono at first poll, first recv_ns)
    start: Option<(u64, u64)>,
    health: FeedHealth,
    restamp: bool,
    /// Lines that could not be parsed.
    pub bad_lines: u64,
}

impl Replay {
    /// Load from text. `emit_trades` also emits the TradeEvents of logs frames.
    pub fn from_text(text: &str, emit_trades: bool) -> Replay {
        let mut r = Replay {
            events: Vec::new(),
            wire: Vec::new(),
            pos: 0,
            pace: Pace::AsFast,
            start: None,
            health: FeedHealth::new(FeedSrc::Replay, 0),
            restamp: false,
            bad_lines: 0,
        };
        let mut dec = FrameDecoder::new();
        for line in text.lines() {
            r.load_line(line, emit_trades, &mut dec);
        }
        r.health
            .connected
            .store(true, std::sync::atomic::Ordering::Relaxed);
        r
    }

    /// Load a file.
    pub fn from_path(path: impl AsRef<Path>, emit_trades: bool) -> io::Result<Replay> {
        let f = std::fs::File::open(path)?;
        let mut text = String::new();
        for l in io::BufReader::new(f).lines() {
            text.push_str(&l?);
            text.push('\n');
        }
        Ok(Self::from_text(&text, emit_trades))
    }

    fn load_line(&mut self, line: &str, emit_trades: bool, dec: &mut FrameDecoder) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        // collector relay form: "<unix_ns>\t<frame>"
        if let Some((ns, frame)) = line.split_once('\t') {
            if let Ok(ns) = ns.parse::<u64>() {
                self.health.frame();
                let events = &mut self.events;
                dec.any_frame(
                    FeedSrc::Relay,
                    ns,
                    frame,
                    emit_trades,
                    &self.health,
                    &mut |ev| events.push(ev),
                );
                return;
            }
        }
        let Ok(rec) = serde_json::from_str::<RecLine<'_>>(line) else {
            self.bad_lines += 1;
            return;
        };
        let src = rec
            .src
            .and_then(FeedSrc::from_name)
            .unwrap_or(FeedSrc::Replay);
        self.health.frame();
        if let Some(b64) = rec.wire_b64 {
            match base64::engine::general_purpose::STANDARD.decode(b64) {
                Ok(bytes) => {
                    let off = self.wire.len() as u32;
                    self.wire.extend_from_slice(&bytes);
                    self.health.event(rec.slot);
                    self.events.push(FeedEvent {
                        src,
                        recv_ns: rec.recv_ns,
                        slot: rec.slot,
                        bank_id: 0,
                        payload: Payload::Wire(WireRef {
                            off,
                            len: bytes.len() as u32,
                            meta_len: 0,
                        }),
                    });
                }
                Err(_) => self.bad_lines += 1,
            }
            return;
        }
        let events = &mut self.events;
        let mut emit = |ev: FeedEvent| events.push(ev);
        if let Some(f) = rec.frame {
            dec.any_frame(
                src,
                rec.recv_ns,
                f.get(),
                emit_trades,
                &self.health,
                &mut emit,
            );
        } else if let Some(t) = rec.frame_text {
            dec.any_frame(src, rec.recv_ns, &t, emit_trades, &self.health, &mut emit);
        } else {
            self.bad_lines += 1;
        }
    }

    /// Set the pacing (default [`Pace::AsFast`]).
    pub fn with_pace(mut self, pace: Pace) -> Replay {
        self.pace = pace;
        self
    }

    /// Stamp events with the poll time instead of the recorded `recv_ns`.
    pub fn restamped(mut self) -> Replay {
        self.restamp = true;
        self
    }

    /// Prepared events (all of them, independent of the read position).
    pub fn events(&self) -> &[FeedEvent] {
        &self.events
    }

    /// Wire bytes of a prepared event.
    pub fn wire(&self, w: WireRef) -> &[u8] {
        let (o, l) = (w.off as usize, w.len as usize);
        self.wire.get(o..o + l).unwrap_or(&[])
    }

    /// Restart from the first event.
    pub fn rewind(&mut self) {
        self.pos = 0;
        self.start = None;
    }

    pub fn remaining(&self) -> usize {
        self.events.len() - self.pos
    }

    /// Load-time counters (creates, misses, failed transactions, ...).
    pub fn load_health(&self) -> &FeedHealth {
        &self.health
    }
}

impl Feed for Replay {
    fn poll(&mut self, out: &mut FeedBuf) -> FeedStatus {
        if self.pos >= self.events.len() {
            return FeedStatus::Ended;
        }
        let now = mono_ns();
        let first = self.events[self.pos].recv_ns;
        let (t0, r0) = *self.start.get_or_insert((now, first));
        let mut n = 0;
        while self.pos < self.events.len() {
            let mut ev = self.events[self.pos];
            if let Pace::Realtime { speed } = self.pace {
                let due = ((ev.recv_ns.saturating_sub(r0)) as f64 / speed.max(1e-9)) as u64;
                if now.saturating_sub(t0) < due {
                    break;
                }
            }
            if self.restamp {
                ev.recv_ns = now;
            }
            let ok = match ev.payload {
                Payload::Wire(w) => {
                    let (o, l) = (w.off as usize, w.len as usize);
                    out.push_wire_parts(
                        ev.src,
                        ev.recv_ns,
                        ev.slot,
                        ev.bank_id,
                        [&self.wire[o..o + l], &[]],
                        w.meta_len as usize,
                    )
                }
                _ => out.push(ev),
            };
            if !ok {
                break;
            }
            self.pos += 1;
            n += 1;
        }
        if n > 0 {
            FeedStatus::Ready(n)
        } else if out.is_full() {
            FeedStatus::Full
        } else {
            FeedStatus::Idle
        }
    }

    fn src(&self) -> FeedSrc {
        FeedSrc::Replay
    }

    fn health(&self) -> Option<&FeedHealth> {
        Some(&self.health)
    }
}
