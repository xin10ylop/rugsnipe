//! Alpenglow `BlockComponent` parser for completed data sets (shreds).
//!
//! From `solana-entry 4.3.1/src/block_component.rs` (docs/research/speed-gap-2.md
//! sections 1 and 4). Every completed data set is one component:
//!
//! | shape | bytes |
//! |---|---|
//! | entry batch | `u64 count (!= 0)` + `count` x bincode `Entry` (the pre-Alpenglow `Vec<Entry>`) |
//! | marker | `u64 0` + `u16 version (=1)` + `u8 variant` + `u16 len` + `len` bytes |
//! | abort | exactly `u64 0` (8 bytes): the leader abandoned the slot |
//!
//! Marker variants (`BlockMarkerV1`, u8 tag in declaration order): `0`
//! BlockFooter, `1` BlockHeader `{parent_slot u64, parent_block_id [32]}`,
//! `2` UpdateParent `{new_parent_slot u64, new_parent_block_id [32]}`, `3`
//! GenesisCertificate. The `len` bytes start with the inner versioned enum's
//! u8 tag (`1` = V1). A header is
//! `00x8 | 0100 | 01 | 2900 | 01 | parent_slot | block_id` (54 bytes).
//!
//! `Entry` = `num_hashes u64 || hash [32] || u64 tx_count || tx*` where each
//! transaction is serialized exactly as on the wire (legacy, v0 or v1), so
//! each one is sliced with [`TxView::parse_prefix`] and handed out as raw
//! bytes. This is a minimal own parser: `solana-entry` would pin Agave
//! crates into the hot path. Off-the-shelf shred decoders that call
//! `deserialize_exact::<Vec<Entry>>` drop every marker and read the abort
//! batch as "no entries"; this parser reports both.

use crate::error::{DecodeError, Result};
use crate::tx::TxView;

/// Bytes of an `Entry` before its transactions: `num_hashes`, `hash`, tx count.
const ENTRY_HEADER: usize = 8 + 32 + 8;
/// Smallest possible serialized transaction (compact 1 sig + header + 1 key + hash + 0 ix).
const MIN_TX_LEN: usize = 1 + 64 + 3 + 1 + 32 + 32 + 1;

/// A parsed completed data set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockComponent<'a> {
    /// One or more entries (transactions and ticks).
    EntryBatch(EntryBatch<'a>),
    /// A block marker (header, footer, update-parent, genesis certificate).
    Marker(BlockMarker<'a>),
    /// The 8-byte all-zero batch: the leader aborted this slot (bank).
    Abort,
}

/// Pre-validated entry batch (every entry and transaction bound-checked and
/// sliced once by [`parse_block_component`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryBatch<'a> {
    num_entries: u64,
    num_transactions: usize,
    /// Bytes after the u64 entry count.
    body: &'a [u8],
}

impl<'a> EntryBatch<'a> {
    pub fn num_entries(&self) -> u64 {
        self.num_entries
    }
    /// Transactions across all entries.
    pub fn num_transactions(&self) -> usize {
        self.num_transactions
    }
    /// Iterate entries in order.
    pub fn entries(&self) -> EntryIter<'a> {
        EntryIter {
            body: self.body,
            pos: 0,
            remaining: self.num_entries,
        }
    }
    /// Every transaction's raw wire bytes, in ledger order.
    pub fn transactions(&self) -> impl Iterator<Item = &'a [u8]> + 'a {
        self.entries().flat_map(|e| e.transactions())
    }
}

/// One entry (a tick when it holds no transactions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryView<'a> {
    pub num_hashes: u64,
    pub hash: &'a [u8; 32],
    num_txs: u64,
    txs: &'a [u8],
}

impl<'a> EntryView<'a> {
    pub fn num_transactions(&self) -> u64 {
        self.num_txs
    }
    pub fn is_tick(&self) -> bool {
        self.num_txs == 0
    }
    /// Raw wire bytes of each transaction (parse with [`TxView::parse`]).
    pub fn transactions(&self) -> TxSliceIter<'a> {
        TxSliceIter {
            buf: self.txs,
            remaining: self.num_txs,
        }
    }
}

