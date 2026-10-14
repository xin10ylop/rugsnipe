//! Cross-feed dedup by signature: the last 65,536 keys (xxh3) in a ring, with
//! an open-addressing index. Fixed memory, no allocation after construction.

use crate::event::FeedSrc;

/// Ring size (keys remembered).
pub const DEDUP_RING: usize = 1 << 16;
const TABLE: usize = DEDUP_RING * 2;
const EMPTY: u32 = u32::MAX;

#[derive(Debug, Clone, Copy)]
struct Entry {
    key: u64,
    recv_ns: u64,
    src: u8,
}

/// Result of [`SigDedup::insert`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seen {
    /// First time: keep the event.
    New,
    /// Seen before (from `first_src` at `first_recv_ns`).
    Dup {
        first_src: FeedSrc,
        first_recv_ns: u64,
    },
}

/// Fixed-capacity set of recent keys with first-seen source and time.
#[derive(Debug)]
pub struct SigDedup {
    ring: Box<[Entry]>,
    head: usize,
    filled: usize,
    table: Box<[u32]>,
}

impl Default for SigDedup {
    fn default() -> Self {
        Self::new()
    }
}

impl SigDedup {
    pub fn new() -> SigDedup {
        SigDedup {
            ring: vec![
                Entry {
                    key: 0,
                    recv_ns: 0,
                    src: 0
                };
                DEDUP_RING
            ]
            .into_boxed_slice(),
            head: 0,
            filled: 0,
            table: vec![EMPTY; TABLE].into_boxed_slice(),
        }
    }

    #[inline]
    fn slot_of(key: u64) -> usize {
        // keys are xxh3 outputs: the low bits are uniform
        (key as usize) & (TABLE - 1)
    }

    fn find(&self, key: u64) -> Option<usize> {
        let mut i = Self::slot_of(key);
        loop {
            let r = self.table[i];
            if r == EMPTY {
                return None;
            }
            if self.ring[r as usize].key == key {
                return Some(i);
            }
            i = (i + 1) & (TABLE - 1);
        }
    }

    /// Remove table slot `i` with backward-shift deletion (linear probing).
    fn remove_at(&mut self, mut i: usize) {
        self.table[i] = EMPTY;
        let mut j = i;
        loop {
            j = (j + 1) & (TABLE - 1);
            let r = self.table[j];
            if r == EMPTY {
                return;
            }
            let home = Self::slot_of(self.ring[r as usize].key);
            // move r back to i if its home is not within (i, j]
            let between = if i <= j {
                home > i && home <= j
            } else {
                home > i || home <= j
            };
            if !between {
                self.table[i] = r;
                self.table[j] = EMPTY;
                i = j;
            }
        }
    }

    /// Is `key` among the remembered keys?
    pub fn contains(&self, key: u64) -> bool {
        self.find(key).is_some()
    }

    /// Insert `key` seen from `src` at `recv_ns`. The oldest key is forgotten
    /// once 65,536 are remembered.
    pub fn insert(&mut self, key: u64, src: FeedSrc, recv_ns: u64) -> Seen {
        if let Some(i) = self.find(key) {
            let e = self.ring[self.table[i] as usize];
            return Seen::Dup {
                first_src: FeedSrc::from_u8(e.src).unwrap_or(FeedSrc::Replay),
                first_recv_ns: e.recv_ns,
            };
        }
        if self.filled == DEDUP_RING {
            let old = self.ring[self.head].key;
            if let Some(i) = self.find(old) {
                if self.table[i] as usize == self.head {
                    self.remove_at(i);
                }
            }
        } else {
            self.filled += 1;
        }
        self.ring[self.head] = Entry {
            key,
            recv_ns,
            src: src as u8,
        };
        let mut i = Self::slot_of(key);
        while self.table[i] != EMPTY {
            i = (i + 1) & (TABLE - 1);
        }
        self.table[i] = self.head as u32;
        self.head = (self.head + 1) % DEDUP_RING;
        Seen::New
    }

    /// Keys currently remembered.
    pub fn len(&self) -> usize {
        self.filled
    }

    pub fn is_empty(&self) -> bool {
        self.filled == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(i: u64) -> u64 {
        xxhash_rust::xxh3::xxh3_64(&i.to_le_bytes())
    }

    #[test]
    fn first_wins_and_reports_source() {
        let mut d = SigDedup::new();
        assert_eq!(d.insert(k(1), FeedSrc::PumpPortal, 10), Seen::New);
        assert_eq!(
            d.insert(k(1), FeedSrc::LogsWs, 25),
            Seen::Dup {
                first_src: FeedSrc::PumpPortal,
                first_recv_ns: 10
            }
        );
        assert!(d.contains(k(1)));
        assert!(!d.contains(k(2)));
    }

    #[test]
    fn forgets_oldest_after_ring_wraps() {
        let mut d = SigDedup::new();
        let n = DEDUP_RING as u64;
        for i in 0..n {
            assert_eq!(d.insert(k(i), FeedSrc::LogsWs, i), Seen::New);
        }
        assert_eq!(d.len(), DEDUP_RING);
        for i in 0..n {
            assert!(d.contains(k(i)), "{i}");
        }
        // 1000 more evict the 1000 oldest, keep the rest
        for i in n..n + 1000 {
            assert_eq!(d.insert(k(i), FeedSrc::LogsWs, i), Seen::New);
        }
        for i in 0..1000 {
            assert!(!d.contains(k(i)), "{i} should be forgotten");
        }
        for i in 1000..n + 1000 {
            assert!(d.contains(k(i)), "{i} should be remembered");
        }
    }

    #[test]
    fn colliding_home_slots_survive_deletion() {
        let mut d = SigDedup::new();
        // keys sharing one home slot (equal low bits) form a probe chain
        let base = 0x1234u64;
        let keys: Vec<u64> = (1..=8).map(|i| base | (i << 40)).collect();
        for &key in &keys {
            d.insert(key, FeedSrc::Relay, 0);
        }
        // fill the ring so every further insert evicts the oldest key
        for i in 0..(DEDUP_RING - keys.len()) as u64 {
            d.insert(k(i + 1_000_000), FeedSrc::Relay, 0);
        }
        for &key in &keys {
            assert!(d.contains(key));
        }
        // evict the colliding keys one by one: the rest of the chain stays reachable
        for e in 0..keys.len() {
            d.insert(k(e as u64 + 9_000_000), FeedSrc::Relay, 0);
            for (j, &key) in keys.iter().enumerate() {
                assert_eq!(d.contains(key), j > e, "after {e} evictions, key {j}");
            }
        }
    }
}
