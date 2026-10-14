//! # rugsnipe (binaries and their shared wiring)
//!
//! | module | what |
//! |---|---|
//! | [`app`] | process assembly shared by the hot and replay binaries (safety gate, wallet, ArmSet, threads) |
//! | [`hot`] | `HotLoop`: the docs/06 section 7.1 sequence (dedup, CreateHit, class filters, watchlist, RiskGate, V0, nonce, patch, sign, send, record) plus the paper position/exit engine driver; run to completion, no heap after warm-up |
//! | [`sender`] | sender thread: secondary variants (re-priced, re-signed) through the gated `SenderSet` |
//! | [`cold`] | cold thread (tokio current-thread runtime): ledger -> `state.db`, kill file + SIGUSR1 latch, exits lease, simulated nonce keeper, funding watcher updates, ArmSet reload, paper E2 fill lane, Prometheus metrics |
//! | [`e2`] | paper fill model E2 (end of slot+1 state) from `getSignaturesForAddress` + `getTransaction` |
//! | [`setup`] | config, exit policy, ArmSet verification, templates, nonce pool, throwaway paper signer |
//! | [`convert`] | `CreateHitLite` -> `CreateHit`, feed `TradeLite` -> exit `Trade` |
//! | [`stats`] | Prometheus counters/gauges (pre-created, so the hot thread only does atomic adds) |
//! | [`guard`] | `rugsnipe-guard` logic: watch the exits lease, take over when stale with open positions |
//! | [`alloc_count`] | counting `#[global_allocator]` for the replay's zero-allocation check |
//!
//! Thread layout of `rugsnipe-hot`: feed adapter thread(s) (LogsWs, its own
//! tokio runtime) -> **hot loop** (pinned when `core_affinity` can) -> sender
//! thread; the cold thread owns everything that may block or allocate.

pub mod alloc_count;
pub mod app;
pub mod cli;
pub mod cold;
pub mod convert;
pub mod e2;
pub mod guard;
pub mod hot;
pub mod msgs;
pub mod sender;
pub mod setup;
pub mod stats;

/// Wall-clock milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Wall-clock seconds since the Unix epoch.
pub fn now_unix() -> u64 {
    now_ms() / 1_000
}

/// The binary's version (compared to an ArmSet's `min_hot_version`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
