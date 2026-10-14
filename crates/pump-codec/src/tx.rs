//! Zero-copy view over a raw wire transaction: legacy, v0 and v1 (SIMD-0385).
//!
//! Layouts:
//!
//! * **legacy**: `compact(n_sigs) || sigs[64*n] || header(3) || compact(n_keys) ||
//!   keys[32*n] || blockhash(32) || compact(n_ix) || ix*`, where each ix is
//!   `program_idx(u8) || compact(n) || acct_idx[n] || compact(len) || data[len]`.
//! * **v0**: as legacy, but the message starts with `0x80` and ends with
//!   `compact(n_lookups) || (table(32) || compact(w) || w_idx || compact(r) || r_idx)*`.
//! * **v1** (SIMD-0385, version byte `0x81`, *signatures at the tail*):
//!   `0x81 || header(3) || config_mask(u32 LE) || lifetime(32) || n_ix(u8) ||
//!   n_addr(u8) || addrs[32*n] || config_values[4*popcount(mask)] ||
//!   ix_headers[(prog u8, n_acct u8, data_len u16 LE) * n_ix] ||
//!   payloads (acct idx || data per ix) || sigs[64 * num_required_signatures]`.
//!   No address lookup tables; compute budget lives in the config mask:
//!   bits 0+1 priority fee (u64 total lamports), bit 2 CU limit, bit 3 loaded
//!   accounts data size, bit 4 heap size.
//!
//! [`TxView::parse`] validates the whole buffer once (bounds, counts, exact
//! length, index ranges); accessors then slice into the input without
//! allocating. `agave-transaction-view` 6.1 compiles in this workspace and is
//! used in the tests as an oracle for every fixture, but the hot path keeps
//! this self-contained parser (no Agave version pin, no `bytes` crate, and
//! per-instruction access without iterator state).

use crate::bytes::read_compact_u16;
use crate::constants::COMPUTE_BUDGET_PROGRAM_ID;
use crate::error::{DecodeError, Result};
use crate::instructions::AccountLookup;
use solana_pubkey::Pubkey;

/// Transaction format version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TxVersion {
    Legacy,
    V0,
    V1,
}

/// The 3-byte message header shared by every format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageHeader {
    pub num_required_signatures: u8,
    pub num_readonly_signed_accounts: u8,
    pub num_readonly_unsigned_accounts: u8,
}

/// SIMD-0385 `TransactionConfigMask` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct V1Config {
    pub mask: u32,
    /// Bits 0+1: total priority fee in lamports (not micro-lamports per CU).
    pub priority_fee_lamports: Option<u64>,
    /// Bit 2.
    pub compute_unit_limit: Option<u32>,
    /// Bit 3.
    pub loaded_accounts_data_size_limit: Option<u32>,
    /// Bit 4.
    pub heap_size: Option<u32>,
}

/// One compiled instruction (indices into the transaction's account list).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstructionView<'a> {
    pub program_id_index: u8,
    pub accounts: &'a [u8],
    pub data: &'a [u8],
}

/// One v0 address-table lookup (unresolved).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddressTableLookupView<'a> {
    pub account_key: &'a Pubkey,
    pub writable_indexes: &'a [u8],
    pub readonly_indexes: &'a [u8],
}

/// Compute-budget requests of a transaction, from the v1 config or from
/// ComputeBudget instructions (legacy/v0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ComputeBudgetRequest {
    pub compute_unit_limit: Option<u32>,
    /// Legacy/v0: micro-lamports per CU (`SetComputeUnitPrice`).
    pub compute_unit_price_micro_lamports: Option<u64>,
    /// v1: total priority fee in lamports.
    pub priority_fee_lamports: Option<u64>,
    pub loaded_accounts_data_size_limit: Option<u32>,
    pub heap_size: Option<u32>,
}

impl ComputeBudgetRequest {
    /// Priority fee in lamports: v1 total, or `ceil(price * limit / 1e6)` for
    /// legacy/v0 (`None` when unknown).
    pub fn priority_fee_lamports_total(&self) -> Option<u64> {
        if let Some(f) = self.priority_fee_lamports {
            return Some(f);
        }
        let price = self.compute_unit_price_micro_lamports? as u128;
        let limit = self.compute_unit_limit? as u128;
        Some(u64::try_from((price * limit).div_ceil(1_000_000)).unwrap_or(u64::MAX))
    }
}