/// Iterator over entries of a validated batch.
#[derive(Debug, Clone)]
pub struct EntryIter<'a> {
    body: &'a [u8],
    pos: usize,
    remaining: u64,
}

impl<'a> Iterator for EntryIter<'a> {
    type Item = EntryView<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        // Validated by `parse_block_component`.
        let (entry, len) = read_entry(self.body, self.pos).ok()?;
        self.pos += len;
        Some(entry)
    }
}

/// Iterator over the transactions of one entry.
#[derive(Debug, Clone)]
pub struct TxSliceIter<'a> {
    buf: &'a [u8],
    remaining: u64,
}

impl<'a> Iterator for TxSliceIter<'a> {
    type Item = &'a [u8];
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        let (_, len) = TxView::parse_prefix(self.buf).ok()?;
        let (tx, rest) = self.buf.split_at(len);
        self.buf = rest;
        Some(tx)
    }
}

/// `BlockHeader` / `UpdateParent` payload (same shape).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParentRef {
    pub slot: u64,
    pub block_id: [u8; 32],
}

/// `BlockFooterV1`. The optional certificates are left raw (BLS aggregates).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockFooter<'a> {
    pub bank_hash: [u8; 32],
    pub block_producer_time_nanos: u64,
    /// At most 255 bytes (e.g. `agave/4.3.1`).
    pub user_agent: &'a [u8],
    /// `block_final_cert`, `skip_reward_cert`, `notar_reward_cert` (borsh-like
    /// `Option`s; `[0, 0, 0]` when all are absent).
    pub certificates_raw: &'a [u8],
}

