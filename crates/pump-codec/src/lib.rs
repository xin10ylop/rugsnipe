//! # pump-codec
//!
//! Pure-byte codec for the pump.fun bonding-curve program
//! (`6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P`) and PumpSwap
//! (`pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA`), state of October 2026.
//! Every other `rugsnipe` crate depends on it. No network, no async, no
//! Anchor: decoders borrow from the input and never panic on malformed data.
//!
//! | module | what |
//! |---|---|
//! | [`constants`] | program IDs, fixed PDAs, fee/buyback/reserved recipients, Jito tip accounts, instruction/event/account discriminators |
//! | [`pda`] | bonding curve (+v2), creator vault, volume accumulators, mayhem, pool authority, canonical PumpSwap pool, fee sharing config, ATAs |
//! | [`curve`] | `Global` / `BondingCurve` / `FeeConfig` decoders and the curve's integer math, exactly as the program rounds |
//! | [`events`] | `CreateEvent`, `TradeEvent`, `CompleteEvent`, migration/post-complete events, PumpSwap `BuyEvent`/`SellEvent`/`CreatePoolEvent`, from CPI data or `Program data:` logs |
//! | [`instructions`] | classify pump / PumpSwap instructions (program ID first, shared discriminators); build `buy_v3`/`sell_v3`/legacy trades, ATA, compute budget, tip, nonce |
//! | [`tx`] | zero-copy [`tx::TxView`] over raw legacy, v0 and v1 (SIMD-0385) transactions |
//! | [`create_hit`] | `CreateHit` from wire bytes or `logsSubscribe` logs: every in-tx buy, credited owners, launcher |
//! | [`launchers`] | known launcher programs that create by CPI |
//! | [`transfers`] | SPL / Token-2022 `Transfer` and `TransferChecked` |
//! | [`alpenglow`] | `BlockComponent` parser: entry batches, block markers, the 8-byte abort |
//!
//! ## Example: find a create in raw transaction bytes
//! ```
//! use pump_codec::{instructions::{classify, Classified, PumpInstruction}, tx::TxView};
//! # fn demo(raw: &[u8]) -> Result<(), pump_codec::DecodeError> {
//! let tx = TxView::parse(raw)?;
//! for ix in tx.instructions() {
//!     let accounts = tx.instruction_accounts(&ix, None);
//!     if let Classified::Pump(PumpInstruction::Create(c)) = classify(tx.program_id(&ix), ix.data, &accounts)? {
//!         println!("{} {:?} by {:?}", c.args.symbol, c.mint, c.user);
//!     }
//! }
//! # Ok(()) }
//! ```
//!
//! Verification: discriminators are recomputed with SHA-256, PDAs checked
//! against known addresses and the pump API, curve math against real
//! `TradeEvent`s, builders against the official `pump-rust-client` 0.4.1 and
//! the Solana interface crates, and the transaction parser against
//! `agave-transaction-view` 6.1 and RPC `encoding: "json"` output for real
//! legacy, v0 and v1 transactions (see `fixtures/README.md`).

#![forbid(unsafe_op_in_unsafe_fn)]
#![warn(missing_debug_implementations)]

pub mod alpenglow;
mod bytes;
pub mod constants;
pub mod create_hit;
pub mod curve;
pub mod error;
pub mod events;
pub mod instructions;
pub mod launchers;
pub mod pda;
pub mod transfers;
pub mod tx;

pub use bytes::discriminator;
pub use create_hit::{create_hit, from_logs, CreateHit, HitError};
pub use error::{DecodeError, Result};
pub use solana_instruction::{AccountMeta, Instruction};
pub use solana_pubkey::Pubkey;