#[derive(Debug, Clone, Copy)]
enum IxLayout {
    /// legacy/v0: offset of the first instruction (after the compact count).
    Compact { start: usize },
    /// v1: offset of the header table and of the payloads.
    V1 { headers: usize, payloads: usize },
}

/// Zero-copy, pre-validated view of a wire transaction.
#[derive(Debug, Clone, Copy)]
pub struct TxView<'a> {
    raw: &'a [u8],
    version: TxVersion,
    header: MessageHeader,
    sigs: (usize, usize),
    message: (usize, usize),
    keys: (usize, usize),
    blockhash: usize,
    num_instructions: usize,
    ix_layout: IxLayout,
    lookups: (usize, usize),
    num_lookup_writable: usize,
    num_lookup_readonly: usize,
    v1_config: Option<V1Config>,
}

const SIG: usize = 64;
const KEY: usize = 32;

#[inline]
fn need(raw: &[u8], offset: usize, n: usize) -> Result<()> {
    if raw.len() < offset || raw.len() - offset < n {
        Err(DecodeError::UnexpectedEof {
            offset,
            needed: n - raw.len().saturating_sub(offset),
        })
    } else {
        Ok(())
    }
}

#[inline]
fn compact(raw: &[u8], offset: &mut usize) -> Result<usize> {
    let (v, n) = read_compact_u16(raw, *offset)?;
    *offset += n;
    Ok(v as usize)
}

#[inline]
fn as_pubkeys(bytes: &[u8]) -> &[Pubkey] {
    // SAFETY: `Pubkey` (solana_address::Address) is `#[repr(transparent)]`
    // over `[u8; 32]`: size 32, alignment 1, no invalid bit patterns. `bytes`
    // has a length that is a multiple of 32 (checked by every caller), so
    // reinterpreting it as `len / 32` pubkeys is sound and the lifetime is
    // inherited from `bytes`.
    const _: () =
        assert!(core::mem::size_of::<Pubkey>() == 32 && core::mem::align_of::<Pubkey>() == 1);
    debug_assert!(bytes.len().is_multiple_of(KEY));
    unsafe { core::slice::from_raw_parts(bytes.as_ptr().cast::<Pubkey>(), bytes.len() / KEY) }
}

impl<'a> TxView<'a> {
    /// Parse and validate `raw` (one complete serialized transaction, no trailing bytes).
    pub fn parse(raw: &'a [u8]) -> Result<Self> {
        let tx = Self::parse_any(raw, true)?;
        tx.validate_indexes()?;
        Ok(tx)
    }

    /// Parse the transaction at the **start** of `buf` (trailing bytes are
    /// allowed) and return it with its serialized length. This is how a
    /// bincode `Entry` (`transactions: Vec<VersionedTransaction>`) is sliced:
    /// legacy, v0 and v1 transactions serialize there exactly as on the wire.
    /// The returned view's [`TxView::raw`] is `&buf[..len]`.
    pub fn parse_prefix(buf: &'a [u8]) -> Result<(Self, usize)> {
        let tx = Self::parse_any(buf, false)?;
        tx.validate_indexes()?;
        let len = tx.raw.len();
        Ok((tx, len))
    }

    fn parse_any(raw: &'a [u8], exact: bool) -> Result<Self> {
        let first = *raw.first().ok_or(DecodeError::UnexpectedEof {
            offset: 0,
            needed: 1,
        })?;
        // Dispatch like Agave: a set MSB on byte 0 means a versioned (v1+)
        // envelope. A legacy/v0 tx would need >= 128 signatures for that.
        if first == 0x81 {
            Self::parse_v1(raw, exact)
        } else if first & 0x80 != 0 {
            Err(DecodeError::InvalidTransaction(
                "unsupported transaction version",
            ))
        } else {
            Self::parse_legacy_or_v0(raw, exact)
        }
    }

