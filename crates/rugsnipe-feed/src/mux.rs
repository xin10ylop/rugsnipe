//! [`FeedMux`]: polls several feeds into one buffer and de-duplicates across
//! them by signature (xxh3 ring of 65,536). A duplicate keeps its slot in the
//! buffer with its payload replaced by [`Payload::Dup`] so the caller records
//! `Dup{src, delay}` (docs/06 section 7.1 step 2) without a second lookup.

use crate::buf::FeedBuf;
use crate::dedup::{Seen, SigDedup};
use crate::event::{wire_first_sig, DupInfo, FeedSrc, Payload, SRC_COUNT};
use crate::health::FeedHealth;
use crate::{Feed, FeedStatus};

/// Several feeds behind one [`Feed`].
pub struct FeedMux {
    feeds: Vec<Box<dyn Feed + Send>>,
    dedup: SigDedup,
    /// Duplicates seen per source (the late copy's source).
    pub dups: [u64; SRC_COUNT],
    /// Sum of duplicate delays per source (ns).
    pub dup_delay_ns: [u64; SRC_COUNT],
    /// First deliveries per source (who won the race).
    pub firsts: [u64; SRC_COUNT],
    next: usize,
}

impl std::fmt::Debug for FeedMux {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FeedMux")
            .field(
                "feeds",
                &self.feeds.iter().map(|x| x.src()).collect::<Vec<_>>(),
            )
            .field("dups", &self.dups)
            .field("firsts", &self.firsts)
            .finish()
    }
}

impl FeedMux {
    pub fn new(feeds: Vec<Box<dyn Feed + Send>>) -> FeedMux {
        FeedMux {
            feeds,
            dedup: SigDedup::new(),
            dups: [0; SRC_COUNT],
            dup_delay_ns: [0; SRC_COUNT],
            firsts: [0; SRC_COUNT],
            next: 0,
        }
    }

    pub fn feeds(&self) -> &[Box<dyn Feed + Send>] {
        &self.feeds
    }

    /// Health of each child feed.
    pub fn healths(&self) -> impl Iterator<Item = &FeedHealth> {
        self.feeds.iter().filter_map(|f| f.health())
    }

    fn dedup_range(&mut self, out: &mut FeedBuf, from: usize) {
        for i in from..out.len() {
            let ev = out.events()[i];
            let key = ev.dedup_key(|w| wire_first_sig(out.wire(w)).map(xxhash_rust::xxh3::xxh3_64));
            let Some(key) = key else { continue };
            match self.dedup.insert(key, ev.src, ev.recv_ns) {
                Seen::New => self.firsts[ev.src as usize] += 1,
                Seen::Dup {
                    first_src,
                    first_recv_ns,
                } => {
                    let delay = ev.recv_ns.saturating_sub(first_recv_ns);
                    self.dups[ev.src as usize] += 1;
                    self.dup_delay_ns[ev.src as usize] += delay;
                    out.events_mut()[i].payload = Payload::Dup(DupInfo {
                        first_src,
                        delay_ns: delay,
                        key,
                    });
                }
            }
        }
    }
}

impl Feed for FeedMux {
    fn poll(&mut self, out: &mut FeedBuf) -> FeedStatus {
        let start = out.len();
        let n = self.feeds.len();
        let (mut ended, mut disconnected, mut stale) = (0, 0, 0);
        for k in 0..n {
            if out.is_full() {
                break;
            }
            let i = (self.next + k) % n;
            match self.feeds[i].poll(out) {
                FeedStatus::Ended => ended += 1,
                FeedStatus::Disconnected => disconnected += 1,
                FeedStatus::Stale => stale += 1,
                _ => {}
            }
        }
        self.next = (self.next + 1) % n.max(1);
        self.dedup_range(out, start);
        let got = out.len() - start;
        if got > 0 {
            FeedStatus::Ready(got)
        } else if out.is_full() {
            FeedStatus::Full
        } else if n > 0 && ended == n {
            FeedStatus::Ended
        } else if n > 0 && ended + disconnected == n {
            FeedStatus::Disconnected
        } else if stale > 0 {
            FeedStatus::Stale
        } else {
            FeedStatus::Idle
        }
    }

    fn src(&self) -> FeedSrc {
        self.feeds.first().map_or(FeedSrc::Replay, |f| f.src())
    }
}
