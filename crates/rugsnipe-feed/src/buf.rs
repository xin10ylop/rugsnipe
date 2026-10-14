//! [`FeedBuf`]: the fixed-capacity output buffer of [`crate::Feed::poll`].

use crate::event::{FeedEvent, FeedSrc, Payload, WireRef};

/// Events plus an arena for wire bytes. Capacities are fixed at construction;
/// pushing never reallocates (a full buffer refuses the push), so a poll loop
/// that reuses one `FeedBuf` is allocation-free after warm-up.
#[derive(Debug)]
pub struct FeedBuf {
    events: Vec<FeedEvent>,
    arena: Vec<u8>,
    cap_events: usize,
    cap_arena: usize,
}

impl FeedBuf {
    /// `events` slots and `arena_bytes` of wire storage.
    pub fn with_capacity(events: usize, arena_bytes: usize) -> FeedBuf {
        FeedBuf {
            events: Vec::with_capacity(events),
            arena: Vec::with_capacity(arena_bytes),
            cap_events: events,
            cap_arena: arena_bytes,
        }
    }

    /// Forget the events (and invalidate their [`WireRef`]s).
    #[inline]
    pub fn clear(&mut self) {
        self.events.clear();
        self.arena.clear();
    }

    #[inline]
    pub fn events(&self) -> &[FeedEvent] {
        &self.events
    }

    #[inline]
    pub fn events_mut(&mut self) -> &mut [FeedEvent] {
        &mut self.events
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Free event slots.
    #[inline]
    pub fn room(&self) -> usize {
        self.cap_events - self.events.len()
    }

    /// Free arena bytes.
    #[inline]
    pub fn arena_room(&self) -> usize {
        self.cap_arena - self.arena.len()
    }

    #[inline]
    pub fn is_full(&self) -> bool {
        self.room() == 0
    }

    /// Append an event with a non-wire payload. `false` if full.
    #[inline]
    pub fn push(&mut self, ev: FeedEvent) -> bool {
        if self.is_full() {
            return false;
        }
        debug_assert!(!matches!(ev.payload, Payload::Wire(_)));
        self.events.push(ev);
        true
    }

    /// Copy `bytes` into the arena and append a [`Payload::Wire`] event.
    /// `false` (nothing written) if events or arena are full.
    pub fn push_wire(
        &mut self,
        src: FeedSrc,
        recv_ns: u64,
        slot: u64,
        bank_id: u64,
        bytes: &[u8],
    ) -> bool {
        if self.is_full() || bytes.len() > self.arena_room() || bytes.len() > u32::MAX as usize {
            return false;
        }
        let off = self.arena.len() as u32;
        self.arena.extend_from_slice(bytes);
        self.events.push(FeedEvent {
            src,
            recv_ns,
            slot,
            bank_id,
            payload: Payload::Wire(WireRef {
                off,
                len: bytes.len() as u32,
            }),
        });
        true
    }

    /// Bytes of a wire payload of this buffer.
    #[inline]
    pub fn wire(&self, w: WireRef) -> &[u8] {
        let (o, l) = (w.off as usize, w.len as usize);
        self.arena.get(o..o + l).unwrap_or(&[])
    }

    /// Keep only the events for which `keep` returns true (wire refs stay valid).
    pub fn retain(&mut self, mut keep: impl FnMut(&FeedEvent) -> bool) {
        self.events.retain(|e| keep(e));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{CreateHitLite, Payload};

    #[test]
    fn capacity_is_fixed() {
        let mut b = FeedBuf::with_capacity(2, 8);
        let ev = FeedEvent {
            src: FeedSrc::Replay,
            recv_ns: 1,
            slot: 2,
            bank_id: 0,
            payload: Payload::Hit(CreateHitLite::default()),
        };
        assert!(b.push(ev));
        assert!(
            !b.push_wire(FeedSrc::Replay, 1, 2, 0, &[0; 9]),
            "arena full"
        );
        assert!(b.push_wire(FeedSrc::Replay, 1, 2, 0, &[5; 8]));
        assert!(!b.push(ev), "events full");
        let Payload::Wire(w) = b.events()[1].payload else {
            panic!()
        };
        assert_eq!(b.wire(w), &[5; 8]);
        let (cap_e, cap_a) = (b.events.capacity(), b.arena.capacity());
        b.clear();
        assert!(b.is_empty());
        assert_eq!((b.events.capacity(), b.arena.capacity()), (cap_e, cap_a));
    }
}
