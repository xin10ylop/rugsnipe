//! Fixed-order message compiler.
//!
//! The caller fixes the order of every account key (Solana only requires the
//! four classes signer-writable, signer-readonly, writable, readonly in that
//! order; inside a class the order is free). Fixed positions are what make
//! the [`crate::template::PatchTable`] possible: a mint-dependent key always
//! sits at the same byte offset, so a new mint is a few `memcpy`s.
//!
//! Layouts written:
//! * legacy: `compact(n_sigs) | sigs | header(3) | compact(n_keys) | keys |
//!   blockhash | compact(n_ix) | (prog u8 | compact(n) | idx | compact(len) | data)*`;
//! * v1 (SIMD-0385): `0x81 | header(3) | mask u32 | lifetime | n_ix u8 |
//!   n_addr u8 | keys | config values | (prog u8, n_acc u8, len u16)* |
//!   (idx | data)* | sigs` (signatures at the tail).

use pump_codec::{AccountMeta, Instruction, Pubkey};

/// v1 transaction config (mask bits 0-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct V1Config {
    /// Bits 0+1: total priority fee in lamports.
    pub priority_fee_lamports: u64,
    /// Bit 2 (unset = 0 = the transaction fails and still pays).
    pub compute_unit_limit: u32,
    /// Bit 3 (unset = 0 = fails on load).
    pub loaded_accounts_data_size_limit: u32,
}

/// Which format to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Legacy,
    V1(V1Config),
}

/// One ordered key with its class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeySpec {
    pub key: Pubkey,
    pub signer: bool,
    pub writable: bool,
}

impl KeySpec {
    pub const fn new(key: Pubkey, signer: bool, writable: bool) -> Self {
        Self {
            key,
            signer,
            writable,
        }
    }
}

/// Where things landed in the serialized transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// Offset of key `i` (32 bytes each).
    pub key_offsets: Vec<usize>,
    /// Offset of the blockhash / lifetime.
    pub lifetime: usize,
    /// Offset of each instruction's data.
    pub ix_data: Vec<usize>,
    /// v1: offset of the u64 priority fee in the config values.
    pub v1_priority: Option<usize>,
    /// v1: offsets of the u32 CU limit and LADS values.
    pub v1_cu_limit: Option<usize>,
    pub v1_lads: Option<usize>,
    /// First signature byte (64 bytes per signer, in key order).
    pub sigs: usize,
    pub n_sigs: usize,
    /// Signed message range `[start, end)`.
    pub message: (usize, usize),
}

/// Compile error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    /// Keys are not in signer-w, signer-ro, w, ro class order.
    ClassOrder(usize),
    DuplicateKey(Pubkey),
    /// An instruction uses a key that is not in the list.
    MissingKey(Pubkey),
    /// An instruction needs a flag (signer / writable) the key's class lacks.
    FlagMismatch(Pubkey),
    TooMany(&'static str),
    TooLong(usize),
}

impl core::fmt::Display for CompileError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for CompileError {}

fn class(k: &KeySpec) -> u8 {
    match (k.signer, k.writable) {
        (true, true) => 0,
        (true, false) => 1,
        (false, true) => 2,
        (false, false) => 3,
    }
}

fn compact_u16(out: &mut Vec<u8>, mut v: usize) {
    loop {
        let mut b = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            b |= 0x80;
        }
        out.push(b);
        if v == 0 {
            break;
        }
    }
}

/// The key order for `ixs` paid by `payer`, for a transaction whose layout
/// nothing patches (the cold paths, `rugsnipe-long`): the payer first, then
/// every other key in first-use order within its class (signer-writable,
/// signer-readonly, writable, readonly), each with the union of the flags
/// its instructions ask for. Program ids are read-only.
pub fn keys_for(payer: &Pubkey, ixs: &[Instruction]) -> Vec<KeySpec> {
    let mut v = vec![KeySpec::new(*payer, true, true)];
    let mut add = |k: Pubkey, s: bool, w: bool| match v.iter_mut().find(|e| e.key == k) {
        Some(e) => {
            e.signer |= s;
            e.writable |= w;
        }
        None => v.push(KeySpec::new(k, s, w)),
    };
    for ix in ixs {
        for m in &ix.accounts {
            add(m.pubkey, m.is_signer, m.is_writable);
        }
        add(ix.program_id, false, false);
    }
    let mut rest = v.split_off(1);
    rest.sort_by_key(class);
    v[0].signer = true;
    v[0].writable = true;
    v.extend(rest);
    v
}

