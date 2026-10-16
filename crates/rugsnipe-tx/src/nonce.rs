//! Durable-nonce pool and the no-double-buy rule (docs/06 section 7.3).
//!
//! **Rule.** A mint may only fire on nonce `xxh3(mint) % N` (no probing),
//! and only with a value whose `last_change_slot < create_slot`.
//!
//! Why it holds across boxes and restarts: every box maps a mint to the same
//! account; all variants on all boxes carry the same pre-create value, so
//! the chain executes at most one of them; any landing or burn changes the
//! value at a slot `>= create_slot`, which disqualifies every later attempt.
//! A collision between two mints costs a snipe, never a double buy.
//!
//! **State per slot**: one packed `AtomicU64` =
//! `last_change_slot (bits 0..40) | idx (40..56) | state (56..58) | burn_tries (58..62)`.
//! The 32-byte value sits in a seqlock next to it (single writer: the
//! account-stream thread; lock-free readers).
//!
//! Transitions: `Free -> InFlight` (fire, CAS), `InFlight -> Free` (value
//! changed), `InFlight -> Burning` (`since + burn_after` slots or the first
//! dump signal), `Burning -> Free` (value changed), `Burning ->
//! Quarantined` after 3 failed burns.

use pump_codec::Pubkey;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering::*};
use xxhash_rust::xxh3::xxh3_64;

const SLOT_BITS: u32 = 40;
const SLOT_MASK: u64 = (1 << SLOT_BITS) - 1;
/// `last_change_slot` sentinel: value unknown, never fire.
pub const UNKNOWN_SLOT: u64 = SLOT_MASK;
/// Failed burns before quarantine.
pub const MAX_BURN_TRIES: u8 = 3;

/// Nonce slot state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NonceState {
    Free = 0,
    InFlight = 1,
    Burning = 2,
    Quarantined = 3,
}

impl NonceState {
    fn from_bits(b: u64) -> Self {
        match b & 3 {
            0 => Self::Free,
            1 => Self::InFlight,
            2 => Self::Burning,
            _ => Self::Quarantined,
        }
    }
}

/// Decoded packed word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packed {
    pub last_change_slot: u64,
    pub idx: u16,
    pub state: NonceState,
    pub burn_tries: u8,
}

impl Packed {
    pub fn pack(&self) -> u64 {
        (self.last_change_slot & SLOT_MASK)
            | ((self.idx as u64) << 40)
            | ((self.state as u64) << 56)
            | (((self.burn_tries & 0xf) as u64) << 58)
    }
    pub fn unpack(w: u64) -> Self {
        Self {
            last_change_slot: w & SLOT_MASK,
            idx: ((w >> 40) & 0xffff) as u16,
            state: NonceState::from_bits(w >> 56),
            burn_tries: ((w >> 58) & 0xf) as u8,
        }
    }
}

/// `sha256("DURABLE_NONCE" ‖ previous_blockhash)`: the value a nonce holds
/// after an advance that landed in a slot whose parent blockhash is
/// `previous_blockhash` (solana-nonce 3.4 `DurableNonce::from_blockhash`).
pub fn next_value(previous_blockhash: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"DURABLE_NONCE");
    h.update(previous_blockhash);
    h.finalize().into()
}

/// Parse an 80-byte nonce account (`Versions::Current(State::Initialized)`):
/// returns `(authority, durable_nonce, lamports_per_signature)`.
pub fn parse_nonce_account(data: &[u8]) -> Option<(Pubkey, [u8; 32], u64)> {
    if data.len() < 80 {
        return None;
    }
    let version = u32::from_le_bytes(data[0..4].try_into().ok()?);
    let state = u32::from_le_bytes(data[4..8].try_into().ok()?);
    if version != 1 || state != 1 {
        return None;
    }
    Some((
        Pubkey::new_from_array(data[8..40].try_into().ok()?),
        data[40..72].try_into().ok()?,
        u64::from_le_bytes(data[72..80].try_into().ok()?),
    ))
}

/// One nonce account.
#[derive(Debug)]
pub struct NonceSlot {
    pub account: Pubkey,
    word: AtomicU64,
    seq: AtomicU64,
    value: [AtomicU64; 4],
    since_slot: AtomicU64,
    intent: AtomicU64,
}

/// Why a mint cannot fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonceSkip {
    /// The slot is InFlight / Burning / Quarantined.
    NotFree(NonceState),
    /// The value changed at or after the create slot (or is unknown).
    ChangedSinceCreate {
        last_change_slot: u64,
    },
    /// Another thread claimed it first.
    Raced,
    EmptyPool,
}

/// A successful claim: fire with exactly this value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Claim {
    pub idx: usize,
    pub account: Pubkey,
    pub value: [u8; 32],
    pub last_change_slot: u64,
}