    fn parse_legacy_or_v0(raw: &'a [u8], exact: bool) -> Result<Self> {
        let mut o = 0usize;
        let n_sigs = compact(raw, &mut o)?;
        need(raw, o, n_sigs * SIG)?;
        let sigs = (o, n_sigs);
        o += n_sigs * SIG;
        let msg_start = o;
        need(raw, o, 1)?;
        let version = if raw[o] & 0x80 != 0 {
            match raw[o] & 0x7f {
                0 => {
                    o += 1;
                    TxVersion::V0
                }
                _ => {
                    return Err(DecodeError::InvalidTransaction(
                        "unsupported message version",
                    ))
                }
            }
        } else {
            TxVersion::Legacy
        };
        need(raw, o, 3)?;
        let header = MessageHeader {
            num_required_signatures: raw[o],
            num_readonly_signed_accounts: raw[o + 1],
            num_readonly_unsigned_accounts: raw[o + 2],
        };
        o += 3;
        if header.num_required_signatures as usize != n_sigs {
            return Err(DecodeError::InvalidTransaction(
                "signature count != num_required_signatures",
            ));
        }
        let n_keys = compact(raw, &mut o)?;
        need(raw, o, n_keys * KEY)?;
        let keys = (o, n_keys);
        o += n_keys * KEY;
        need(raw, o, 32)?;
        let blockhash = o;
        o += 32;
        let n_ix = compact(raw, &mut o)?;
        let ix_start = o;
        for _ in 0..n_ix {
            need(raw, o, 1)?;
            o += 1;
            let na = compact(raw, &mut o)?;
            need(raw, o, na)?;
            o += na;
            let nd = compact(raw, &mut o)?;
            need(raw, o, nd)?;
            o += nd;
        }
        let (mut lookups, mut nw, mut nr) = ((o, 0usize), 0usize, 0usize);
        if version == TxVersion::V0 {
            let n_l = compact(raw, &mut o)?;
            lookups = (o, n_l);
            for _ in 0..n_l {
                need(raw, o, KEY)?;
                o += KEY;
                let w = compact(raw, &mut o)?;
                need(raw, o, w)?;
                o += w;
                let r = compact(raw, &mut o)?;
                need(raw, o, r)?;
                o += r;
                nw += w;
                nr += r;
            }
        }
        if exact && o != raw.len() {
            return Err(DecodeError::InvalidTransaction(
                "trailing bytes after message",
            ));
        }
        let raw = &raw[..o];
        Ok(Self {
            raw,
            version,
            header,
            sigs,
            message: (msg_start, o),
            keys,
            blockhash,
            num_instructions: n_ix,
            ix_layout: IxLayout::Compact { start: ix_start },
            lookups,
            num_lookup_writable: nw,
            num_lookup_readonly: nr,
            v1_config: None,
        })
    }

