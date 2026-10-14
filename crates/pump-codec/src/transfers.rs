//! SPL Token / Token-2022 `Transfer` and `TransferChecked` decoding.
//!
//! pump emits no event for token movements, so the "consolidate before the
//! dump" exit trigger (docs/06 section 8, trigger 2) watches these
//! instructions. Both token programs share the instruction tags:
//!
//! | tag | instruction | data | accounts |
//! |---|---|---|---|
//! | 3 | `Transfer` | `u64 amount` | source, destination, authority, [multisig signers] |
//! | 12 | `TransferChecked` | `u64 amount, u8 decimals` | source, mint, destination, authority, [signers] |
//!
//! Plain `Transfer` does not name the mint (Token-2022 rejects it for mints
//! with some extensions but pump mints have none), so a consumer keys it on
//! the token accounts it already tracks. Inner (CPI) transfers come from
//! transaction meta and are decoded the same way.

use crate::constants::{SPL_TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID};
use crate::events::InnerInstruction;
use crate::instructions::AccountLookup;
use crate::tx::TxView;
use solana_pubkey::Pubkey;

/// Which token program executed the transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenProgram {
    Spl,
    Token2022,
}

impl TokenProgram {
    pub fn from_program(program_id: &Pubkey) -> Option<Self> {
        if *program_id == SPL_TOKEN_PROGRAM_ID {
            Some(Self::Spl)
        } else if *program_id == TOKEN_2022_PROGRAM_ID {
            Some(Self::Token2022)
        } else {
            None
        }
    }
    pub fn program_id(self) -> Pubkey {
        match self {
            Self::Spl => SPL_TOKEN_PROGRAM_ID,
            Self::Token2022 => TOKEN_2022_PROGRAM_ID,
        }
    }
}

/// A decoded token transfer. Accounts are `None` when they live in an
/// unresolved address lookup table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenTransfer {
    pub program: TokenProgram,
    /// `true` for `TransferChecked` (carries mint and decimals).
    pub checked: bool,
    pub source: Option<Pubkey>,
    pub destination: Option<Pubkey>,
    /// Authority: the source account's owner or delegate.
    pub owner: Option<Pubkey>,
    pub amount: u64,
    /// `TransferChecked` only.
    pub mint: Option<Pubkey>,
    /// `TransferChecked` only.
    pub decimals: Option<u8>,
    /// Top-level instruction index (the CPI's parent for inner transfers).
    pub outer_index: u8,
    /// Position in the parent's inner-instruction list; `None` at top level.
    pub inner_index: Option<u16>,
}

/// Decode one instruction as a token transfer (`None` if it is not one).
pub fn decode_token_transfer<A: AccountLookup + ?Sized>(
    program_id: &Pubkey,
    data: &[u8],
    accounts: &A,
) -> Option<TokenTransfer> {
    let program = TokenProgram::from_program(program_id)?;
    let tag = *data.first()?;
    let amount = u64::from_le_bytes(data.get(1..9)?.try_into().ok()?);
    let a = |i: usize| accounts.account(i);
    let base = TokenTransfer {
        program,
        checked: false,
        source: None,
        destination: None,
        owner: None,
        amount,
        mint: None,
        decimals: None,
        outer_index: 0,
        inner_index: None,
    };
    match tag {
        3 if data.len() == 9 && accounts.account_count() >= 3 => Some(TokenTransfer {
            source: a(0),
            destination: a(1),
            owner: a(2),
            ..base
        }),
        12 if data.len() == 10 && accounts.account_count() >= 4 => Some(TokenTransfer {
            checked: true,
            source: a(0),
            mint: a(1),
            destination: a(2),
            owner: a(3),
            decimals: Some(data[9]),
            ..base
        }),
        _ => None,
    }
}

/// Every token transfer in a transaction: top-level instructions, then the
/// inner instructions from meta (`inner` may be empty, e.g. for shreds).
/// `loaded` resolves v0 lookup-table entries (`meta.loadedAddresses`).
pub fn transfers(
    tx: &TxView<'_>,
    loaded: Option<&[Pubkey]>,
    inner: &[InnerInstruction<'_>],
) -> Vec<TokenTransfer> {
    let mut out = Vec::new();
    for (i, ix) in tx.instructions().enumerate() {
        let accounts = tx.instruction_accounts(&ix, loaded);
        if let Some(mut t) = decode_token_transfer(tx.program_id(&ix), ix.data, &accounts) {
            t.outer_index = i as u8;
            out.push(t);
        }
    }
    let mut last_outer = None;
    let mut pos = 0u16;
    for ix in inner {
        if last_outer != Some(ix.outer_index) {
            last_outer = Some(ix.outer_index);
            pos = 0;
        } else {
            pos = pos.saturating_add(1);
        }
        let Some(program) = tx.account_key(ix.program_id_index as usize, loaded) else {
            continue;
        };
        let accounts = tx.indexed_accounts(ix.accounts, loaded);
        if let Some(mut t) = decode_token_transfer(&program, ix.data, &accounts) {
            t.outer_index = ix.outer_index;
            t.inner_index = Some(pos);
            out.push(t);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_both_shapes() {
        let k: Vec<Pubkey> = (0..5).map(|_| Pubkey::new_unique()).collect();
        let mut d = vec![3u8];
        d.extend_from_slice(&42u64.to_le_bytes());
        let t = decode_token_transfer(&TOKEN_2022_PROGRAM_ID, &d, k.as_slice()).unwrap();
        assert_eq!(
            (
                t.source,
                t.destination,
                t.owner,
                t.amount,
                t.mint,
                t.checked
            ),
            (Some(k[0]), Some(k[1]), Some(k[2]), 42, None, false)
        );
        let mut d = vec![12u8];
        d.extend_from_slice(&7u64.to_le_bytes());
        d.push(6);
        let t = decode_token_transfer(&SPL_TOKEN_PROGRAM_ID, &d, k.as_slice()).unwrap();
        assert_eq!(t.program, TokenProgram::Spl);
        assert_eq!(
            (t.source, t.mint, t.destination, t.owner, t.decimals),
            (Some(k[0]), Some(k[1]), Some(k[2]), Some(k[3]), Some(6))
        );
        // wrong program, short data, too few accounts, other tags
        assert!(decode_token_transfer(&Pubkey::new_unique(), &d, k.as_slice()).is_none());
        assert!(decode_token_transfer(&SPL_TOKEN_PROGRAM_ID, &d[..9], k.as_slice()).is_none());
        assert!(decode_token_transfer(&SPL_TOKEN_PROGRAM_ID, &d, &k[..3]).is_none());
        let mut burn = vec![8u8];
        burn.extend_from_slice(&1u64.to_le_bytes());
        assert!(decode_token_transfer(&SPL_TOKEN_PROGRAM_ID, &burn, k.as_slice()).is_none());
    }
}
