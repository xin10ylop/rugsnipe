//! The post-execution meta trailer of a [`crate::Payload::Wire`] event.
//!
//! A Yellowstone transaction update carries what the raw bytes do not:
//! its index in the block, whether it failed, `meta.fee`, the addresses its
//! lookup tables loaded and its inner (CPI) instructions. The gRPC adapter
//! thread appends them to the transaction bytes in this compact layout (all
//! integers little-endian) so `pump_codec::create_hit` can resolve v0
//! lookups and launcher CPIs **on the hot thread without allocating**:
//!
//! ```text
//! u32  tx_index            (u32::MAX = unknown)
//! u8   flags               (1 failed, 2 inner instructions present, 4 inner list truncated)
//! u64  fee                 (meta.fee)
//! u8   n_writable, u8 n_readonly, then (n_writable + n_readonly) x 32-byte keys
//! u16  n_inner, then per inner instruction:
//!      u8 outer_index, u8 program_id_index, u8 n_accounts, [n_accounts] u8, u16 data_len, [data_len] u8
//! ```
//!
//! [`encode`] runs on the adapter thread (it may allocate); [`WireMeta::parse`]
//! and its accessors only borrow.

use pump_codec::events::InnerInstruction;
use pump_codec::Pubkey;

/// `flags` bits.
pub mod flags {
    /// `meta.err` was set.
    pub const FAILED: u8 = 1;
    /// `meta.inner_instructions` was present (possibly empty).
    pub const HAS_INNER: u8 = 2;
    /// More inner instructions than [`super::MAX_INNER`] (or than a field
    /// can hold): the list is cut.
    pub const INNER_TRUNCATED: u8 = 4;
}

/// Inner instructions kept per transaction (the rest are cut).
pub const MAX_INNER: usize = 128;
/// Loaded addresses kept per transaction (a transaction indexes at most 256 accounts).
pub const MAX_LOADED: usize = 256;

const HEAD: usize = 4 + 1 + 8 + 2;

/// One inner instruction to encode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InnerOwned {
    pub outer_index: u8,
    pub program_id_index: u8,
    pub accounts: Vec<u8>,
    pub data: Vec<u8>,
}

/// What [`encode`] writes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetaIn {
    pub tx_index: Option<u32>,
    pub failed: bool,
    pub fee: u64,
    pub loaded_writable: Vec<[u8; 32]>,
    pub loaded_readonly: Vec<[u8; 32]>,
    /// `None`: the update carried no inner instructions.
    pub inner: Option<Vec<InnerOwned>>,
}

/// Append the trailer for `m` to `out`; returns its length.
pub fn encode(m: &MetaIn, out: &mut Vec<u8>) -> usize {
    let start = out.len();
    out.extend_from_slice(&m.tx_index.unwrap_or(u32::MAX).to_le_bytes());
    let mut fl = 0u8;
    if m.failed {
        fl |= flags::FAILED;
    }
    if m.inner.is_some() {
        fl |= flags::HAS_INNER;
    }
    let flag_at = out.len();
    out.push(fl);
    out.extend_from_slice(&m.fee.to_le_bytes());
    let nw = m.loaded_writable.len().min(255);
    let nr = m.loaded_readonly.len().min(255 - nw);
    out.push(nw as u8);
    out.push(nr as u8);
    for k in m.loaded_writable[..nw]
        .iter()
        .chain(&m.loaded_readonly[..nr])
    {
        out.extend_from_slice(k);
    }
    let inner = m.inner.as_deref().unwrap_or(&[]);
    let mut kept = 0usize;
    let n_at = out.len();
    out.extend_from_slice(&0u16.to_le_bytes());
    let mut truncated = inner.len() > MAX_INNER;
    for ix in inner.iter().take(MAX_INNER) {
        if ix.accounts.len() > 255 || ix.data.len() > u16::MAX as usize {
            truncated = true;
            break;
        }
        out.push(ix.outer_index);
        out.push(ix.program_id_index);
        out.push(ix.accounts.len() as u8);
        out.extend_from_slice(&ix.accounts);
        out.extend_from_slice(&(ix.data.len() as u16).to_le_bytes());
        out.extend_from_slice(&ix.data);
        kept += 1;
    }
    out[n_at..n_at + 2].copy_from_slice(&(kept as u16).to_le_bytes());
    if truncated {
        out[flag_at] |= flags::INNER_TRUNCATED;
    }
    out.len() - start
}