/// A block marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockMarker<'a> {
    Footer(BlockFooter<'a>),
    /// First component of every Alpenglow block.
    Header(ParentRef),
    /// Fast-leader-handover reparenting: invalidates every earlier component
    /// of this slot's bank (not active before FLH, Agave >= 4.4).
    UpdateParent(ParentRef),
    /// One-time migration certificate (raw payload after the V1 fields' start).
    GenesisCertificate(&'a [u8]),
    /// A marker version or variant this parser does not know (reported, not dropped).
    Unknown {
        version: u16,
        variant: u8,
        payload: &'a [u8],
    },
}

#[inline]
fn u64_at(b: &[u8], o: usize) -> Result<u64> {
    b.get(o..o + 8)
        .map(|s| u64::from_le_bytes(s.try_into().unwrap()))
        .ok_or(DecodeError::UnexpectedEof {
            offset: o,
            needed: (o + 8).saturating_sub(b.len()),
        })
}

/// Parse one entry at `pos` of `body`; returns it and its byte length.
fn read_entry(body: &[u8], pos: usize) -> Result<(EntryView<'_>, usize)> {
    let num_hashes = u64_at(body, pos)?;
    let hash: &[u8; 32] = body
        .get(pos + 8..pos + 40)
        .ok_or(DecodeError::UnexpectedEof {
            offset: pos + 8,
            needed: 32,
        })?
        .try_into()
        .unwrap();
    let num_txs = u64_at(body, pos + 40)?;
    let start = pos + ENTRY_HEADER;
    let rest = &body[start..];
    if (num_txs as u128) * (MIN_TX_LEN as u128) > rest.len() as u128 {
        return Err(DecodeError::InvalidLength {
            offset: pos + 40,
            value: num_txs.min(usize::MAX as u64) as usize,
        });
    }
    let mut o = 0usize;
    for _ in 0..num_txs {
        let (_, len) = TxView::parse_prefix(&rest[o..])?;
        o += len;
    }
    Ok((
        EntryView {
            num_hashes,
            hash,
            num_txs,
            txs: &rest[..o],
        },
        ENTRY_HEADER + o,
    ))
}

/// Parse one completed data set. Strict: trailing bytes are an error.
pub fn parse_block_component(data: &[u8]) -> Result<BlockComponent<'_>> {
    let count = u64_at(data, 0)?;
    if count != 0 {
        let body = &data[8..];
        if (count as u128) * (ENTRY_HEADER as u128) > body.len() as u128 {
            return Err(DecodeError::InvalidLength {
                offset: 0,
                value: count.min(usize::MAX as u64) as usize,
            });
        }
        let (mut pos, mut ntx) = (0usize, 0usize);
        for _ in 0..count {
            let (e, len) = read_entry(body, pos)?;
            ntx += e.num_txs as usize;
            pos += len;
        }
        if pos != body.len() {
            return Err(DecodeError::InvalidTransaction(
                "trailing bytes after entry batch",
            ));
        }
        return Ok(BlockComponent::EntryBatch(EntryBatch {
            num_entries: count,
            num_transactions: ntx,
            body,
        }));
    }
    if data.len() == 8 {
        return Ok(BlockComponent::Abort);
    }
    // Marker: u16 version, u8 variant, u16 len, payload.
    let head = data.get(8..13).ok_or(DecodeError::UnexpectedEof {
        offset: 8,
        needed: 13usize.saturating_sub(data.len()),
    })?;
    let version = u16::from_le_bytes([head[0], head[1]]);
    let variant = head[2];
    let len = u16::from_le_bytes([head[3], head[4]]) as usize;
    let payload = &data[13..];
    if payload.len() != len {
        return Err(DecodeError::InvalidLength {
            offset: 11,
            value: len,
        });
    }
    let unknown = BlockComponent::Marker(BlockMarker::Unknown {
        version,
        variant,
        payload,
    });
    if version != 1 {
        return Ok(unknown);
    }
    let parent = |p: &[u8]| -> Result<Option<ParentRef>> {
        // inner tag 1 (V1) + u64 + [32]
        if p.first() != Some(&1) {
            return Ok(None);
        }
        if p.len() != 41 {
            return Err(DecodeError::InvalidLength {
                offset: 13,
                value: p.len(),
            });
        }
        Ok(Some(ParentRef {
            slot: u64::from_le_bytes(p[1..9].try_into().unwrap()),
            block_id: p[9..41].try_into().unwrap(),
        }))
    };
    let marker = match variant {
        0 => {
            // VersionedBlockFooter tag 1, bank_hash, time, u8 ua_len, ua, certs
            if payload.first() != Some(&1) {
                return Ok(unknown);
            }
            let need = |n: usize| {
                if payload.len() < n {
                    Err(DecodeError::UnexpectedEof {
                        offset: 13 + payload.len(),
                        needed: n - payload.len(),
                    })
                } else {
                    Ok(())
                }
            };
            need(1 + 32 + 8 + 1)?;
            let ua_len = payload[41] as usize;
            need(42 + ua_len)?;
            BlockMarker::Footer(BlockFooter {
                bank_hash: payload[1..33].try_into().unwrap(),
                block_producer_time_nanos: u64::from_le_bytes(payload[33..41].try_into().unwrap()),
                user_agent: &payload[42..42 + ua_len],
                certificates_raw: &payload[42 + ua_len..],
            })
        }
        1 => match parent(payload)? {
            Some(p) => BlockMarker::Header(p),
            None => return Ok(unknown),
        },
        2 => match parent(payload)? {
            Some(p) => BlockMarker::UpdateParent(p),
            None => return Ok(unknown),
        },
        3 => BlockMarker::GenesisCertificate(payload),
        _ => return Ok(unknown),
    };
    Ok(BlockComponent::Marker(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// Byte vectors serialized by `solana-entry 4.3.1` + `wincode 0.6.1`
    /// (scratch crate `mtA`, docs/research/speed-gap-2.md section 4) with
    /// `Hash::new_unique()` ids.
    const SE_HEADER: &str = "00000000000000000100012900017b000000000000000100000000000000000000000000000000000000000000000000000000000000";
    const SE_UPDATE_PARENT: &str = "000000000000000001000229000178000000000000000200000000000000000000000000000000000000000000000000000000000000";
    const SE_FOOTER: &str = "0000000000000000010000380001030000000000000000000000000000000000000000000000000000000000000001000000000000000b61676176652f342e332e31000000";

    #[test]
    fn abort_batch() {
        assert_eq!(
            parse_block_component(&[0u8; 8]).unwrap(),
            BlockComponent::Abort
        );
        // 7 bytes is not a component
        assert!(parse_block_component(&[0u8; 7]).is_err());
    }

    #[test]
    fn header_vector_from_solana_entry() {
        let b = unhex(SE_HEADER);
        assert_eq!(b.len(), 54);
        // 00x8 | 0100 | 01 | 2900 | 01 | parent_slot | block_id
        assert_eq!(&b[8..14], &[0x01, 0x00, 0x01, 0x29, 0x00, 0x01]);
        match parse_block_component(&b).unwrap() {
            BlockComponent::Marker(BlockMarker::Header(p)) => {
                assert_eq!(p.slot, 123);
                let mut want = [0u8; 32];
                want[0] = 1; // Hash::new_unique() #1
                assert_eq!(p.block_id, want);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn update_parent_vector_from_solana_entry() {
        match parse_block_component(&unhex(SE_UPDATE_PARENT)).unwrap() {
            BlockComponent::Marker(BlockMarker::UpdateParent(p)) => {
                assert_eq!(p.slot, 120);
                assert_eq!(p.block_id[0], 2);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn footer_vector_from_solana_entry() {
        match parse_block_component(&unhex(SE_FOOTER)).unwrap() {
            BlockComponent::Marker(BlockMarker::Footer(f)) => {
                assert_eq!(f.bank_hash[0], 3);
                assert_eq!(f.block_producer_time_nanos, 1);
                assert_eq!(f.user_agent, b"agave/4.3.1");
                assert_eq!(f.certificates_raw, &[0, 0, 0]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn hand_built_header_and_bad_lengths() {
        let mut b = vec![0u8; 8];
        b.extend_from_slice(&[0x01, 0x00, 0x01, 0x29, 0x00, 0x01]);
        b.extend_from_slice(&456_789_012u64.to_le_bytes());
        b.extend_from_slice(&[0xab; 32]);
        assert_eq!(
            parse_block_component(&b).unwrap(),
            BlockComponent::Marker(BlockMarker::Header(ParentRef {
                slot: 456_789_012,
                block_id: [0xab; 32]
            }))
        );
        // truncated payload, trailing byte, unknown version / variant
        assert!(parse_block_component(&b[..b.len() - 1]).is_err());
        let mut t = b.clone();
        t.push(0);
        assert!(parse_block_component(&t).is_err());
        let mut v2 = b.clone();
        v2[8] = 2;
        assert!(matches!(
            parse_block_component(&v2).unwrap(),
            BlockComponent::Marker(BlockMarker::Unknown { version: 2, .. })
        ));
        let mut var9 = b.clone();
        var9[10] = 9;
        assert!(matches!(
            parse_block_component(&var9).unwrap(),
            BlockComponent::Marker(BlockMarker::Unknown { variant: 9, .. })
        ));
        // marker head cut short
        assert!(parse_block_component(&b[..10]).is_err());
    }

    #[test]
    fn huge_counts_do_not_allocate_or_panic() {
        let mut b = u64::MAX.to_le_bytes().to_vec();
        b.extend_from_slice(&[0u8; 48]);
        assert!(parse_block_component(&b).is_err());
        // one entry claiming 2^40 transactions
        let mut b = 1u64.to_le_bytes().to_vec();
        b.extend_from_slice(&0u64.to_le_bytes());
        b.extend_from_slice(&[0u8; 32]);
        b.extend_from_slice(&(1u64 << 40).to_le_bytes());
        assert!(parse_block_component(&b).is_err());
    }

    #[test]
    fn tick_only_batch() {
        let mut b = 2u64.to_le_bytes().to_vec();
        for i in 0..2u8 {
            b.extend_from_slice(&62_500u64.to_le_bytes());
            b.extend_from_slice(&[i; 32]);
            b.extend_from_slice(&0u64.to_le_bytes());
        }
        match parse_block_component(&b).unwrap() {
            BlockComponent::EntryBatch(eb) => {
                assert_eq!(eb.num_entries(), 2);
                assert_eq!(eb.num_transactions(), 0);
                let e: Vec<_> = eb.entries().collect();
                assert!(e.iter().all(|e| e.is_tick() && e.num_hashes == 62_500));
                assert_eq!(e[1].hash, &[1u8; 32]);
            }
            other => panic!("{other:?}"),
        }
    }
}
