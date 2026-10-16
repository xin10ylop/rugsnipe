//! Yellowstone protobuf <-> wire bytes.
//!
//! A Yellowstone `SubscribeUpdateTransaction` carries the transaction as a
//! structured protobuf (`solana.storage.ConfirmedBlock.Transaction`), not as
//! its wire bytes. [`tx_to_wire`] re-serializes it exactly as the cluster
//! did (legacy, v0 with lookups, v1 / SIMD-0385 with its config mask), so
//! the hot thread runs the same `TxView` / `create_hit` code as on shreds
//! or recorded wire frames. [`wire_to_tx`] is the inverse (tests and the
//! local mock server); every pump-codec fixture round-trips byte for byte
//! (`tests/grpc_mock.rs`). [`meta_in`] turns the status meta into the
//! [`crate::wiremeta`] trailer.

use crate::wiremeta::{InnerOwned, MetaIn};
use pump_codec::tx::{TxVersion, TxView};
use yellowstone_grpc_proto::prelude::{
    CompiledInstruction, Message, MessageAddressTableLookup, MessageHeader, Transaction,
    TransactionConfig, TransactionStatusMeta,
};

fn put_compact(out: &mut Vec<u8>, mut v: usize) {
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

fn u8_of(v: u32, what: &'static str) -> Result<u8, String> {
    u8::try_from(v).map_err(|_| format!("{what} {v} does not fit a byte"))
}

/// Serialize a protobuf transaction to its wire bytes.
pub fn tx_to_wire(tx: &Transaction) -> Result<Vec<u8>, String> {
    let m = tx.message.as_ref().ok_or("transaction without message")?;
    let h = m.header.ok_or("message without header")?;
    let header = [
        u8_of(h.num_required_signatures, "num_required_signatures")?,
        u8_of(
            h.num_readonly_signed_accounts,
            "num_readonly_signed_accounts",
        )?,
        u8_of(
            h.num_readonly_unsigned_accounts,
            "num_readonly_unsigned_accounts",
        )?,
    ];
    if m.recent_blockhash.len() != 32 {
        return Err(format!("blockhash of {} bytes", m.recent_blockhash.len()));
    }
    if m.account_keys.iter().any(|k| k.len() != 32) {
        return Err("account key not 32 bytes".into());
    }
    if tx.signatures.iter().any(|s| s.len() != 64) {
        return Err("signature not 64 bytes".into());
    }
    let mut out = Vec::with_capacity(1280);
    if m.versioned && m.config.is_some() {
        // v1: no lookups; signatures at the tail
        if !m.address_table_lookups.is_empty() {
            return Err("v1 message with lookups".into());
        }
        if m.account_keys.len() > 255 || m.instructions.len() > 255 {
            return Err("v1 message too large".into());
        }
        let c = m.config.unwrap_or_default();
        let mut mask = 0u32;
        let mut vals: Vec<u8> = vec![];
        if let Some(p) = c.priority_fee {
            mask |= 0b11;
            vals.extend_from_slice(&p.to_le_bytes());
        }
        for (bit, v) in [
            (2, c.compute_unit_limit),
            (3, c.loaded_accounts_data_size_limit),
            (4, c.heap_size),
        ] {
            if let Some(v) = v {
                mask |= 1 << bit;
                vals.extend_from_slice(&v.to_le_bytes());
            }
        }
        out.push(0x81);
        out.extend_from_slice(&header);
        out.extend_from_slice(&mask.to_le_bytes());
        out.extend_from_slice(&m.recent_blockhash);
        out.push(m.instructions.len() as u8);
        out.push(m.account_keys.len() as u8);
        for k in &m.account_keys {
            out.extend_from_slice(k);
        }
        out.extend_from_slice(&vals);
        for ix in &m.instructions {
            if ix.accounts.len() > 255 || ix.data.len() > u16::MAX as usize {
                return Err("v1 instruction too large".into());
            }
            out.push(u8_of(ix.program_id_index, "program_id_index")?);
            out.push(ix.accounts.len() as u8);
            out.extend_from_slice(&(ix.data.len() as u16).to_le_bytes());
        }
        for ix in &m.instructions {
            out.extend_from_slice(&ix.accounts);
            out.extend_from_slice(&ix.data);
        }
        if tx.signatures.len() != header[0] as usize {
            return Err("v1 signature count != num_required_signatures".into());
        }
        for s in &tx.signatures {
            out.extend_from_slice(s);
        }
        return Ok(out);
    }
    put_compact(&mut out, tx.signatures.len());
    for s in &tx.signatures {
        out.extend_from_slice(s);
    }
    if m.versioned {
        out.push(0x80);
    }
    out.extend_from_slice(&header);
    put_compact(&mut out, m.account_keys.len());
    for k in &m.account_keys {
        out.extend_from_slice(k);
    }
    out.extend_from_slice(&m.recent_blockhash);
    put_compact(&mut out, m.instructions.len());
    for ix in &m.instructions {
        out.push(u8_of(ix.program_id_index, "program_id_index")?);
        put_compact(&mut out, ix.accounts.len());
        out.extend_from_slice(&ix.accounts);
        put_compact(&mut out, ix.data.len());
        out.extend_from_slice(&ix.data);
    }
    if m.versioned {
        put_compact(&mut out, m.address_table_lookups.len());
        for l in &m.address_table_lookups {
            if l.account_key.len() != 32 {
                return Err("lookup table key not 32 bytes".into());
            }
            out.extend_from_slice(&l.account_key);
            put_compact(&mut out, l.writable_indexes.len());
            out.extend_from_slice(&l.writable_indexes);
            put_compact(&mut out, l.readonly_indexes.len());
            out.extend_from_slice(&l.readonly_indexes);
        }
    } else if !m.address_table_lookups.is_empty() {
        return Err("legacy message with lookups".into());
    }
    Ok(out)
}

/// Parse wire bytes into the protobuf form (inverse of [`tx_to_wire`]).
pub fn wire_to_tx(raw: &[u8]) -> Result<Transaction, String> {
    let tx = TxView::parse(raw).map_err(|e| e.to_string())?;
    let h = tx.header();
    let instructions = tx
        .instructions()
        .map(|ix| CompiledInstruction {
            program_id_index: ix.program_id_index as u32,
            accounts: ix.accounts.to_vec(),
            data: ix.data.to_vec(),
        })
        .collect();
    let address_table_lookups = tx
        .address_table_lookups()
        .map(|l| MessageAddressTableLookup {
            account_key: l.account_key.to_bytes().to_vec(),
            writable_indexes: l.writable_indexes.to_vec(),
            readonly_indexes: l.readonly_indexes.to_vec(),
        })
        .collect();
    let config = tx.v1_config().map(|c| TransactionConfig {
        priority_fee: c.priority_fee_lamports,
        compute_unit_limit: c.compute_unit_limit,
        loaded_accounts_data_size_limit: c.loaded_accounts_data_size_limit,
        heap_size: c.heap_size,
    });
    Ok(Transaction {
        signatures: (0..tx.num_signatures())
            .filter_map(|i| tx.signature(i).map(|s| s.to_vec()))
            .collect(),
        message: Some(Message {
            header: Some(MessageHeader {
                num_required_signatures: h.num_required_signatures as u32,
                num_readonly_signed_accounts: h.num_readonly_signed_accounts as u32,
                num_readonly_unsigned_accounts: h.num_readonly_unsigned_accounts as u32,
            }),
            account_keys: tx
                .static_account_keys()
                .iter()
                .map(|k| k.to_bytes().to_vec())
                .collect(),
            recent_blockhash: tx.recent_blockhash().to_vec(),
            instructions,
            versioned: tx.version() != TxVersion::Legacy,
            address_table_lookups,
            config,
        }),
    })
}

fn key32(b: &[u8]) -> Option<[u8; 32]> {
    b.try_into().ok()
}

/// The [`crate::wiremeta`] fields of a transaction update.
pub fn meta_in(meta: Option<&TransactionStatusMeta>, index: Option<u32>) -> MetaIn {
    let Some(m) = meta else {
        return MetaIn {
            tx_index: index,
            ..Default::default()
        };
    };
    let inner = if m.inner_instructions_none {
        None
    } else {
        let mut v = vec![];
        for g in &m.inner_instructions {
            let outer = g.index.min(255) as u8;
            for ix in &g.instructions {
                v.push(InnerOwned {
                    outer_index: outer,
                    program_id_index: ix.program_id_index.min(255) as u8,
                    accounts: ix.accounts.clone(),
                    data: ix.data.clone(),
                });
            }
        }
        Some(v)
    };
    MetaIn {
        tx_index: index,
        failed: m.err.is_some(),
        fee: m.fee,
        loaded_writable: m
            .loaded_writable_addresses
            .iter()
            .filter_map(|k| key32(k))
            .collect(),
        loaded_readonly: m
            .loaded_readonly_addresses
            .iter()
            .filter_map(|k| key32(k))
            .collect(),
        inner,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_u16_encoding() {
        for (v, want) in [
            (0usize, vec![0u8]),
            (127, vec![0x7f]),
            (128, vec![0x80, 0x01]),
            (300, vec![0xac, 0x02]),
            (16_384, vec![0x80, 0x80, 0x01]),
        ] {
            let mut o = vec![];
            put_compact(&mut o, v);
            assert_eq!(o, want, "{v}");
        }
    }

    #[test]
    fn malformed_protobuf_is_refused() {
        assert!(tx_to_wire(&Transaction::default()).is_err());
        let mut t = Transaction {
            signatures: vec![vec![1; 64]],
            message: Some(Message {
                header: Some(MessageHeader {
                    num_required_signatures: 1,
                    num_readonly_signed_accounts: 0,
                    num_readonly_unsigned_accounts: 0,
                }),
                account_keys: vec![vec![2; 32]],
                recent_blockhash: vec![3; 31],
                ..Default::default()
            }),
        };
        assert!(tx_to_wire(&t).is_err(), "short blockhash");
        t.message.as_mut().unwrap().recent_blockhash = vec![3; 32];
        let w = tx_to_wire(&t).unwrap();
        assert_eq!(w[0], 1);
        assert_eq!(wire_to_tx(&w).unwrap(), t);
    }
}