/// A parsed trailer (borrows the arena).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WireMeta<'a> {
    pub tx_index: Option<u32>,
    pub flags: u8,
    pub fee: u64,
    n_writable: u8,
    n_readonly: u8,
    keys: &'a [u8],
    n_inner: u16,
    inner: &'a [u8],
}

impl<'a> WireMeta<'a> {
    /// Parse a trailer; `None` if it is empty or malformed.
    pub fn parse(b: &'a [u8]) -> Option<WireMeta<'a>> {
        if b.len() < HEAD {
            return None;
        }
        let tx_index = u32::from_le_bytes(b[0..4].try_into().ok()?);
        let fl = b[4];
        let fee = u64::from_le_bytes(b[5..13].try_into().ok()?);
        let (nw, nr) = (b[13], b[14]);
        let kl = 32 * (nw as usize + nr as usize);
        let keys = b.get(HEAD..HEAD + kl)?;
        let rest = &b[HEAD + kl..];
        let n_inner = u16::from_le_bytes(rest.get(..2)?.try_into().ok()?);
        let inner = &rest[2..];
        // validate the inner records once, so the accessors cannot fail
        let mut p = 0usize;
        for _ in 0..n_inner {
            let na = *inner.get(p + 2)? as usize;
            let dl_at = p + 3 + na;
            let dl = u16::from_le_bytes(inner.get(dl_at..dl_at + 2)?.try_into().ok()?) as usize;
            p = dl_at + 2 + dl;
            if p > inner.len() {
                return None;
            }
        }
        Some(WireMeta {
            tx_index: (tx_index != u32::MAX).then_some(tx_index),
            flags: fl,
            fee,
            n_writable: nw,
            n_readonly: nr,
            keys,
            n_inner,
            inner: &inner[..p],
        })
    }

    pub fn failed(&self) -> bool {
        self.flags & flags::FAILED != 0
    }

    /// The update carried inner instructions (possibly none).
    pub fn has_inner(&self) -> bool {
        self.flags & flags::HAS_INNER != 0
    }

    pub fn inner_truncated(&self) -> bool {
        self.flags & flags::INNER_TRUNCATED != 0
    }

    /// Number of loaded addresses (writable then readonly).
    pub fn n_loaded(&self) -> usize {
        self.n_writable as usize + self.n_readonly as usize
    }

    pub fn n_inner(&self) -> usize {
        self.n_inner as usize
    }

    /// Copy the loaded addresses (writable then readonly, the order
    /// `TxView::account_key` expects) into `out`; returns how many.
    pub fn loaded_into(&self, out: &mut [Pubkey]) -> usize {
        let n = self.n_loaded().min(out.len());
        for (i, o) in out.iter_mut().enumerate().take(n) {
            let mut a = [0u8; 32];
            a.copy_from_slice(&self.keys[32 * i..32 * i + 32]);
            *o = Pubkey::new_from_array(a);
        }
        n
    }

    /// Fill `out` with the inner instructions (borrowing the trailer);
    /// returns how many (at most `out.len()`).
    pub fn inner_into(&self, out: &mut [InnerInstruction<'a>]) -> usize {
        let mut p = 0usize;
        let mut k = 0usize;
        let b = self.inner;
        while k < self.n_inner as usize && k < out.len() {
            let (outer, prog, na) = (b[p], b[p + 1], b[p + 2] as usize);
            let accounts = &b[p + 3..p + 3 + na];
            let dl_at = p + 3 + na;
            let dl = u16::from_le_bytes([b[dl_at], b[dl_at + 1]]) as usize;
            let data = &b[dl_at + 2..dl_at + 2 + dl];
            out[k] = InnerInstruction {
                outer_index: outer,
                program_id_index: prog,
                accounts,
                data,
            };
            p = dl_at + 2 + dl;
            k += 1;
        }
        k
    }
}

/// An empty inner instruction (array initializer).
pub const EMPTY_INNER: InnerInstruction<'static> = InnerInstruction {
    outer_index: 0,
    program_id_index: 0,
    accounts: &[],
    data: &[],
};

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> MetaIn {
        MetaIn {
            tx_index: Some(17),
            failed: false,
            fee: 105_000,
            loaded_writable: vec![[1; 32], [2; 32]],
            loaded_readonly: vec![[3; 32]],
            inner: Some(vec![
                InnerOwned {
                    outer_index: 2,
                    program_id_index: 11,
                    accounts: vec![0, 1],
                    data: vec![9; 40],
                },
                InnerOwned {
                    outer_index: 3,
                    program_id_index: 23,
                    accounts: vec![],
                    data: vec![],
                },
            ]),
        }
    }