    fn parse_v1(raw: &'a [u8], exact: bool) -> Result<Self> {
        // 0x81 | header(3) | mask(4) | lifetime(32) | n_ix | n_addr
        need(raw, 0, 1 + 3 + 4 + 32 + 2)?;
        let header = MessageHeader {
            num_required_signatures: raw[1],
            num_readonly_signed_accounts: raw[2],
            num_readonly_unsigned_accounts: raw[3],
        };
        let mask = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]);
        let blockhash = 8;
        let n_ix = raw[40] as usize;
        let n_addr = raw[41] as usize;
        let mut o = 42;
        need(raw, o, n_addr * KEY)?;
        let keys = (o, n_addr);
        o += n_addr * KEY;
        // Config values: one 4-byte slot per set bit, in ascending bit order.
        let slots = mask.count_ones() as usize;
        need(raw, o, slots * 4)?;
        if (mask & 0b11) == 0b01 || (mask & 0b11) == 0b10 {
            return Err(DecodeError::InvalidTransaction(
                "v1 priority fee needs both mask bits 0 and 1",
            ));
        }
        let cfg_bytes = &raw[o..o + slots * 4];
        let mut cfg = V1Config {
            mask,
            ..Default::default()
        };
        let mut slot = 0usize;
        let u32_at = |s: usize| u32::from_le_bytes(cfg_bytes[4 * s..4 * s + 4].try_into().unwrap());
        if mask & 0b11 == 0b11 {
            cfg.priority_fee_lamports =
                Some(u64::from_le_bytes(cfg_bytes[0..8].try_into().unwrap()));
            slot = 2;
        }
        for bit in 2..32u32 {
            if mask & (1 << bit) == 0 {
                continue;
            }
            let v = u32_at(slot);
            match bit {
                2 => cfg.compute_unit_limit = Some(v),
                3 => cfg.loaded_accounts_data_size_limit = Some(v),
                4 => cfg.heap_size = Some(v),
                _ => {} // unknown future field: skipped, still visible in `mask`
            }
            slot += 1;
        }
        o += slots * 4;
        let headers = o;
        need(raw, o, n_ix * 4)?;
        let mut payload_len = 0usize;
        for i in 0..n_ix {
            let h = &raw[headers + 4 * i..headers + 4 * i + 4];
            payload_len += h[1] as usize + u16::from_le_bytes([h[2], h[3]]) as usize;
        }
        o += n_ix * 4;
        let payloads = o;
        need(raw, o, payload_len)?;
        o += payload_len;
        let msg_end = o;
        let n_sigs = header.num_required_signatures as usize;
        need(raw, o, n_sigs * SIG)?;
        let sigs = (o, n_sigs);
        o += n_sigs * SIG;
        if exact && o != raw.len() {
            return Err(DecodeError::InvalidTransaction(
                "trailing bytes after v1 signatures",
            ));
        }
        let raw = &raw[..o];
        Ok(Self {
            raw,
            version: TxVersion::V1,
            header,
            sigs,
            message: (0, msg_end),
            keys,
            blockhash,
            num_instructions: n_ix,
            ix_layout: IxLayout::V1 { headers, payloads },
            lookups: (msg_end, 0),
            num_lookup_writable: 0,
            num_lookup_readonly: 0,
            v1_config: Some(cfg),
        })
    }

    fn validate_indexes(&self) -> Result<()> {
        let h = self.header;
        if h.num_required_signatures == 0
            || h.num_readonly_signed_accounts >= h.num_required_signatures
        {
            return Err(DecodeError::InvalidTransaction("bad message header"));
        }
        let n_static = self.keys.1;
        if n_static < h.num_required_signatures as usize + h.num_readonly_unsigned_accounts as usize
        {
            return Err(DecodeError::InvalidTransaction(
                "fewer keys than the header requires",
            ));
        }
        let total = self.total_account_count();
        for ix in self.instructions() {
            if ix.program_id_index as usize >= n_static {
                return Err(DecodeError::InvalidTransaction(
                    "program id index out of static keys",
                ));
            }
            if ix.accounts.iter().any(|&a| a as usize >= total) {
                return Err(DecodeError::InvalidTransaction(
                    "account index out of range",
                ));
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ accessors
    /// The full raw transaction.
    pub fn raw(&self) -> &'a [u8] {
        self.raw
    }
    pub fn version(&self) -> TxVersion {
        self.version
    }
    pub fn header(&self) -> MessageHeader {
        self.header
    }
    /// The signed message bytes (legacy/v0: after the signatures; v1: before them).
    pub fn message_bytes(&self) -> &'a [u8] {
        &self.raw[self.message.0..self.message.1]
    }
    /// Byte offset of the signature block in `raw` (v1: the tail).
    pub fn signatures_offset(&self) -> usize {
        self.sigs.0
    }
    /// Number of signatures.
    pub fn num_signatures(&self) -> usize {
        self.sigs.1
    }
    /// Signature `i` (64 bytes).
    pub fn signature(&self, i: usize) -> Option<&'a [u8; 64]> {
        if i >= self.sigs.1 {
            return None;
        }
        let o = self.sigs.0 + i * SIG;
        self.raw[o..o + SIG].try_into().ok()
    }
    /// All signatures as one byte slice (`64 * n`).
    pub fn signatures_bytes(&self) -> &'a [u8] {
        &self.raw[self.sigs.0..self.sigs.0 + self.sigs.1 * SIG]
    }
    /// The first signature = the transaction ID (base58 for RPC).
    pub fn transaction_id(&self) -> Option<String> {
        self.signature(0).map(|s| bs58::encode(s).into_string())
    }
    /// Static account keys (zero-copy).
    pub fn static_account_keys(&self) -> &'a [Pubkey] {
        as_pubkeys(&self.raw[self.keys.0..self.keys.0 + self.keys.1 * KEY])
    }
    /// Fee payer (first static key).
    pub fn fee_payer(&self) -> &'a Pubkey {
        &self.static_account_keys()[0]
    }
    /// Signer keys (first `num_required_signatures` static keys).
    pub fn signer_keys(&self) -> &'a [Pubkey] {
        &self.static_account_keys()[..self.header.num_required_signatures as usize]
    }
    /// Recent blockhash / lifetime specifier (a nonce value for durable-nonce txs).
    pub fn recent_blockhash(&self) -> &'a [u8; 32] {
        self.raw[self.blockhash..self.blockhash + 32]
            .try_into()
            .unwrap()
    }
    /// v1 config (`None` for legacy/v0).
    pub fn v1_config(&self) -> Option<V1Config> {
        self.v1_config
    }
    /// Number of instructions.
    pub fn num_instructions(&self) -> usize {
        self.num_instructions
    }
    /// Static keys + ALT-loaded writable + ALT-loaded readonly.
    pub fn total_account_count(&self) -> usize {
        self.keys.1 + self.num_lookup_writable + self.num_lookup_readonly
    }
    /// Number of ALT-loaded writable / readonly accounts (v0).
    pub fn num_lookup_accounts(&self) -> (usize, usize) {
        (self.num_lookup_writable, self.num_lookup_readonly)
    }

    /// Iterate instructions in order.
    pub fn instructions(&self) -> InstructionIter<'a> {
        InstructionIter {
            raw: self.raw,
            layout: self.ix_layout,
            remaining: self.num_instructions,
            index: 0,
            cursor: match self.ix_layout {
                IxLayout::Compact { start } => start,
                IxLayout::V1 { payloads, .. } => payloads,
            },
        }
    }

    /// Instruction `i` (walks from the start for legacy/v0; O(i)).
    pub fn instruction(&self, i: usize) -> Option<InstructionView<'a>> {
        self.instructions().nth(i)
    }

    /// v0 address-table lookups (empty for legacy/v1).
    pub fn address_table_lookups(&self) -> LookupIter<'a> {
        LookupIter {
            raw: self.raw,
            cursor: self.lookups.0,
            remaining: self.lookups.1,
        }
    }

    /// Program ID of an instruction (always a static key).
    pub fn program_id(&self, ix: &InstructionView<'_>) -> &'a Pubkey {
        &self.static_account_keys()[ix.program_id_index as usize]
    }

    /// Account list of an instruction as an [`AccountLookup`]. Static keys
    /// resolve; ALT-loaded ones resolve only if `loaded` is supplied (the
    /// `meta.loadedAddresses` writable list followed by the readonly list).
    pub fn instruction_accounts<'b>(
        &self,
        ix: &InstructionView<'b>,
        loaded: Option<&'b [Pubkey]>,
    ) -> IxAccounts<'b>
    where
        'a: 'b,
    {
        IxAccounts {
            static_keys: self.static_account_keys(),
            loaded,
            indexes: ix.accounts,
        }
    }

    /// Account `index` in the full ordering (static keys, then the ALT-loaded
    /// writable and readonly addresses from `loaded`).
    pub fn account_key(&self, index: usize, loaded: Option<&[Pubkey]>) -> Option<Pubkey> {
        let st = self.static_account_keys();
        if index < st.len() {
            Some(st[index])
        } else {
            loaded?.get(index - st.len()).copied()
        }
    }

    /// Accounts of an inner (CPI) instruction from transaction meta: its
    /// `accounts` are indexes into the same full ordering as top-level ones.
    pub fn indexed_accounts<'b>(
        &self,
        indexes: &'b [u8],
        loaded: Option<&'b [Pubkey]>,
    ) -> IxAccounts<'b>
    where
        'a: 'b,
    {
        IxAccounts {
            static_keys: self.static_account_keys(),
            loaded,
            indexes,
        }
    }

    /// `true` if account `index` (in the full ordering) is a signer.
    pub fn is_signer(&self, index: usize) -> bool {
        index < self.header.num_required_signatures as usize
    }

    /// Whether account `index` is requested writable (header rules; ALT
    /// writable section counts as writable). Does not apply runtime demotion.
    pub fn is_writable(&self, index: usize) -> bool {
        let h = self.header;
        let n_static = self.keys.1;
        let n_sig = h.num_required_signatures as usize;
        if index < n_sig {
            index < n_sig - h.num_readonly_signed_accounts as usize
        } else if index < n_static {
            index < n_static - h.num_readonly_unsigned_accounts as usize
        } else {
            index < n_static + self.num_lookup_writable
        }
    }

    /// Compute-budget requests (v1 config, or ComputeBudget instructions).
    pub fn compute_budget(&self) -> ComputeBudgetRequest {
        if let Some(cfg) = self.v1_config {
            return ComputeBudgetRequest {
                compute_unit_limit: cfg.compute_unit_limit,
                compute_unit_price_micro_lamports: None,
                priority_fee_lamports: cfg.priority_fee_lamports,
                loaded_accounts_data_size_limit: cfg.loaded_accounts_data_size_limit,
                heap_size: cfg.heap_size,
            };
        }
        let mut out = ComputeBudgetRequest::default();
        for ix in self.instructions() {
            if *self.program_id(&ix) != COMPUTE_BUDGET_PROGRAM_ID {
                continue;
            }
            let d = ix.data;
            let u32_arg = || {
                d.get(1..5)
                    .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            };
            match d.first() {
                Some(1) => out.heap_size = u32_arg(),
                Some(2) => out.compute_unit_limit = u32_arg(),
                Some(3) => {
                    out.compute_unit_price_micro_lamports = d
                        .get(1..9)
                        .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
                }
                Some(4) => out.loaded_accounts_data_size_limit = u32_arg(),
                _ => {}
            }
        }
        out
    }
}

