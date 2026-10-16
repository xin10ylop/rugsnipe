//! Thread -> hot loop transport for wire bytes: a single-producer
//! single-consumer **byte** ring (`rtrb<u8>`) of variable-length records
//! `[header 32 B][transaction][meta trailer]`. The adapter thread writes a
//! whole record or nothing ([`WireSink::push`]); the hot loop copies records
//! straight from the ring's memory into its [`FeedBuf`] arena
//! ([`WireSource::poll_into`]): lock-free and allocation-free.
//!
//! Header (little-endian): `u32 payload_len, u32 meta_len, u64 recv_ns,
//! u64 slot, u64 bank_id`.

use crate::buf::FeedBuf;
use crate::event::FeedSrc;
use crate::health::FeedHealth;
use std::sync::Arc;

/// Record header size.
pub const HDR: usize = 32;

/// Producer side (adapter thread).
#[derive(Debug)]
pub struct WireSink {
    prod: rtrb::Producer<u8>,
    rec: Vec<u8>,
    health: Arc<FeedHealth>,
}

/// Consumer side (hot loop).
#[derive(Debug)]
pub struct WireSource {
    cons: rtrb::Consumer<u8>,
    src: FeedSrc,
}

/// A connected pair over a ring of `bytes` bytes.
pub fn wire_ring(src: FeedSrc, bytes: usize, health: Arc<FeedHealth>) -> (WireSink, WireSource) {
    let (prod, cons) = rtrb::RingBuffer::new(bytes.max(1 << 14));
    (
        WireSink {
            prod,
            rec: Vec::with_capacity(16 * 1024),
            health,
        },
        WireSource { cons, src },
    )
}

impl WireSink {
    /// Write one record (`tx` then `meta`); a full ring drops it and counts
    /// it in `ring_drops`.
    pub fn push(&mut self, recv_ns: u64, slot: u64, bank_id: u64, tx: &[u8], meta: &[u8]) -> bool {
        let n = tx.len() + meta.len();
        if n > u32::MAX as usize || HDR + n > self.prod.buffer().capacity() {
            FeedHealth::inc(&self.health.ring_drops);
            return false;
        }
        self.rec.clear();
        self.rec.extend_from_slice(&(n as u32).to_le_bytes());
        self.rec
            .extend_from_slice(&(meta.len() as u32).to_le_bytes());
        self.rec.extend_from_slice(&recv_ns.to_le_bytes());
        self.rec.extend_from_slice(&slot.to_le_bytes());
        self.rec.extend_from_slice(&bank_id.to_le_bytes());
        self.rec.extend_from_slice(tx);
        self.rec.extend_from_slice(meta);
        match self.prod.push_entire_slice(&self.rec) {
            Ok(()) => true,
            Err(_) => {
                FeedHealth::inc(&self.health.ring_drops);
                false
            }
        }
    }

    /// The consumer is gone.
    pub fn is_abandoned(&self) -> bool {
        self.prod.is_abandoned()
    }
}

fn copy_out(parts: (&[u8], &[u8]), dst: &mut [u8]) {
    let (a, b) = parts;
    let k = a.len().min(dst.len());
    dst[..k].copy_from_slice(&a[..k]);
    let rest = dst.len() - k;
    dst[k..].copy_from_slice(&b[..rest]);
}

impl WireSource {
    /// Move whole records into `out` while it has room; returns how many.
    pub fn poll_into(&mut self, out: &mut FeedBuf) -> usize {
        let mut got = 0;
        loop {
            if out.is_full() || self.cons.slots() < HDR {
                return got;
            }
            let mut h = [0u8; HDR];
            match self.cons.read_chunk(HDR) {
                Ok(c) => copy_out(c.as_slices(), &mut h),
                Err(_) => return got,
            }
            let n = u32::from_le_bytes([h[0], h[1], h[2], h[3]]) as usize;
            let meta = u32::from_le_bytes([h[4], h[5], h[6], h[7]]) as usize;
            let recv = u64::from_le_bytes(h[8..16].try_into().unwrap_or([0; 8]));
            let slot = u64::from_le_bytes(h[16..24].try_into().unwrap_or([0; 8]));
            let bank = u64::from_le_bytes(h[24..32].try_into().unwrap_or([0; 8]));
            if n > out.arena_room() {
                return got; // drain the buffer first
            }
            let Ok(c) = self.cons.read_chunk(HDR + n) else {
                return got; // the producer writes whole records: cannot happen
            };
            let (a, b) = c.as_slices();
            // skip the header across the two halves
            let (pa, pb): (&[u8], &[u8]) = if a.len() >= HDR {
                (&a[HDR..], b)
            } else {
                (&[], &b[HDR - a.len()..])
            };
            let ok = out.push_wire_parts(self.src, recv, slot, bank, [pa, pb], meta);
            c.commit_all();
            if ok {
                got += 1;
            }
        }
    }

    /// The producer is gone and every record was read.
    pub fn is_finished(&self) -> bool {
        self.cons.is_abandoned() && self.cons.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Payload;

    #[test]
    fn records_cross_the_wrap_intact() {
        let h = Arc::new(FeedHealth::new(FeedSrc::Grpc, 0));
        let (mut sink, mut src) = wire_ring(FeedSrc::Grpc, 1 << 14, h.clone());
        let mut buf = FeedBuf::with_capacity(8, 1 << 16);
        let mut seen = 0u64;
        // many records of varying size wrap the 16 KiB ring several times
        for i in 0..400u64 {
            let tx: Vec<u8> = (0..(100 + (i * 37) % 900))
                .map(|k| (k as u8) ^ (i as u8))
                .collect();
            let meta = vec![i as u8; (i % 7) as usize];
            while !sink.push(i, 1000 + i, i % 3, &tx, &meta) {
                // full: drain
                buf.clear();
                src.poll_into(&mut buf);
                for e in buf.events() {
                    let Payload::Wire(w) = e.payload else {
                        panic!()
                    };
                    let j = e.recv_ns;
                    assert_eq!(e.slot, 1000 + j);
                    assert_eq!(e.bank_id, j % 3);
                    assert_eq!(e.src, FeedSrc::Grpc);
                    assert_eq!(buf.wire_meta(w), vec![j as u8; (j % 7) as usize].as_slice());
                    let want: Vec<u8> = (0..(100 + (j * 37) % 900))
                        .map(|k| (k as u8) ^ (j as u8))
                        .collect();
                    assert_eq!(buf.wire_tx(w), want.as_slice());
                    assert_eq!(j, seen);
                    seen += 1;
                }
            }
        }
        loop {
            buf.clear();
            if src.poll_into(&mut buf) == 0 {
                break;
            }
            seen += buf.len() as u64;
        }
        assert_eq!(seen, 400);
        assert!(h.ring_drops.load(std::sync::atomic::Ordering::Relaxed) > 0);
    }

    #[test]
    fn oversize_is_dropped_and_a_full_arena_waits() {
        let h = Arc::new(FeedHealth::new(FeedSrc::Grpc, 0));
        let (mut sink, mut src) = wire_ring(FeedSrc::Grpc, 1 << 14, h);
        assert!(!sink.push(0, 0, 0, &vec![0u8; 1 << 15], &[]));
        assert!(sink.push(0, 1, 0, &[7u8; 600], &[]));
        let mut small = FeedBuf::with_capacity(8, 100);
        assert_eq!(
            src.poll_into(&mut small),
            0,
            "arena too small: the record stays"
        );
        let mut big = FeedBuf::with_capacity(8, 1000);
        assert_eq!(src.poll_into(&mut big), 1);
        drop(sink);
        assert!(src.is_finished());
    }
}