    #[test]
    fn roundtrip() {
        let m = sample();
        let mut b = vec![0xAA]; // something before the trailer
        let n = encode(&m, &mut b);
        assert_eq!(n, b.len() - 1);
        let w = WireMeta::parse(&b[1..]).unwrap();
        assert_eq!(w.tx_index, Some(17));
        assert!(!w.failed() && w.has_inner() && !w.inner_truncated());
        assert_eq!(w.fee, 105_000);
        let mut keys = [Pubkey::default(); 8];
        assert_eq!(w.loaded_into(&mut keys), 3);
        assert_eq!(keys[0], Pubkey::new_from_array([1; 32]));
        assert_eq!(keys[2], Pubkey::new_from_array([3; 32]));
        let mut inn = [EMPTY_INNER; 4];
        assert_eq!(w.inner_into(&mut inn), 2);
        assert_eq!(inn[0].outer_index, 2);
        assert_eq!(inn[0].program_id_index, 11);
        assert_eq!(inn[0].accounts, &[0, 1]);
        assert_eq!(inn[0].data, &[9; 40][..]);
        assert_eq!(inn[1].outer_index, 3);
        assert!(inn[1].data.is_empty());
        // a short output keeps the first ones
        let mut one = [EMPTY_INNER; 1];
        assert_eq!(w.inner_into(&mut one), 1);
    }

    #[test]
    fn unknown_index_failed_and_no_inner() {
        let m = MetaIn {
            tx_index: None,
            failed: true,
            inner: None,
            ..Default::default()
        };
        let mut b = vec![];
        encode(&m, &mut b);
        let w = WireMeta::parse(&b).unwrap();
        assert_eq!(w.tx_index, None);
        assert!(w.failed() && !w.has_inner());
        assert_eq!((w.n_loaded(), w.n_inner()), (0, 0));
    }

    #[test]
    fn truncation_and_malformed() {
        let mut m = sample();
        let one = m.inner.as_ref().unwrap()[0].clone();
        m.inner = Some(vec![one; MAX_INNER + 5]);
        let mut b = vec![];
        encode(&m, &mut b);
        let w = WireMeta::parse(&b).unwrap();
        assert!(w.inner_truncated());
        assert_eq!(w.n_inner(), MAX_INNER);
        // every prefix of a valid trailer is refused or shorter-but-valid, never a panic
        for cut in 0..b.len() {
            if let Some(w) = WireMeta::parse(&b[..cut]) {
                let mut inn = [EMPTY_INNER; MAX_INNER];
                let _ = w.inner_into(&mut inn);
            }
        }
        assert!(WireMeta::parse(&[]).is_none());
    }
}