/// Iterator over a [`TxView`]'s instructions.
#[derive(Debug, Clone)]
pub struct InstructionIter<'a> {
    raw: &'a [u8],
    layout: IxLayout,
    remaining: usize,
    index: usize,
    cursor: usize,
}

impl<'a> Iterator for InstructionIter<'a> {
    type Item = InstructionView<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        let raw = self.raw;
        // Bounds were validated by `TxView::parse`.
        let ix = match self.layout {
            IxLayout::Compact { .. } => {
                let mut o = self.cursor;
                let program_id_index = raw[o];
                o += 1;
                let na = compact(raw, &mut o).ok()?;
                let accounts = &raw[o..o + na];
                o += na;
                let nd = compact(raw, &mut o).ok()?;
                let data = &raw[o..o + nd];
                o += nd;
                self.cursor = o;
                InstructionView {
                    program_id_index,
                    accounts,
                    data,
                }
            }
            IxLayout::V1 { headers, .. } => {
                let h = &raw[headers + 4 * self.index..headers + 4 * self.index + 4];
                let na = h[1] as usize;
                let nd = u16::from_le_bytes([h[2], h[3]]) as usize;
                let o = self.cursor;
                self.cursor = o + na + nd;
                InstructionView {
                    program_id_index: h[0],
                    accounts: &raw[o..o + na],
                    data: &raw[o + na..o + na + nd],
                }
            }
        };
        self.index += 1;
        Some(ix)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for InstructionIter<'_> {}

/// Iterator over v0 address-table lookups.
#[derive(Debug, Clone)]
pub struct LookupIter<'a> {
    raw: &'a [u8],
    cursor: usize,
    remaining: usize,
}

