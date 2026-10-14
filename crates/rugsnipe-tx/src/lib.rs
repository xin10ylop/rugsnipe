//! # rugsnipe-tx
//!
//! Everything between a decision and bytes on the wire (docs/06 sections
//! 7.1 steps 10-14, 7.3, 8):
//!
//! | module | what |
//! |---|---|
//! | [`msg`] | fixed-order message compiler (legacy and v1 / SIMD-0385) that records byte offsets |
//! | [`template`] | `BuyTemplate` (nonce, compute budget or v1 config, T22 ATA create, `buy_exact_quote_in_v3`, tip), `SellTemplate` (`sell_v3` [+ `CloseAccount`]), `AmmSellTemplate` (PumpSwap canonical pool `sell_v2`), each with a [`template::PatchTable`] |
//! | [`nonce`] | `NoncePool`: packed `AtomicU64` per slot, `xxh3(mint) % N`, no probing, pre-create value only; next value = `sha256("DURABLE_NONCE" ‖ prev_blockhash)` |
//! | [`signer`] | `HotSigner`: keypair bytes in zeroizing memory, never printed |
//! | [`senders`] | `trait Sender`, `Paper` (records only), `RpcSend` (`rpc`), HTTP senders (`senders`), `Breaker`, the gated `SenderSet` |
//! | [`sysix`] | durable-nonce account create / withdraw instructions (for `ctl nonce init/close`) |
//!
//! Safety rules (tested): paper is the default and needs no keys; the paper
//! sender never opens a socket; real senders are compiled only with a
//! feature **and** constructed only when `Config::validate` passes for probe
//! or live (signed `gate_passed` ArmSet, every cap set, ...).

pub mod msg;
pub mod nonce;
pub mod senders;
pub mod signer;
pub mod sysix;
pub mod template;

#[cfg(feature = "rpc")]
pub mod rpc;

pub use pump_codec::Pubkey;

/// Largest transaction any variant may have (Astralane/NextBlock QUIC and
/// Nozomi bundles cap at 1,232 bytes even for v1).
pub const MAX_TX_LEN: usize = 1_232;