/// Fixed pool of nonce accounts.
#[derive(Debug)]
pub struct NoncePool {
    pub authority: Pubkey,
    slots: Box<[NonceSlot]>,
}

impl NoncePool {
    /// All slots start Free with an unknown value (cannot fire until
    /// [`NoncePool::on_value`] reports the account's value).
    pub fn new(authority: Pubkey, accounts: &[Pubkey]) -> Self {
        let slots = accounts
            .iter()
            .enumerate()
            .map(|(i, a)| NonceSlot {
                account: *a,
                word: AtomicU64::new(
                    Packed {
                        last_change_slot: UNKNOWN_SLOT,
                        idx: i as u16,
                        state: NonceState::Free,
                        burn_tries: 0,
                    }
                    .pack(),
                ),
                seq: AtomicU64::new(0),
                value: Default::default(),
                since_slot: AtomicU64::new(0),
                intent: AtomicU64::new(0),
            })
            .collect();
        Self { authority, slots }
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
    pub fn account(&self, idx: usize) -> Pubkey {
        self.slots[idx].account
    }

    /// The only slot a mint may use.
    #[inline]
    pub fn index_for(&self, mint: &Pubkey) -> usize {
        (xxh3_64(mint.as_ref()) % self.slots.len().max(1) as u64) as usize
    }

    pub fn packed(&self, idx: usize) -> Packed {
        Packed::unpack(self.slots[idx].word.load(Acquire))
    }

    /// Seqlock read of the value.
    pub fn value(&self, idx: usize) -> [u8; 32] {
        let s = &self.slots[idx];
        loop {
            let a = s.seq.load(SeqCst);
            if a & 1 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let mut out = [0u8; 32];
            for (i, v) in s.value.iter().enumerate() {
                out[8 * i..8 * i + 8].copy_from_slice(&v.load(SeqCst).to_le_bytes());
            }
            if s.seq.load(SeqCst) == a {
                return out;
            }
        }
    }

    /// Hot path: claim the mint's nonce for a create at `create_slot`.
    ///
    /// The word and the value are read inside one seqlock window; a
    /// concurrent value update (odd or changed sequence) makes the claim
    /// fail as `Raced` and nothing is sent.
    pub fn try_claim(
        &self,
        mint: &Pubkey,
        create_slot: u64,
        intent: u64,
        now_slot: u64,
    ) -> Result<Claim, NonceSkip> {
        if self.slots.is_empty() {
            return Err(NonceSkip::EmptyPool);
        }
        let idx = self.index_for(mint);
        let s = &self.slots[idx];
        let s1 = s.seq.load(SeqCst);
        if s1 & 1 == 1 {
            return Err(NonceSkip::Raced);
        }
        let w = s.word.load(SeqCst);
        let p = Packed::unpack(w);
        if p.state != NonceState::Free {
            return Err(NonceSkip::NotFree(p.state));
        }
        if p.last_change_slot == UNKNOWN_SLOT || p.last_change_slot >= create_slot {
            return Err(NonceSkip::ChangedSinceCreate {
                last_change_slot: p.last_change_slot,
            });
        }
        let mut value = [0u8; 32];
        for (i, v) in s.value.iter().enumerate() {
            value[8 * i..8 * i + 8].copy_from_slice(&v.load(SeqCst).to_le_bytes());
        }
        let claimed = Packed {
            state: NonceState::InFlight,
            ..p
        }
        .pack();
        if s.word.compare_exchange(w, claimed, SeqCst, SeqCst).is_err() {
            return Err(NonceSkip::Raced);
        }
        if s.seq.load(SeqCst) != s1 {
            let _ = s.word.compare_exchange(claimed, w, SeqCst, SeqCst);
            return Err(NonceSkip::Raced);
        }
        s.since_slot.store(now_slot, Release);
        s.intent.store(intent, Release);
        Ok(Claim {
            idx,
            account: s.account,
            value,
            last_change_slot: p.last_change_slot,
        })
    }

    /// Give a claim back without sending (e.g. signing failed). Only valid
    /// if nothing was sent with the value.
    pub fn release_unsent(&self, c: &Claim) {
        let s = &self.slots[c.idx];
        let _ = s.word.fetch_update(AcqRel, Acquire, |w| {
            let p = Packed::unpack(w);
            (p.state == NonceState::InFlight && p.last_change_slot == c.last_change_slot).then(
                || {
                    Packed {
                        state: NonceState::Free,
                        ..p
                    }
                    .pack()
                },
            )
        });
    }

    /// Account stream: the nonce account now holds `value` (changed in
    /// `slot`). Returns the state before the update. InFlight and Burning
    /// become Free; Quarantined stays (operator action).
    pub fn on_value(&self, idx: usize, value: [u8; 32], slot: u64) -> NonceState {
        let s = &self.slots[idx];
        s.seq.fetch_add(1, SeqCst);
        let prev = s
            .word
            .fetch_update(SeqCst, SeqCst, |w| {
                let p = Packed::unpack(w);
                let state = if p.state == NonceState::Quarantined {
                    p.state
                } else {
                    NonceState::Free
                };
                Some(
                    Packed {
                        last_change_slot: slot.min(SLOT_MASK - 1),
                        state,
                        burn_tries: 0,
                        ..p
                    }
                    .pack(),
                )
            })
            .unwrap();
        for (i, v) in s.value.iter().enumerate() {
            v.store(
                u64::from_le_bytes(value[8 * i..8 * i + 8].try_into().unwrap()),
                SeqCst,
            );
        }
        s.seq.fetch_add(1, SeqCst);
        Packed::unpack(prev).state
    }

    /// InFlight slots fired at least `burn_after` slots ago.
    pub fn due_for_burn(&self, now_slot: u64, burn_after: u64) -> Vec<usize> {
        (0..self.slots.len())
            .filter(|&i| {
                self.packed(i).state == NonceState::InFlight
                    && self.slots[i].since_slot.load(Acquire) + burn_after <= now_slot
            })
            .collect()
    }

    /// InFlight -> Burning (dump signal or timeout). `true` if it moved.
    pub fn start_burn(&self, idx: usize) -> bool {
        self.slots[idx]
            .word
            .fetch_update(AcqRel, Acquire, |w| {
                let p = Packed::unpack(w);
                (p.state == NonceState::InFlight).then(|| {
                    Packed {
                        state: NonceState::Burning,
                        ..p
                    }
                    .pack()
                })
            })
            .is_ok()
    }

    /// A burn transaction failed; quarantine after [`MAX_BURN_TRIES`].
    /// Returns the state afterwards.
    pub fn burn_failed(&self, idx: usize) -> NonceState {
        let _ = self.slots[idx].word.fetch_update(AcqRel, Acquire, |w| {
            let p = Packed::unpack(w);
            if p.state != NonceState::Burning {
                return None;
            }
            let tries = p.burn_tries + 1;
            let state = if tries >= MAX_BURN_TRIES {
                NonceState::Quarantined
            } else {
                NonceState::Burning
            };
            Some(
                Packed {
                    burn_tries: tries,
                    state,
                    ..p
                }
                .pack(),
            )
        });
        self.packed(idx).state
    }

    /// Never fire on this slot (account missing, not initialized, or held by
    /// another authority at seeding); only [`NoncePool::unquarantine`] clears it.
    pub fn quarantine(&self, idx: usize) {
        let _ = self.slots[idx].word.fetch_update(AcqRel, Acquire, |w| {
            Some(
                Packed {
                    state: NonceState::Quarantined,
                    ..Packed::unpack(w)
                }
                .pack(),
            )
        });
    }

    /// Recovery: a slot that was in flight when the process stopped (its
    /// value has not changed since): the old variants may still land, so it
    /// is InFlight again from `now_slot` and the keeper burns it.
    pub fn resume_inflight(&self, idx: usize, intent: u64, now_slot: u64) {
        let s = &self.slots[idx];
        let moved = s
            .word
            .fetch_update(AcqRel, Acquire, |w| {
                let p = Packed::unpack(w);
                (p.state == NonceState::Free).then(|| {
                    Packed {
                        state: NonceState::InFlight,
                        ..p
                    }
                    .pack()
                })
            })
            .is_ok();
        if moved {
            s.since_slot.store(now_slot, Release);
            s.intent.store(intent, Release);
        }
    }

    /// Slot at which `idx` was claimed (InFlight) or 0.
    pub fn since_slot(&self, idx: usize) -> u64 {
        self.slots[idx].since_slot.load(Acquire)
    }

    /// Operator clears a quarantine (after inspecting the account).
    pub fn unquarantine(&self, idx: usize) {
        let _ = self.slots[idx].word.fetch_update(AcqRel, Acquire, |w| {
            let p = Packed::unpack(w);
            (p.state == NonceState::Quarantined).then(|| {
                Packed {
                    state: NonceState::Free,
                    burn_tries: 0,
                    ..p
                }
                .pack()
            })
        });
    }

    /// Intent that holds slot `idx` (0 if none).
    pub fn intent(&self, idx: usize) -> u64 {
        self.slots[idx].intent.load(Acquire)
    }

    /// Number of Free slots with a known value.
    pub fn free_count(&self) -> usize {
        (0..self.slots.len())
            .filter(|&i| {
                let p = self.packed(i);
                p.state == NonceState::Free && p.last_change_slot != UNKNOWN_SLOT
            })
            .count()
    }
}
