//! [`FiredSet`]: first-wins claim per (mint, class), shared across threads with
//! atomics. The feed core asks it before acting on a create, so a create seen
//! on several feeds (or by several loops) fires once, and the loser learns
//! which feed won (feed lead "for free", docs/06 section 10).

use crate::event::FeedSrc;
use pump_codec::Pubkey;
use std::sync::atomic::{AtomicU64, Ordering};

const PROBE: usize = 16;
const TAG_MASK: u64 = 0xF;

/// Result of [`FiredSet::first`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fired {
    /// This caller won the claim.
    First,
    /// Already claimed, first by `first_src`.
    Already { first_src: FeedSrc },
}

/// Lock-free table of claimed `(mint, class)` keys. Fixed memory
/// (`8 * capacity` bytes). When every slot of a probe window is taken by other
/// keys, the oldest-looking slot is overwritten (lossy only after ~capacity
/// claims, far beyond a day of creates at the default size).
#[derive(Debug)]
pub struct FiredSet {
    slots: Box<[AtomicU64]>,
    mask: usize,
}

impl FiredSet {
    /// `capacity` is rounded up to a power of two (>= 1024).
    pub fn with_capacity(capacity: usize) -> FiredSet {
        let n = capacity.max(1024).next_power_of_two();
        let slots: Vec<AtomicU64> = (0..n).map(|_| AtomicU64::new(0)).collect();
        FiredSet {
            slots: slots.into_boxed_slice(),
            mask: n - 1,
        }
    }

    #[inline]
    fn key(mint: &Pubkey, class: u8) -> u64 {
        let mut b = [0u8; 33];
        b[..32].copy_from_slice(mint.as_ref());
        b[32] = class;
        let h = xxhash_rust::xxh3::xxh3_64(&b) & !TAG_MASK;
        if h == 0 {
            1 << 4
        } else {
            h
        }
    }

    /// Claim `(mint, class)` for `src`. Exactly one caller gets [`Fired::First`].
    pub fn first(&self, mint: &Pubkey, class: u8, src: FeedSrc) -> Fired {
        let h = Self::key(mint, class);
        let tagged = h | (src as u64 + 1);
        let start = (h >> 4) as usize;
        for i in 0..PROBE {
            let slot = &self.slots[(start + i) & self.mask];
            let mut cur = slot.load(Ordering::Acquire);
            if cur == 0 {
                match slot.compare_exchange(0, tagged, Ordering::AcqRel, Ordering::Acquire) {
                    Ok(_) => return Fired::First,
                    Err(now) => cur = now,
                }
            }
            if cur & !TAG_MASK == h {
                let tag = (cur & TAG_MASK) as u8;
                return Fired::Already {
                    first_src: FeedSrc::from_u8(tag.saturating_sub(1)).unwrap_or(FeedSrc::Replay),
                };
            }
        }
        // window full of other keys: overwrite the home slot
        self.slots[start & self.mask].store(tagged, Ordering::Release);
        Fired::First
    }

    /// Has `(mint, class)` been claimed?
    pub fn is_fired(&self, mint: &Pubkey, class: u8) -> bool {
        let h = Self::key(mint, class);
        let start = (h >> 4) as usize;
        (0..PROBE)
            .any(|i| self.slots[(start + i) & self.mask].load(Ordering::Acquire) & !TAG_MASK == h)
    }

    /// Forget every claim.
    pub fn clear(&self) {
        for s in self.slots.iter() {
            s.store(0, Ordering::Release);
        }
    }
}

impl Default for FiredSet {
    /// 262,144 slots (2 MiB).
    fn default() -> Self {
        FiredSet::with_capacity(1 << 18)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn first_wins_per_mint_and_class() {
        let f = FiredSet::default();
        let m = Pubkey::new_unique();
        assert_eq!(f.first(&m, 0, FeedSrc::PumpPortal), Fired::First);
        assert_eq!(
            f.first(&m, 0, FeedSrc::LogsWs),
            Fired::Already {
                first_src: FeedSrc::PumpPortal
            }
        );
        assert_eq!(f.first(&m, 1, FeedSrc::LogsWs), Fired::First, "other class");
        assert!(f.is_fired(&m, 0) && f.is_fired(&m, 1) && !f.is_fired(&m, 2));
        f.clear();
        assert!(!f.is_fired(&m, 0));
    }

    #[test]
    fn exactly_one_winner_across_threads() {
        let f = Arc::new(FiredSet::with_capacity(4096));
        let mints: Vec<Pubkey> = (0..500).map(|_| Pubkey::new_unique()).collect();
        let mints = Arc::new(mints);
        let handles: Vec<_> = (0..4)
            .map(|t| {
                let (f, mints) = (f.clone(), mints.clone());
                std::thread::spawn(move || {
                    let src = FeedSrc::from_u8(t).unwrap();
                    mints
                        .iter()
                        .filter(|m| f.first(m, 0, src) == Fired::First)
                        .count()
                })
            })
            .collect();
        let wins: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();
        assert_eq!(wins, 500);
    }
}