/// [`keys_for`] + [`compile`] into a legacy transaction, signed by `sign`
/// (the payer's signature over the message; the payer must be the only
/// signer). Returns the bytes and the signature.
pub fn compile_signed_legacy(
    payer: &Pubkey,
    ixs: &[Instruction],
    blockhash: &[u8; 32],
    sign: impl FnOnce(&[u8]) -> [u8; 64],
) -> Result<(Vec<u8>, [u8; 64]), CompileError> {
    let keys = keys_for(payer, ixs);
    let (mut bytes, layout) = compile(&keys, ixs, blockhash, Format::Legacy)?;
    if layout.n_sigs != 1 {
        return Err(CompileError::TooMany("signers (the payer must sign alone)"));
    }
    let sig = sign(&bytes[layout.message.0..layout.message.1]);
    bytes[layout.sigs..layout.sigs + 64].copy_from_slice(&sig);
    Ok((bytes, sig))
}

/// Compile `ixs` over the exact key order `keys` into a zero-signed
/// transaction. Every instruction account must exist in `keys` with at
/// least the flags the instruction asks for; extra flags are an error
/// unless another instruction needs them (no silent privilege escalation).
pub fn compile(
    keys: &[KeySpec],
    ixs: &[Instruction],
    lifetime: &[u8; 32],
    format: Format,
) -> Result<(Vec<u8>, Layout), CompileError> {
    // class order + duplicates
    for i in 1..keys.len() {
        if class(&keys[i]) < class(&keys[i - 1]) {
            return Err(CompileError::ClassOrder(i));
        }
    }
    for (i, k) in keys.iter().enumerate() {
        if keys[..i].iter().any(|o| o.key == k.key) {
            return Err(CompileError::DuplicateKey(k.key));
        }
    }
    let idx = |k: &Pubkey| keys.iter().position(|s| s.key == *k);
    // flags: needed (union over instructions) must equal the class flags
    let mut need = vec![(false, false); keys.len()];
    let mut used = vec![false; keys.len()];
    let mut compiled: Vec<(u8, Vec<u8>, &[u8])> = Vec::with_capacity(ixs.len());
    for ix in ixs {
        let p = idx(&ix.program_id).ok_or(CompileError::MissingKey(ix.program_id))?;
        used[p] = true;
        let mut accts = Vec::with_capacity(ix.accounts.len());
        for AccountMeta {
            pubkey,
            is_signer,
            is_writable,
        } in &ix.accounts
        {
            let i = idx(pubkey).ok_or(CompileError::MissingKey(*pubkey))?;
            used[i] = true;
            need[i].0 |= *is_signer;
            need[i].1 |= *is_writable;
            accts.push(i as u8);
        }
        compiled.push((p as u8, accts, &ix.data));
    }
    for (i, k) in keys.iter().enumerate() {
        // The fee payer (key 0) is always a writable signer.
        let (s, w) = if i == 0 { (true, true) } else { need[i] };
        if (s, w) != (k.signer, k.writable) || !(used[i] || i == 0) {
            return Err(CompileError::FlagMismatch(k.key));
        }
    }
    let n_sig = keys.iter().filter(|k| k.signer).count();
    let n_ro_signed = keys.iter().filter(|k| k.signer && !k.writable).count();
    let n_ro_unsigned = keys.iter().filter(|k| !k.signer && !k.writable).count();
    if keys.len() > 64 || ixs.len() > 64 {
        return Err(CompileError::TooMany("keys/instructions (v1 limit 64)"));
    }
    let header = [n_sig as u8, n_ro_signed as u8, n_ro_unsigned as u8];
    let mut out = Vec::with_capacity(1_232);
    let mut key_offsets = Vec::with_capacity(keys.len());
    let mut ix_data = Vec::with_capacity(ixs.len());
    let layout = match format {
        Format::Legacy => {
            compact_u16(&mut out, n_sig);
            let sigs = out.len();
            out.resize(out.len() + 64 * n_sig, 0);
            let msg_start = out.len();
            out.extend_from_slice(&header);
            compact_u16(&mut out, keys.len());
            for k in keys {
                key_offsets.push(out.len());
                out.extend_from_slice(k.key.as_ref());
            }
            let lt = out.len();
            out.extend_from_slice(lifetime);
            compact_u16(&mut out, compiled.len());
            for (p, a, d) in &compiled {
                out.push(*p);
                compact_u16(&mut out, a.len());
                out.extend_from_slice(a);
                compact_u16(&mut out, d.len());
                ix_data.push(out.len());
                out.extend_from_slice(d);
            }
            Layout {
                key_offsets,
                lifetime: lt,
                ix_data,
                v1_priority: None,
                v1_cu_limit: None,
                v1_lads: None,
                sigs,
                n_sigs: n_sig,
                message: (msg_start, out.len()),
            }
        }
        Format::V1(cfg) => {
            out.push(0x81);
            out.extend_from_slice(&header);
            out.extend_from_slice(&0x0Fu32.to_le_bytes());
            let lt = out.len();
            out.extend_from_slice(lifetime);
            out.push(compiled.len() as u8);
            out.push(keys.len() as u8);
            for k in keys {
                key_offsets.push(out.len());
                out.extend_from_slice(k.key.as_ref());
            }
            let pr = out.len();
            out.extend_from_slice(&cfg.priority_fee_lamports.to_le_bytes());
            let cu = out.len();
            out.extend_from_slice(&cfg.compute_unit_limit.to_le_bytes());
            let la = out.len();
            out.extend_from_slice(&cfg.loaded_accounts_data_size_limit.to_le_bytes());
            for (p, a, d) in &compiled {
                if a.len() > 255 || d.len() > u16::MAX as usize {
                    return Err(CompileError::TooMany("v1 instruction accounts/data"));
                }
                out.push(*p);
                out.push(a.len() as u8);
                out.extend_from_slice(&(d.len() as u16).to_le_bytes());
            }
            for (_, a, d) in &compiled {
                out.extend_from_slice(a);
                ix_data.push(out.len());
                out.extend_from_slice(d);
            }
            let msg_end = out.len();
            let sigs = out.len();
            out.resize(out.len() + 64 * n_sig, 0);
            Layout {
                key_offsets,
                lifetime: lt,
                ix_data,
                v1_priority: Some(pr),
                v1_cu_limit: Some(cu),
                v1_lads: Some(la),
                sigs,
                n_sigs: n_sig,
                message: (0, msg_end),
            }
        }
    };
    if out.len() > crate::MAX_TX_LEN {
        return Err(CompileError::TooLong(out.len()));
    }
    Ok((out, layout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_encoding() {
        for (v, want) in [
            (0usize, vec![0u8]),
            (127, vec![0x7f]),
            (128, vec![0x80, 1]),
            (16384, vec![0x80, 0x80, 1]),
        ] {
            let mut o = vec![];
            compact_u16(&mut o, v);
            assert_eq!(o, want);
        }
    }

    #[test]
    fn rejects_bad_orders_and_flags() {
        let (a, b, p) = (
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        );
        let ix = Instruction {
            program_id: p,
            accounts: vec![AccountMeta::new(b, false)],
            data: vec![1],
        };
        let ok = [
            KeySpec::new(a, true, true),
            KeySpec::new(b, false, true),
            KeySpec::new(p, false, false),
        ];
        assert!(compile(&ok, std::slice::from_ref(&ix), &[0; 32], Format::Legacy).is_ok());
        let bad_order = [
            KeySpec::new(a, true, true),
            KeySpec::new(p, false, false),
            KeySpec::new(b, false, true),
        ];
        assert_eq!(
            compile(
                &bad_order,
                std::slice::from_ref(&ix),
                &[0; 32],
                Format::Legacy
            )
            .unwrap_err(),
            CompileError::ClassOrder(2)
        );
        let escalated = [
            KeySpec::new(a, true, true),
            KeySpec::new(b, true, true),
            KeySpec::new(p, false, false),
        ];
        assert_eq!(
            compile(
                &escalated,
                std::slice::from_ref(&ix),
                &[0; 32],
                Format::Legacy
            )
            .unwrap_err(),
            CompileError::FlagMismatch(b)
        );
        let missing = [KeySpec::new(a, true, true), KeySpec::new(p, false, false)];
        assert_eq!(
            compile(&missing, &[ix], &[0; 32], Format::Legacy).unwrap_err(),
            CompileError::MissingKey(b)
        );
    }
}
