//! # rugsnipe-state
//!
//! The hot box's durable state (docs/06 sections 5.1, 7.3, 8, 12, 13):
//!
//! | module | what |
//! |---|---|
//! | [`schema`] | the `state.db` tables of docs/06 section 5.1 (SQLite, WAL, integer lamports) |
//! | [`store`] | `StateStore`: one writer thread fed by a channel, batched transactions; read connections for recovery and reports |
//! | [`ledger`] | `Ledger`: typed writes for intent -> send -> fill -> position -> exit, with the intent state machine enforced in SQL |
//! | [`recovery`] | `Recovery`: on start, load open positions and nonce states, reconcile them against the chain ([`chain::ChainRead`]), mark orphans |
//! | [`lease`] | the `exits` lease (heartbeat, compare-and-set takeover with `epoch + 1`) shared by hot and guard |
//! | [`budget`] | `RpcBudget`: per-IP, per-method token bucket sized from `[rpc_budget]` |
//! | [`chain`] | `ChainRead` trait; `RpcChain` (feature `rpc`) over public JSON-RPC |
//!
//! Nothing here runs on the hot thread: the hot loop pushes `Copy`
//! messages into a ring and the cold thread turns them into [`store::Write`]s.

pub mod budget;
pub mod chain;
pub mod lease;
pub mod ledger;
pub mod recovery;
pub mod rows;
pub mod schema;
pub mod store;

pub use budget::RpcBudget;
pub use chain::ChainRead;
pub use lease::{Lease, LeaseRow};
pub use ledger::Ledger;
pub use recovery::{recover, RecoveryReport};
pub use rows::*;
pub use store::{StateStore, StateWriter, Write};

/// Errors of this crate (SQLite or a chain read).
#[derive(Debug)]
pub enum StateError {
    Sql(rusqlite::Error),
    Chain(String),
    Io(std::io::Error),
}

impl core::fmt::Display for StateError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Sql(e) => write!(f, "sqlite: {e}"),
            Self::Chain(e) => write!(f, "chain read: {e}"),
            Self::Io(e) => write!(f, "io: {e}"),
        }
    }
}
impl std::error::Error for StateError {}
impl From<rusqlite::Error> for StateError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e)
    }
}
impl From<std::io::Error> for StateError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, StateError>;