impl<'a> Iterator for LookupIter<'a> {
    type Item = AddressTableLookupView<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        let raw = self.raw;
        let mut o = self.cursor;
        let account_key = &as_pubkeys(&raw[o..o + KEY])[0];
        o += KEY;
        let w = compact(raw, &mut o).ok()?;
        let writable_indexes = &raw[o..o + w];
        o += w;
        let r = compact(raw, &mut o).ok()?;
        let readonly_indexes = &raw[o..o + r];
        o += r;
        self.cursor = o;
        Some(AddressTableLookupView {
            account_key,
            writable_indexes,
            readonly_indexes,
        })
    }
}

/// An instruction's accounts resolved through a [`TxView`] (see
/// [`TxView::instruction_accounts`]).
#[derive(Debug, Clone, Copy)]
pub struct IxAccounts<'a> {
    static_keys: &'a [Pubkey],
    loaded: Option<&'a [Pubkey]>,
    indexes: &'a [u8],
}

impl IxAccounts<'_> {
    /// Transaction-level index of instruction account `i`.
    pub fn index(&self, i: usize) -> Option<u8> {
        self.indexes.get(i).copied()
    }
}

impl AccountLookup for IxAccounts<'_> {
    fn account_count(&self) -> usize {
        self.indexes.len()
    }
    fn account(&self, i: usize) -> Option<Pubkey> {
        let idx = *self.indexes.get(i)? as usize;
        if idx < self.static_keys.len() {
            Some(self.static_keys[idx])
        } else {
            self.loaded?.get(idx - self.static_keys.len()).copied()
        }
    }
}
