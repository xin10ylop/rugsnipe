//! # rugsnipe (binaries and their shared wiring)
//!
//! | module | what |
//! |---|---|
//! | [`app`] | process assembly shared by the hot and replay binaries (safety gate, wallet, ArmSet, threads) |
//! | [`hot`] | `HotLoop`: the docs/06 section 7.1 sequence (dedup, CreateHit, class filters, watchlist, RiskGate, V0, nonce, patch, sign, send, record) plus the paper position/exit engine driver; run to completion, no heap after warm-up |
//! | [`sender`] | sender thread: secondary variants (re-priced, re-signed) through the gated `SenderSet`; in probe one variant per real sender (its tip account and minimum tip), each on a warm worker |
//! | [`keys`] | probe key loading from `$CREDENTIALS_DIRECTORY` (probe wallet, `api-keys`), with the permission refusals; named secrets (gRPC x-token, Telegram) |
//! | [`leader`] | leader schedule per epoch + `getClusterNodes` classes (Jito-Agave, BAM, Harmonic, Firedancer, other), a lock-free per-slot snapshot for the hot loop |
//! | [`triggers`] | the kill triggers of docs/06 section 13 as pure evaluators (send errors, unexplained drop, upgrade, disk, clock, drift, feed stale, marked-to-market daily loss) |
//! | [`alerts`] | optional Telegram alerts (start, kill, hourly) and `/kill` polling, off the hot thread |
//! | [`deadman`] | dead-man check (no real buys while launches keep flowing): a 15-minute ring of per-minute create/fire deltas from the hot thread's counters, pure; the cold thread ticks it every minute and alerts at most hourly |
//! | [`real`] | the real fill lane (probe): signature statuses, our own TradeEvent, `meta.fee` of failed buys, landing rank and premium, drops after the nonce burn, sells and balances |
//! | [`cold`] | cold thread (tokio current-thread runtime): ledger -> `state.db`, kill file + SIGUSR1 latch, exits lease, simulated nonce keeper, funding watcher updates, ArmSet reload, paper E2 fill lane, Prometheus metrics |
//! | [`e1`] | paper fill model E1 (rank 1 behind the bundle, docs/04): creation-slot reconstruction, verdict, replay tape |
//! | [`e2`] | paper fill model E2 (end of slot+1 state) from `getSignaturesForAddress` + `getTransaction` |
//! | [`filter`] | the LEAD entry filter: the frozen P(take-profit) trees (`entry.filter_model`), slot-0 feature extraction, the live per-signer outcome store, the create-rate ring |
//! | [`setup`] | config, exit policy, ArmSet verification, templates, nonce pool, throwaway paper signer |
//! | [`convert`] | `CreateHitLite` -> `CreateHit`, feed `TradeLite` -> exit `Trade` |
//! | [`stats`] | Prometheus counters/gauges (pre-created, so the hot thread only does atomic adds) |
//! | [`guard`] | `rugsnipe-guard` logic: watch the exits lease, take over when stale with open positions |
//! | [`inflight`] | shutdown: real sells already sent are resolved on chain and booked before the ledger closes |
//! | [`alloc_count`] | counting `#[global_allocator]` for the replay's zero-allocation check |
//!
//! Thread layout of `rugsnipe-hot`: feed adapter thread(s) (LogsWs, its own
//! tokio runtime) -> **hot loop** (pinned when `core_affinity` can) -> sender
//! thread; the cold thread owns everything that may block or allocate.

pub mod alerts;
pub mod alloc_count;
pub mod app;
pub mod cli;
pub mod cold;
pub mod convert;
pub mod deadman;
pub mod dumpers;
pub mod e1;
pub mod e2;
pub mod filter;
pub mod firstseen;
pub mod guard;
pub mod hot;
pub mod inflight;
pub mod keys;
pub mod leader;
pub mod msgs;
pub mod operators;
pub mod real;
pub mod sender;
pub mod setup;
pub mod stats;
pub mod triggers;
pub mod tweet;
pub mod tweetfeed;

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
