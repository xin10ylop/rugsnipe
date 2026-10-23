//! [`Grpc`]: the Yellowstone gRPC ("Dragon's Mouth") post-execution feed
//! (feature `grpc`; Triton, Alchemy, Helius LaserStream, SVS and the other
//! standard Yellowstone endpoints of docs/research/infra-shopping-2026.md
//! section 1).
//!
//! One `Subscribe` stream at `processed` commitment with these filters:
//!
//! | filter | what | becomes |
//! |---|---|---|
//! | `creates` | transactions that mention the pump mint authority `TSLvdd…` (every create), successful only | [`Payload::Wire`] with the re-serialized transaction bytes and a [`crate::wiremeta`] trailer (index, fee, ALT-loaded addresses, inner instructions): `pump_codec::create_hit` runs on the hot thread |
//! | `wallet` | transactions that mention our wallet (failed ones too) | [`Payload::Trade`] for each of our own pump TradeEvents (the real fill lane's own trade) |
//! | `mints` | transactions that mention a held mint (watch / unwatch at runtime) | [`Payload::Trade`] for every TradeEvent of a held mint (exit engine) |
//! | `pump` | (`bundle_run`) every pump program transaction | the creation slot's trades of each new mint, assembled into a [`BundleRun`] for the cold thread |
//! | `slots` | slot updates | `last_slot` (drift and stale detection) |
//!
//! * `x-token` header from the credential named by `[feeds] grpc.x_token`;
//!   compression off (latency); TCP_NODELAY, HTTP/2 keep-alive;
//! * the server's pings are answered (load balancers drop silent streams);
//! * stale detection: no data message (a transaction or a slot update; a
//!   healthy stream sends a slot every ~400 ms) for `stale_ms` forces a
//!   reconnect (pings and pongs do not count: a stuck server still answers
//!   them); reconnects use jittered exponential
//!   backoff and re-send the current filters;
//! * dedup semantics of [`crate::FeedMux`]: a wire event's key is its first
//!   signature, a trade's key its signature and TradeEvent ordinal (the same
//!   ordinal the logs path gives, so a trade seen on both feeds is one);
//! * ordering: events carry the slot, the bank id and (in the trailer) the
//!   transaction index; arrivals out of (slot, index) order are counted in
//!   `reordered`, and bundle runs are sorted by index.
//!
//! The adapter thread decodes protobuf (it allocates); the hot loop only
//! pops the two rings ([`crate::ring`] for trades, [`crate::wirering`] for
//! wire bytes): lock-free and allocation-free.

pub mod convert;
#[cfg(feature = "grpc-mock")]
pub mod mock;

use crate::buf::FeedBuf;
use crate::bundle::{BundleAssembler, BundleRun, RunTrade};
use crate::clock::mono_ns;
use crate::decode::decode_logs;
use crate::event::{FeedEvent, FeedSrc, Payload, RunDoneLite, Sig, TradeLite};
use crate::health::{log_gate, miss_line, FeedHealth, MissInfo, MissKind};
use crate::ring::{ring, RingFeed, RingSink};
use crate::wiremeta;
use crate::wirering::{wire_ring, WireSink, WireSource};
use crate::{Feed, FeedStatus};
use futures_util::{SinkExt, StreamExt};
use pump_codec::constants::{MINT_AUTHORITY, PUMP_PROGRAM_ID};
use pump_codec::curve::{CurveState, FeeBps};
use pump_codec::events::{events_from_inner_instructions, InnerInstruction, PumpEvent};
use pump_codec::instructions::TradeAmounts;
use pump_codec::tx::{TxVersion, TxView};
use pump_codec::{HitError, Pubkey};
use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use yellowstone_grpc_client::{ClientTlsConfig, GeyserGrpcClient};
use yellowstone_grpc_proto::prelude::{
    subscribe_update::UpdateOneof, subscribe_update_deshred::UpdateOneof as DeshredOneof,
    CommitmentLevel, SubscribeDeshredRequest, SubscribeRequest,
    SubscribeRequestFilterDeshredTransactions, SubscribeRequestFilterSlots,
    SubscribeRequestFilterTransactions, SubscribeRequestPing, SubscribeUpdate,
    SubscribeUpdateDeshred, SubscribeUpdateDeshredTransaction, SubscribeUpdateTransaction,
};

/// The deshred stream's filter name.
pub const F_DESHRED: &str = "deshred";

/// Filter names (the server echoes them in `SubscribeUpdate.filters`).
pub const F_CREATES: &str = "creates";
pub const F_WALLET: &str = "wallet";
pub const F_MINTS: &str = "mints";
pub const F_PUMP: &str = "pump";
pub const F_SLOTS: &str = "slots";

/// Settings (`[feeds] grpc = { enabled, url, x_token, stale_ms, bundle_run }`).
#[derive(Clone, PartialEq)]
pub struct GrpcConfig {
    /// The source every event of this adapter carries ([`FeedSrc::Grpc`],
    /// or [`FeedSrc::GrpcB`] for the comparison feed).
    pub src: FeedSrc,
    /// `https://host[:port]` (TLS, native roots) or `http://` (plain h2c: local tests).
    pub url: String,
    /// The `x-token` value (the credential's content; never logged).
    pub x_token: Option<String>,
    pub stale_ms: u64,
    pub connect_timeout_ms: u64,
    pub min_backoff_ms: u64,
    pub max_backoff_ms: u64,
    /// Trade-event ring (events) and wire ring (bytes) to the hot loop.
    pub ring_capacity: usize,
    pub wire_ring_bytes: usize,
    /// The mention that marks a create (the pump mint authority).
    pub mention: Pubkey,
    /// Our wallet (`wallet` filter); `None` = no own-trade filter.
    pub wallet: Option<Pubkey>,
    pub watch_mints: Vec<Pubkey>,
    pub max_watches: usize,
    /// Also stream every pump transaction to assemble the creation slot's
    /// bundle run ([`BundleRun`], to the cold thread).
    pub bundle_run: bool,
    pub bundle_timeout_ms: u64,
    /// Subscribe to the pre-execution stream (`SubscribeDeshred`: transactions
    /// decoded from shreds before the block is replayed) instead of the
    /// post-execution `Subscribe`. Every pump transaction arrives with no
    /// meta; creates go to the hot loop as wire frames with an arrival-order
    /// index, and the create slot's buys are quoted on our own curve so the
    /// bundle gate sees the run before the chain executes it.
    pub deshred: bool,
    /// Fee schedule for the pre-execution quotes.
    pub fees: FeeBps,
    /// Client pings when the server is quiet (`<= stale_ms / 3`).
    pub ping_ms: u64,
}

impl std::fmt::Debug for GrpcConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GrpcConfig")
            .field("url", &self.url)
            .field("x_token", &self.x_token.as_ref().map(|_| "<set>"))
            .field("stale_ms", &self.stale_ms)
            .field("wallet", &self.wallet)
            .field("watch_mints", &self.watch_mints.len())
            .field("bundle_run", &self.bundle_run)
            .finish_non_exhaustive()
    }
}

impl Default for GrpcConfig {
    fn default() -> Self {
        GrpcConfig {
            src: FeedSrc::Grpc,
            url: String::new(),
            x_token: None,
            stale_ms: 2_000,
            connect_timeout_ms: 10_000,
            min_backoff_ms: 250,
            max_backoff_ms: 30_000,
            ring_capacity: 8_192,
            wire_ring_bytes: 8 << 20,
            mention: MINT_AUTHORITY,
            wallet: None,
            watch_mints: Vec::new(),
            max_watches: 64,
            bundle_run: false,
            bundle_timeout_ms: 2_000,
            ping_ms: 5_000,
            deshred: false,
            fees: FeeBps::DEFAULT,
        }
    }
}

/// Control messages to the adapter thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    WatchMint(Pubkey),
    UnwatchMint(Pubkey),
}

/// Counters beyond [`FeedHealth`] (adapter thread writes, anyone reads).
#[derive(Debug, Default)]
pub struct GrpcStats {
    pub transactions: AtomicU64,
    pub wire_events: AtomicU64,
    pub trade_events: AtomicU64,
    /// Bundle-run completions reported to the hot loop ([`RunDoneLite`]).
    pub run_done_events: AtomicU64,
    pub slot_updates: AtomicU64,
    pub pings: AtomicU64,
    pub reordered: AtomicU64,
    pub convert_errors: AtomicU64,
    pub bundle_runs: AtomicU64,
    pub bundle_drops: AtomicU64,
    pub resubscribes: AtomicU64,
    /// Highest slot of any update (slot or transaction).
    pub slot: AtomicU64,
}

/// The adapter (a [`Feed`]): two rings from the adapter thread.
#[derive(Debug)]
pub struct Grpc {
    src: FeedSrc,
    events: RingFeed,
    wire: WireSource,
    ctl: mpsc::UnboundedSender<Control>,
    stats: Arc<GrpcStats>,
    bundles: Option<std::sync::mpsc::Receiver<BundleRun>>,
}

/// Cloneable control handle (watch / unwatch held mints from the cold
/// thread after the adapter moved into a [`crate::FeedMux`]).
#[derive(Debug, Clone)]
pub struct GrpcHandle {
    ctl: mpsc::UnboundedSender<Control>,
    health: Arc<FeedHealth>,
    stats: Arc<GrpcStats>,
}

impl GrpcHandle {
    pub fn watch_mint(&self, k: Pubkey) -> bool {
        self.ctl.send(Control::WatchMint(k)).is_ok()
    }
    pub fn unwatch_mint(&self, k: Pubkey) -> bool {
        self.ctl.send(Control::UnwatchMint(k)).is_ok()
    }
    pub fn health(&self) -> &Arc<FeedHealth> {
        &self.health
    }
    pub fn stats(&self) -> &Arc<GrpcStats> {
        &self.stats
    }
}

impl Grpc {
    /// Start the adapter thread (its own current-thread tokio runtime).
    pub fn spawn(cfg: GrpcConfig) -> io::Result<Grpc> {
        if !(cfg.url.starts_with("https://") || cfg.url.starts_with("http://")) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "grpc url must be http(s)://",
            ));
        }
        let src = cfg.src;
        let (sink, mut events, stop) = ring(src, cfg.ring_capacity, cfg.stale_ms);
        let (wsink, wire) = wire_ring(src, cfg.wire_ring_bytes, sink.health().clone());
        let (ctl, ctl_rx) = mpsc::unbounded_channel();
        let stats = Arc::new(GrpcStats::default());
        let (btx, brx) = std::sync::mpsc::sync_channel::<BundleRun>(1_024);
        let st = stats.clone();
        let bundle = cfg.bundle_run;
        let h = std::thread::Builder::new()
            .name(format!("feed-{}", src.name()))
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(_) => return,
                };
                let mut a = Adapter::new(cfg, sink, wsink, st, btx);
                rt.block_on(a.run(stop, ctl_rx));
            })?;
        events.set_thread(h);
        Ok(Grpc {
            src,
            events,
            wire,
            ctl,
            stats,
            bundles: bundle.then_some(brx),
        })
    }

    pub fn handle(&self) -> GrpcHandle {
        GrpcHandle {
            ctl: self.ctl.clone(),
            health: self.events.health_arc().clone(),
            stats: self.stats.clone(),
        }
    }

    pub fn health_arc(&self) -> &Arc<FeedHealth> {
        self.events.health_arc()
    }

    pub fn stats(&self) -> &Arc<GrpcStats> {
        &self.stats
    }

    /// The bundle-run receiver (with `bundle_run`), for the cold thread.
    pub fn take_bundles(&mut self) -> Option<std::sync::mpsc::Receiver<BundleRun>> {
        self.bundles.take()
    }

    /// Stop the adapter thread and wait for it.
    pub fn shutdown(&mut self) {
        self.events.shutdown();
    }
}

impl Feed for Grpc {
    #[inline]
    fn poll(&mut self, out: &mut FeedBuf) -> FeedStatus {
        let w = self.wire.poll_into(out);
        match self.events.poll(out) {
            FeedStatus::Ready(n) => FeedStatus::Ready(n + w),
            st if w > 0 => {
                let _ = st;
                FeedStatus::Ready(w)
            }
            st => st,
        }
    }

    fn src(&self) -> FeedSrc {
        self.src
    }

    fn health(&self) -> Option<&FeedHealth> {
        self.events.health()
    }
}

/// The subscription for the current watch set.
pub fn build_request(cfg: &GrpcConfig, mints: &[Pubkey]) -> SubscribeRequest {
    let mut req = SubscribeRequest {
        commitment: Some(CommitmentLevel::Processed as i32),
        ..Default::default()
    };
    let tx = |include: Vec<String>, failed: bool| SubscribeRequestFilterTransactions {
        vote: Some(false),
        failed: (!failed).then_some(false),
        account_include: include,
        ..Default::default()
    };
    req.transactions.insert(
        F_CREATES.to_string(),
        tx(vec![cfg.mention.to_string()], false),
    );
    if let Some(w) = cfg.wallet {
        // failed transactions too: our failed buys are costs
        req.transactions
            .insert(F_WALLET.to_string(), tx(vec![w.to_string()], true));
    }
    if !mints.is_empty() {
        req.transactions.insert(
            F_MINTS.to_string(),
            tx(mints.iter().map(|m| m.to_string()).collect(), false),
        );
    }
    if cfg.bundle_run {
        req.transactions.insert(
            F_PUMP.to_string(),
            tx(vec![PUMP_PROGRAM_ID.to_string()], false),
        );
    }
    req.slots.insert(
        F_SLOTS.to_string(),
        SubscribeRequestFilterSlots {
            filter_by_commitment: Some(true),
            ..Default::default()
        },
    );
    req
}

fn ping_request(id: i32) -> SubscribeRequest {
    SubscribeRequest {
        ping: Some(SubscribeRequestPing { id }),
        ..Default::default()
    }
}

/// The pre-execution subscription: every pump transaction (no votes) and the
/// slot updates. Pings go in later requests only ([`deshred_ping`]): a ping
/// in the first request makes some servers ignore the filters.
pub fn build_deshred_request(_cfg: &GrpcConfig) -> SubscribeDeshredRequest {
    let mut req = SubscribeDeshredRequest::default();
    req.deshred_transactions.insert(
        F_DESHRED.to_string(),
        SubscribeRequestFilterDeshredTransactions {
            vote: Some(false),
            account_include: vec![PUMP_PROGRAM_ID.to_string()],
            ..Default::default()
        },
    );
    req.slots.insert(
        F_SLOTS.to_string(),
        SubscribeRequestFilterSlots {
            filter_by_commitment: Some(true),
            ..Default::default()
        },
    );
    req
}

/// Quote a buy before execution on our simulated curve and advance the
/// curve by it: the trade the chain is about to execute, as the bundle gate
/// and the exit engine will see it. `None` for a sell, an empty buy or a buy
/// the curve cannot fill.
pub fn pre_exec_buy(
    curve: &mut CurveState,
    amounts: TradeAmounts,
    fees: FeeBps,
) -> Option<pump_codec::curve::BuyQuote> {
    let q = match amounts {
        TradeAmounts::ExactTokensOut { amount, .. } => curve.quote_buy_exact_tokens(amount, fees),
        TradeAmounts::ExactQuoteIn {
            spendable_quote_in, ..
        } => curve.quote_buy_exact_sol_in(spendable_quote_in, fees),
        TradeAmounts::Sell { .. } => None,
    }?;
    if q.tokens == 0 {
        return None;
    }
    curve.apply_buy(q.tokens, q.sol_cost);
    Some(q)
}

fn deshred_ping(id: i32) -> SubscribeDeshredRequest {
    SubscribeDeshredRequest {
        ping: Some(SubscribeRequestPing { id }),
        ..Default::default()
    }
}

/// Jittered exponential backoff (xorshift on the clock).
fn jitter(ms: u64) -> u64 {
    let mut x = mono_ns() | 1;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    ms * (800 + (x % 401)) / 1000
}

async fn sleep_or_stop(stop: &AtomicBool, ms: u64) {
    let mut left = ms;
    while left > 0 && !stop.load(Ordering::Acquire) {
        let d = left.min(100);
        tokio::time::sleep(Duration::from_millis(d)).await;
        left -= d;
    }
}

/// Trades of a transaction from its logs (the logs path's ordinals) or,
/// when the logs were truncated, from its self-CPI events.
pub fn trades_of(
    u: &SubscribeUpdateTransaction,
    keys_full: &[Pubkey],
    out: &mut Vec<TradeLite>,
) -> usize {
    out.clear();
    let Some(info) = u.transaction.as_ref() else {
        return 0;
    };
    let Some(meta) = info.meta.as_ref() else {
        return 0;
    };
    if meta.err.is_some() {
        return 0;
    }
    let sig = Sig(info.signature.as_slice().try_into().unwrap_or([0; 64]));
    let o = decode_logs(u.slot, sig, false, &meta.log_messages, out);
    if !o.truncated && !meta.log_messages_none {
        return out.len();
    }
    // the runtime cut the logs: the CPI events are complete
    out.clear();
    let inner: Vec<InnerInstruction<'_>> = meta
        .inner_instructions
        .iter()
        .flat_map(|g| {
            g.instructions.iter().map(move |ix| InnerInstruction {
                outer_index: g.index.min(255) as u8,
                program_id_index: ix.program_id_index.min(255) as u8,
                accounts: &ix.accounts,
                data: &ix.data,
            })
        })
        .collect();
    if let Ok(evs) = events_from_inner_instructions(keys_full, inner, true) {
        for e in evs {
            if let PumpEvent::Trade(t) = e.event {
                let ordinal = out.len().min(255) as u8;
                out.push(TradeLite {
                    mint: t.mint,
                    user: t.user,
                    is_buy: t.is_buy,
                    sol_amount: t.sol_amount,
                    token_amount: t.token_amount,
                    fee: t
                        .fee
                        .unwrap_or(0)
                        .saturating_add(t.creator_fee.unwrap_or(0)),
                    index: u32::MAX,
                    virtual_sol_reserves: t.virtual_sol_reserves,
                    virtual_token_reserves: t.virtual_token_reserves,
                    slot: u.slot,
                    sig,
                    ordinal,
                });
            }
        }
    }
    out.len()
}

struct Adapter {
    cfg: GrpcConfig,
    sink: RingSink,
    wire: WireSink,
    stats: Arc<GrpcStats>,
    health: Arc<FeedHealth>,
    bundles: std::sync::mpsc::SyncSender<BundleRun>,
    asm: BundleAssembler,
    mints: Vec<Pubkey>,
    mint_set: HashSet<Pubkey>,
    trades: Vec<TradeLite>,
    meta_buf: Vec<u8>,
    last_pos: (u64, u32),
    recent: HashMap<Pubkey, u64>,
    /// Deshred: (slot, next arrival index) of the current slot.
    seq: (u64, u32),
    /// Deshred: our simulated curve per create of the last slots, advanced by
    /// every pre-execution buy we quote.
    pre: HashMap<Pubkey, CurveState>,
    /// Highest slot seen by the pruning of `recent`.
    recent_slot: u64,
    /// A gated decode miss of the current transaction, printed by `on_tx`
    /// after the wire push (never between the decode and the hot ring).
    pending_miss: Option<(MissInfo, HitError)>,
}

impl Adapter {
    fn new(
        cfg: GrpcConfig,
        sink: RingSink,
        wire: WireSink,
        stats: Arc<GrpcStats>,
        bundles: std::sync::mpsc::SyncSender<BundleRun>,
    ) -> Adapter {
        let mut mints = cfg.watch_mints.clone();
        mints.truncate(cfg.max_watches);
        let health = sink.health().clone();
        Adapter {
            asm: BundleAssembler::new(256, cfg.bundle_timeout_ms),
            mint_set: mints.iter().copied().collect(),
            mints,
            cfg,
            sink,
            wire,
            stats,
            health,
            bundles,
            trades: Vec::with_capacity(64),
            meta_buf: Vec::with_capacity(16 * 1024),
            last_pos: (0, 0),
            recent: HashMap::new(),
            seq: (0, 0),
            pre: HashMap::new(),
            recent_slot: 0,
            pending_miss: None,
        }
    }

    /// Apply a watch change; `true` when the filters changed.
    fn apply(&mut self, c: Control) -> bool {
        match c {
            Control::WatchMint(m) => {
                if self.mint_set.contains(&m) || self.mints.len() >= self.cfg.max_watches {
                    return false;
                }
                self.mint_set.insert(m);
                self.mints.push(m);
                true
            }
            Control::UnwatchMint(m) => {
                if !self.mint_set.remove(&m) {
                    return false;
                }
                self.mints.retain(|x| *x != m);
                true
            }
        }
    }

    async fn run(&mut self, stop: Arc<AtomicBool>, mut ctl: mpsc::UnboundedReceiver<Control>) {
        let mut backoff = self.cfg.min_backoff_ms;
        while !stop.load(Ordering::Acquire) {
            let t_conn = mono_ns();
            // pending watch changes apply to the next subscription
            while let Ok(c) = ctl.try_recv() {
                self.apply(c);
            }
            let ended = self.session(&stop, &mut ctl).await;
            self.health.connected.store(false, Ordering::Relaxed);
            if stop.load(Ordering::Acquire) {
                break;
            }
            FeedHealth::inc(&self.health.reconnects);
            let lived_ms = mono_ns().saturating_sub(t_conn) / 1_000_000;
            if lived_ms > 60_000 {
                backoff = self.cfg.min_backoff_ms;
            }
            let wait = jitter(backoff);
            // the reason goes to the journal: a server that refuses the
            // stream (Unauthenticated, Unimplemented, a bad url) would
            // otherwise show only as a climbing reconnect counter
            eprintln!(
                "{} feed {}: session ended after {} ms: {} (reconnect in {} ms)",
                self.health.src.name(),
                self.cfg.url,
                lived_ms,
                match &ended {
                    Ok(()) => "closed".to_string(),
                    Err(e) => e.clone(),
                },
                wait
            );
            sleep_or_stop(&stop, wait).await;
            backoff = (backoff * 2).min(self.cfg.max_backoff_ms);
        }
        // hand over what is still open
        self.asm.on_slot(u64::MAX, mono_ns());
        self.flush_bundles();
    }

    async fn connect(&self) -> Result<GeyserGrpcClient, String> {
        let mut b = GeyserGrpcClient::build_from_shared(self.cfg.url.clone())
            .map_err(|e| e.to_string())?
            .x_token(self.cfg.x_token.clone())
            .map_err(|e| e.to_string())?
            .connect_timeout(Duration::from_millis(self.cfg.connect_timeout_ms))
            .timeout(Duration::from_secs(24 * 3600))
            .tcp_nodelay(true)
            .http2_keep_alive_interval(Duration::from_secs(10))
            .keep_alive_timeout(Duration::from_secs(5))
            .keep_alive_while_idle(true)
            .max_decoding_message_size(64 << 20);
        if self.cfg.url.starts_with("https://") {
            b = b
                .tls_config(ClientTlsConfig::new().with_native_roots())
                .map_err(|e| e.to_string())?;
        }
        let fut = b.connect();
        tokio::time::timeout(Duration::from_millis(self.cfg.connect_timeout_ms), fut)
            .await
            .map_err(|_| "connect timeout".to_string())?
            .map_err(|e| e.to_string())
    }

    async fn session(
        &mut self,
        stop: &AtomicBool,
        ctl: &mut mpsc::UnboundedReceiver<Control>,
    ) -> Result<(), String> {
        if self.cfg.deshred {
            return self.session_deshred(stop, ctl).await;
        }
        let mut client = self.connect().await?;
        let req = build_request(&self.cfg, &self.mints);
        let (mut sink, mut stream) = tokio::time::timeout(
            Duration::from_millis(self.cfg.connect_timeout_ms),
            client.subscribe_with_request(Some(req)),
        )
        .await
        .map_err(|_| "subscribe timeout".to_string())?
        .map_err(|e| e.to_string())?;
        self.health.connected.store(true, Ordering::Relaxed);
        // a fresh stream is not stale until stale_ms passes without messages
        self.health
            .last_frame_ns
            .store(mono_ns(), Ordering::Relaxed);
        let ping_ms = if self.cfg.stale_ms > 0 {
            self.cfg.ping_ms.min(self.cfg.stale_ms / 3)
        } else {
            self.cfg.ping_ms
        }
        .max(200);
        let mut ping = tokio::time::interval(Duration::from_millis(ping_ms));
        ping.tick().await;
        let mut check = tokio::time::interval(Duration::from_millis(50));
        let mut ping_id: i32 = 1;
        while !stop.load(Ordering::Acquire) {
            tokio::select! {
                m = stream.next() => {
                    match m {
                        Some(Ok(u)) => {
                            if self.on_update(u) {
                                ping_id = ping_id.wrapping_add(1);
                                if sink.send(ping_request(ping_id)).await.is_err() {
                                    return Err("ping reply failed".into());
                                }
                            }
                        }
                        Some(Err(e)) => return Err(e.to_string()),
                        None => return Err("stream ended".into()),
                    }
                }
                _ = ping.tick() => {
                    ping_id = ping_id.wrapping_add(1);
                    if sink.send(ping_request(ping_id)).await.is_err() {
                        return Err("ping failed".into());
                    }
                }
                _ = check.tick() => {
                    let now = mono_ns();
                    if self.health.is_stale_at(now) {
                        FeedHealth::inc(&self.health.stale_reconnects);
                        return Err("stale".into());
                    }
                    self.asm.on_tick(now);
                    self.flush_bundles();
                }
                Some(c) = ctl.recv() => {
                    let mut changed = self.apply(c);
                    while let Ok(c) = ctl.try_recv() {
                        changed |= self.apply(c);
                    }
                    if changed {
                        self.stats.resubscribes.fetch_add(1, Ordering::Relaxed);
                        if sink.send(build_request(&self.cfg, &self.mints)).await.is_err() {
                            return Err("resubscribe failed".into());
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// The pre-execution session: `SubscribeDeshred` with the pump program
    /// filter and slot updates; pings keep the stream alive.
    async fn session_deshred(
        &mut self,
        stop: &AtomicBool,
        ctl: &mut mpsc::UnboundedReceiver<Control>,
    ) -> Result<(), String> {
        let mut client = self.connect().await?;
        let req = build_deshred_request(&self.cfg);
        let (mut sink, mut stream) = tokio::time::timeout(
            Duration::from_millis(self.cfg.connect_timeout_ms),
            client.subscribe_deshred_with_request(Some(req)),
        )
        .await
        .map_err(|_| "subscribe_deshred timeout".to_string())?
        .map_err(|e| e.to_string())?;
        self.health.connected.store(true, Ordering::Relaxed);
        self.health
            .last_frame_ns
            .store(mono_ns(), Ordering::Relaxed);
        let ping_ms = if self.cfg.stale_ms > 0 {
            self.cfg.ping_ms.min(self.cfg.stale_ms / 3)
        } else {
            self.cfg.ping_ms
        }
        .max(200);
        let mut ping = tokio::time::interval(Duration::from_millis(ping_ms));
        ping.tick().await;
        let mut check = tokio::time::interval(Duration::from_millis(50));
        let mut ping_id: i32 = 1;
        while !stop.load(Ordering::Acquire) {
            tokio::select! {
                m = stream.next() => {
                    match m {
                        Some(Ok(u)) => {
                            if self.on_deshred_update(u) {
                                ping_id = ping_id.wrapping_add(1);
                                if sink.send(deshred_ping(ping_id)).await.is_err() {
                                    return Err("ping reply failed".into());
                                }
                            }
                        }
                        Some(Err(e)) => return Err(e.to_string()),
                        None => return Err("stream ended".into()),
                    }
                }
                _ = ping.tick() => {
                    ping_id = ping_id.wrapping_add(1);
                    if sink.send(deshred_ping(ping_id)).await.is_err() {
                        return Err("ping failed".into());
                    }
                }
                _ = check.tick() => {
                    let now = mono_ns();
                    if self.health.is_stale_at(now) {
                        FeedHealth::inc(&self.health.stale_reconnects);
                        return Err("stale".into());
                    }
                    self.asm.on_tick(now);
                    self.flush_bundles();
                }
                Some(c) = ctl.recv() => {
                    // watch changes do not change the deshred filter (every
                    // pump transaction is streamed); the state is kept
                    self.apply(c);
                    while let Ok(c) = ctl.try_recv() {
                        self.apply(c);
                    }
                }
            }
        }
        Ok(())
    }

    /// One deshred update; `true` when the server pinged (answer it).
    fn on_deshred_update(&mut self, u: SubscribeUpdateDeshred) -> bool {
        let recv = mono_ns();
        let Some(up) = u.update_oneof else {
            return false;
        };
        match up {
            DeshredOneof::Ping(_) => {
                self.stats.pings.fetch_add(1, Ordering::Relaxed);
                return true;
            }
            DeshredOneof::Pong(_) => {}
            DeshredOneof::Slot(s) => {
                self.health.frame();
                self.stats.slot_updates.fetch_add(1, Ordering::Relaxed);
                self.bump_slot(s.slot);
                self.asm.on_slot(s.slot, recv);
                self.flush_bundles();
            }
            DeshredOneof::DeshredTransaction(t) => {
                self.health.frame();
                self.on_deshred_tx(&t, recv);
            }
            DeshredOneof::DeshredUpdateParent(_) => {}
        }
        false
    }

    /// A pre-execution transaction: the create goes to the hot loop as a
    /// wire frame (no meta, arrival-order index); a buy of a create of the
    /// last slots is quoted on our simulated curve and goes to the hot loop
    /// as the trade the chain is about to execute (same signature and
    /// ordinal as the post-execution copy, so the mux drops the later one).
    fn on_deshred_tx(&mut self, t: &SubscribeUpdateDeshredTransaction, recv: u64) {
        self.stats.transactions.fetch_add(1, Ordering::Relaxed);
        let Some(info) = t.transaction.as_ref() else {
            return;
        };
        if info.is_vote {
            return;
        }
        let Some(tx) = info.transaction.as_ref() else {
            return;
        };
        let slot = t.slot;
        self.bump_slot(slot);
        if slot != self.seq.0 {
            if slot < self.seq.0 {
                self.stats.reordered.fetch_add(1, Ordering::Relaxed);
            }
            self.seq = (slot, 0);
        }
        let index = self.seq.1;
        self.seq.1 = self.seq.1.saturating_add(1);
        let wire = match convert::tx_to_wire(tx) {
            Ok(w) => w,
            Err(_) => {
                self.stats.convert_errors.fetch_add(1, Ordering::Relaxed);
                FeedHealth::inc(&self.health.decode_errors);
                return;
            }
        };
        let mut keys: Vec<Pubkey> = tx
            .message
            .as_ref()
            .map(|m| {
                m.account_keys
                    .iter()
                    .filter_map(|k| <[u8; 32]>::try_from(k.as_slice()).ok())
                    .map(Pubkey::new_from_array)
                    .collect()
            })
            .unwrap_or_default();
        let n_static = keys.len();
        let mut meta = wiremeta::MetaIn {
            tx_index: Some(index),
            ..Default::default()
        };
        for k in &info.loaded_writable_addresses {
            if let Ok(a) = <[u8; 32]>::try_from(k.as_slice()) {
                keys.push(Pubkey::new_from_array(a));
                meta.loaded_writable.push(a);
            }
        }
        for k in &info.loaded_readonly_addresses {
            if let Ok(a) = <[u8; 32]>::try_from(k.as_slice()) {
                keys.push(Pubkey::new_from_array(a));
                meta.loaded_readonly.push(a);
            }
        }
        self.meta_buf.clear();
        wiremeta::encode(&meta, &mut self.meta_buf);
        let sig = Sig(info.signature.as_slice().try_into().unwrap_or([0; 64]));
        let created = self.check_create(&wire, &keys, slot);
        if let Some(mint) = created {
            if self.wire.push(recv, slot, 0, &wire, &self.meta_buf) {
                self.stats.wire_events.fetch_add(1, Ordering::Relaxed);
                self.health.event(slot);
            }
            if let Some((info, err)) = self.pending_miss.take() {
                eprintln!("{}", miss_line(self.health.src.name(), &info, &err));
            }
            // the create's own buys and the curve after them, from the
            // instruction arguments (no execution yet)
            let mut own: Vec<RunTrade> = Vec::new();
            if let Ok(view) = TxView::parse(&wire) {
                let loaded = (keys.len() > n_static).then(|| &keys[n_static..]);
                if let Ok(Some(h)) = pump_codec::create_hit(&view, loaded, &[], self.cfg.fees) {
                    if let Some(c) = h.curve_after {
                        if h.intx_sol_in > 0 {
                            own.push(RunTrade {
                                index,
                                is_create: true,
                                user: h.user,
                                is_buy: true,
                                sol: h.intx_sol_in,
                                tok: h.intx_tokens_out,
                                v: c.virtual_sol,
                                t: c.virtual_token,
                            });
                        }
                        self.pre.insert(mint, c);
                    }
                }
            }
            if self.cfg.bundle_run {
                self.asm.on_create(mint, sig, slot, Some(index), recv, &own);
                self.recent.insert(mint, slot);
                let done = self.asm.on_trades(slot, Some(index), recv, &[]);
                self.push_run_done(done, slot, index, recv);
            }
        } else if self.cfg.bundle_run {
            // buys of the last slots' creates, quoted before execution
            self.trades.clear();
            if let Ok(view) = TxView::parse(&wire) {
                let loaded = (keys.len() > n_static).then(|| &keys[n_static..]);
                if let Ok(buys) = pump_codec::create_hit::pump_buys(&view, loaded, &[]) {
                    for (k, b) in buys.iter().enumerate() {
                        let Some(mint) = b.mint else { continue };
                        if self.recent.get(&mint) != Some(&slot) {
                            continue;
                        }
                        // our own buy is never quoted here: the hot loop
                        // opens a position on our executed TradeEvent (the
                        // post-execution feed or the fill lane); a copy from
                        // before execution would mark the fill before the
                        // chain ran it, and its signature would make the
                        // executed copy the mux's duplicate
                        if b.user.is_some() && b.user == self.cfg.wallet {
                            continue;
                        }
                        let Some(curve) = self.pre.get_mut(&mint) else {
                            continue;
                        };
                        let Some(q) = pre_exec_buy(curve, b.amounts, self.cfg.fees) else {
                            continue;
                        };
                        self.trades.push(TradeLite {
                            mint,
                            user: b.user.unwrap_or_default(),
                            is_buy: true,
                            sol_amount: q.sol_cost,
                            token_amount: q.tokens,
                            fee: q.protocol_fee.saturating_add(q.creator_fee),
                            virtual_sol_reserves: curve.virtual_sol,
                            virtual_token_reserves: curve.virtual_token,
                            slot,
                            sig,
                            ordinal: k.min(255) as u8,
                            index,
                        });
                    }
                }
            }
            let mut run_trades: Vec<(Pubkey, RunTrade)> = Vec::with_capacity(self.trades.len());
            for t in self.trades.iter() {
                let ev = FeedEvent {
                    src: self.cfg.src,
                    recv_ns: recv,
                    slot,
                    bank_id: 0,
                    payload: Payload::Trade(*t),
                };
                if self.sink.push(ev) {
                    self.stats.trade_events.fetch_add(1, Ordering::Relaxed);
                    self.health.event(slot);
                }
                run_trades.push((
                    t.mint,
                    RunTrade {
                        index,
                        is_create: false,
                        user: t.user,
                        is_buy: true,
                        sol: t.sol_amount,
                        tok: t.token_amount,
                        v: t.virtual_sol_reserves,
                        t: t.virtual_token_reserves,
                    },
                ));
            }
            let done = self.asm.on_trades(slot, Some(index), recv, &run_trades);
            self.push_run_done(done, slot, index, recv);
        }
        if self.cfg.bundle_run {
            if slot > self.recent_slot || self.recent.len() > 4_096 {
                self.recent_slot = self.recent_slot.max(slot);
                let floor = slot.saturating_sub(4);
                self.recent.retain(|_, s| *s >= floor);
                let recent = &self.recent;
                self.pre.retain(|m, _| recent.contains_key(m));
            }
            self.flush_bundles();
        }
    }

    fn push_run_done(&mut self, done: Vec<Pubkey>, slot: u64, index: u32, recv: u64) {
        for mint in done {
            let ev = FeedEvent {
                src: self.cfg.src,
                recv_ns: recv,
                slot,
                bank_id: 0,
                payload: Payload::RunDone(RunDoneLite { mint, slot, index }),
            };
            if self.sink.push(ev) {
                self.stats.run_done_events.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn bump_slot(&self, slot: u64) {
        if slot > self.stats.slot.load(Ordering::Relaxed) {
            self.stats.slot.store(slot, Ordering::Relaxed);
        }
        if slot > self.health.last_slot.load(Ordering::Relaxed) {
            self.health.last_slot.store(slot, Ordering::Relaxed);
        }
    }

    fn flush_bundles(&mut self) {
        for r in self.asm.drain() {
            self.stats.bundle_runs.fetch_add(1, Ordering::Relaxed);
            if self.bundles.try_send(r).is_err() {
                self.stats.bundle_drops.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Handle one update; `true` when the server pinged (answer it).
    fn on_update(&mut self, u: SubscribeUpdate) -> bool {
        let recv = mono_ns();
        let Some(up) = u.update_oneof else {
            return false;
        };
        match up {
            // pings and pongs prove the connection, not the data flow: only
            // transactions and slots refresh the stale detector
            UpdateOneof::Ping(_) => {
                self.stats.pings.fetch_add(1, Ordering::Relaxed);
                return true;
            }
            UpdateOneof::Pong(_) => {}
            UpdateOneof::Slot(s) => {
                self.health.frame();
                self.stats.slot_updates.fetch_add(1, Ordering::Relaxed);
                self.bump_slot(s.slot);
                self.asm.on_slot(s.slot, recv);
                self.flush_bundles();
            }
            UpdateOneof::Transaction(t) => {
                self.health.frame();
                self.on_tx(&u.filters, &t, recv);
            }
            _ => {}
        }
        false
    }

    fn on_tx(&mut self, filters: &[String], u: &SubscribeUpdateTransaction, recv: u64) {
        self.stats.transactions.fetch_add(1, Ordering::Relaxed);
        let Some(info) = u.transaction.as_ref() else {
            return;
        };
        let Some(tx) = info.transaction.as_ref() else {
            return;
        };
        let slot = u.slot;
        self.bump_slot(slot);
        let index = u32::try_from(info.index).ok();
        // ordering by (slot, index) as delivered
        let pos = (slot, index.unwrap_or(0));
        if pos < self.last_pos {
            self.stats.reordered.fetch_add(1, Ordering::Relaxed);
        }
        self.last_pos = self.last_pos.max(pos);
        let failed = info.meta.as_ref().is_some_and(|m| m.err.is_some());
        if failed {
            FeedHealth::inc(&self.health.failed_tx);
        }
        let has = |f: &str| filters.iter().any(|x| x == f);
        let wire = match convert::tx_to_wire(tx) {
            Ok(w) => w,
            Err(_) => {
                self.stats.convert_errors.fetch_add(1, Ordering::Relaxed);
                FeedHealth::inc(&self.health.decode_errors);
                return;
            }
        };
        // the full key list (static + loaded) for CPI events and the create check
        let mut keys: Vec<Pubkey> = tx
            .message
            .as_ref()
            .map(|m| {
                m.account_keys
                    .iter()
                    .filter_map(|k| <[u8; 32]>::try_from(k.as_slice()).ok())
                    .map(Pubkey::new_from_array)
                    .collect()
            })
            .unwrap_or_default();
        if let Some(m) = info.meta.as_ref() {
            for k in m
                .loaded_writable_addresses
                .iter()
                .chain(&m.loaded_readonly_addresses)
            {
                if let Ok(a) = <[u8; 32]>::try_from(k.as_slice()) {
                    keys.push(Pubkey::new_from_array(a));
                }
            }
        }
        let mut created: Option<Pubkey> = None;
        if has(F_CREATES) && !failed {
            self.meta_buf.clear();
            let m = convert::meta_in(info.meta.as_ref(), index);
            wiremeta::encode(&m, &mut self.meta_buf);
            // decode here too (cold side of the thread): health counters and
            // the mint of the bundle run; the hot thread decodes its own copy
            created = self.check_create(&wire, &keys, slot);
            if self.wire.push(recv, slot, u.bank_id, &wire, &self.meta_buf) {
                self.stats.wire_events.fetch_add(1, Ordering::Relaxed);
                self.health.event(slot);
            }
            // the miss line (a blocking stderr write) only after the push
            if let Some((info, err)) = self.pending_miss.take() {
                eprintln!("{}", miss_line(self.health.src.name(), &info, &err));
            }
        }
        let n = trades_of(u, &keys, &mut self.trades);
        for t in self.trades.iter_mut() {
            t.index = index.unwrap_or(u32::MAX);
        }
        if n > 0 {
            let wallet = self.cfg.wallet;
            for t in self.trades.iter() {
                // held mints, our own trades and (bundle_run) the trades of
                // the last slots' creates: the hot loop's bundle gate fires
                // on another wallet's buy in the create slot
                if Some(t.user) == wallet
                    || self.mint_set.contains(&t.mint)
                    || (self.cfg.bundle_run && self.recent.get(&t.mint) == Some(&slot))
                {
                    let ev = FeedEvent {
                        src: self.cfg.src,
                        recv_ns: recv,
                        slot,
                        bank_id: u.bank_id,
                        payload: Payload::Trade(*t),
                    };
                    if self.sink.push(ev) {
                        self.stats.trade_events.fetch_add(1, Ordering::Relaxed);
                        self.health.event(slot);
                    }
                }
            }
        }
        if self.cfg.bundle_run {
            let run_trades: Vec<(Pubkey, RunTrade)> = self
                .trades
                .iter()
                .map(|t| {
                    (
                        t.mint,
                        RunTrade {
                            index: index.unwrap_or(u32::MAX),
                            is_create: false,
                            user: t.user,
                            is_buy: t.is_buy,
                            sol: t.sol_amount,
                            tok: t.token_amount,
                            v: t.virtual_sol_reserves,
                            t: t.virtual_token_reserves,
                        },
                    )
                })
                .collect();
            let done = if let Some(mint) = created {
                let own: Vec<RunTrade> = run_trades
                    .iter()
                    .filter(|(m, _)| *m == mint)
                    .map(|(_, t)| *t)
                    .collect();
                let sig = Sig(info.signature.as_slice().try_into().unwrap_or([0; 64]));
                self.asm.on_create(mint, sig, slot, index, recv, &own);
                self.recent.insert(mint, slot);
                // a create is a later index for the slot's other open runs
                self.asm.on_trades(slot, index, recv, &[])
            } else if run_trades.iter().any(|(m, _)| self.recent.contains_key(m)) {
                self.asm.on_trades(slot, index, recv, &run_trades)
            } else {
                // a later index of the slot still closes open bundle runs
                self.asm.on_trades(slot, index, recv, &[])
            };
            // the hot loop's bundle gate fires a parked create once its run
            // is complete (the post-bundle state is then known)
            for mint in done {
                let ev = FeedEvent {
                    src: self.cfg.src,
                    recv_ns: recv,
                    slot,
                    bank_id: u.bank_id,
                    payload: Payload::RunDone(RunDoneLite {
                        mint,
                        slot,
                        index: index.unwrap_or(u32::MAX),
                    }),
                };
                if self.sink.push(ev) {
                    self.stats.run_done_events.fetch_add(1, Ordering::Relaxed);
                }
            }
            if slot > self.recent_slot || self.recent.len() > 4_096 {
                self.recent_slot = self.recent_slot.max(slot);
                let floor = slot.saturating_sub(4);
                self.recent.retain(|_, s| *s >= floor);
            }
            self.flush_bundles();
        }
    }

    /// Decode the create on the adapter thread: health counters (creates,
    /// decode misses by kind) and the mint for the bundle run. A gated miss
    /// is parked in `pending_miss` for `on_tx` to print after the wire push.
    fn check_create(&mut self, wire: &[u8], keys: &[Pubkey], slot: u64) -> Option<Pubkey> {
        let mut check = classify_create(wire, keys);
        if matches!(check, CreateCheck::NeedsMeta) {
            // a launcher CPI create: the trailer just encoded carries the
            // inner instructions, as the hot thread reads them; without this
            // the mint is unknown here, its bundle run is never assembled
            // and its trades never reach the hot loop's bundle gate
            check = classify_cpi_create(wire, keys, &self.meta_buf);
        }
        match check {
            CreateCheck::Hit(mint) => {
                FeedHealth::inc(&self.health.creates);
                Some(mint)
            }
            CreateCheck::NeedsMeta => {
                // a launcher CPI: the hot thread has the inner instructions
                FeedHealth::inc(&self.health.creates);
                None
            }
            CreateCheck::NotCreate => None,
            CreateCheck::Miss { err, mut info } => {
                info.slot = slot;
                info.ordinal = self.health.miss(info.kind);
                if log_gate(info.ordinal) {
                    self.pending_miss = Some((info, err));
                }
                None
            }
        }
    }
}

/// What the adapter-side decode of a `creates`-filter transaction found.
#[derive(Debug, Clone, Copy)]
pub enum CreateCheck {
    /// A create of this mint.
    Hit(Pubkey),
    /// A launcher CPI create: only the hot thread (with the inner
    /// instructions) can resolve the mint.
    NeedsMeta,
    /// Mentions the mint authority but creates nothing (expected: the
    /// `creates` filter is a mention filter).
    NotCreate,
    /// A create-looking transaction the decoder rejected, with what the log
    /// line needs (`info.slot` / `info.ordinal` are the caller's).
    Miss { err: HitError, info: MissInfo },
}

/// [`classify_create`] for a launcher CPI create, with the inner
/// instructions of the encoded wire trailer `meta` (what the hot thread
/// decodes). `NeedsMeta` again means the trailer holds no inner
/// instructions (`inner_instructions_none`).
pub fn classify_cpi_create(wire: &[u8], keys: &[Pubkey], meta: &[u8]) -> CreateCheck {
    let Ok(tx) = TxView::parse(wire) else {
        return CreateCheck::NeedsMeta;
    };
    let Some(m) = wiremeta::WireMeta::parse(meta) else {
        return CreateCheck::NeedsMeta;
    };
    let mut inner = [wiremeta::EMPTY_INNER; wiremeta::MAX_INNER];
    let ni = m.inner_into(&mut inner);
    if ni == 0 {
        return CreateCheck::NeedsMeta;
    }
    let n_static = tx.static_account_keys().len();
    let loaded = (keys.len() > n_static).then(|| &keys[n_static..]);
    match pump_codec::create_hit(
        &tx,
        loaded,
        &inner[..ni],
        pump_codec::curve::FeeBps::DEFAULT,
    ) {
        Ok(Some(h)) => CreateCheck::Hit(h.mint),
        Ok(None) => CreateCheck::NotCreate,
        Err(HitError::CpiCreateNeedsMeta { .. }) => CreateCheck::NeedsMeta,
        Err(e) => CreateCheck::Miss {
            err: e,
            info: MissInfo::blank(MissKind::of_hit(&e)),
        },
    }
}

/// Classify a `creates`-filter transaction as the adapter decodes it: no
/// inner instructions (`create_hit` with an empty list), the ALT-loaded keys
/// from `keys` (static keys first, then `meta.loadedAddresses`).
///
/// A transaction whose wire bytes do not parse is a `Miss` too (it feeds the
/// decode-miss kill ratio): the bytes are our own re-serialization of the
/// protobuf, so this is a codec bug and should stay at zero.
pub fn classify_create(wire: &[u8], keys: &[Pubkey]) -> CreateCheck {
    let tx = match TxView::parse(wire) {
        Ok(tx) => tx,
        Err(e) => {
            return CreateCheck::Miss {
                err: HitError::Decode(e),
                info: MissInfo::blank(MissKind::of_decode(&e)),
            };
        }
    };
    let n_static = tx.static_account_keys().len();
    let loaded = (keys.len() > n_static).then(|| &keys[n_static..]);
    match pump_codec::create_hit(&tx, loaded, &[], pump_codec::curve::FeeBps::DEFAULT) {
        Ok(Some(h)) => CreateCheck::Hit(h.mint),
        Ok(None) => CreateCheck::NotCreate,
        Err(HitError::CpiCreateNeedsMeta { .. }) => CreateCheck::NeedsMeta,
        Err(e) => {
            let mut info = MissInfo::blank(MissKind::of_hit(&e));
            if let Some(sig) = tx.signature(0) {
                info.sig = *sig;
            }
            for ix in tx.instructions().take(info.programs.len()) {
                info.programs[info.n_programs as usize] = *tx.program_id(&ix);
                info.n_programs += 1;
            }
            info.n_ix = tx.num_instructions().min(255) as u8;
            info.lut = Some(tx.version() != TxVersion::Legacy);
            info.loaded = Some(loaded.is_some());
            info.inner = Some(false);
            CreateCheck::Miss { err: e, info }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_deshred_request_streams_every_pump_transaction_and_the_slots() {
        let r = build_deshred_request(&GrpcConfig::default());
        let f = &r.deshred_transactions[F_DESHRED];
        assert_eq!(f.account_include, vec![PUMP_PROGRAM_ID.to_string()]);
        assert_eq!(f.vote, Some(false));
        assert!(r.slots.contains_key(F_SLOTS));
        assert!(
            r.ping.is_none(),
            "a ping in the first request disables the filters on some servers"
        );
        assert!(deshred_ping(9).ping.is_some_and(|p| p.id == 9));
    }

    #[test]
    fn pre_execution_buys_follow_the_curve_the_chain_will_run() {
        let creator = Pubkey::new_unique();
        let fees = FeeBps::DEFAULT;
        let mut c = CurveState::fresh(creator);
        let v0 = c.virtual_sol;
        let sim = CurveState::fresh(creator).simulate_buys(&[1_000_000_000, 500_000_000], fees);
        let q1 = pre_exec_buy(
            &mut c,
            TradeAmounts::ExactQuoteIn {
                spendable_quote_in: 1_000_000_000,
                min_tokens_out: 0,
            },
            fees,
        )
        .unwrap();
        assert_eq!(q1.tokens, sim[0].0.tokens);
        assert_eq!(c.virtual_sol, sim[0].1.virtual_sol);
        assert!(c.virtual_sol > v0);
        let q2 = pre_exec_buy(
            &mut c,
            TradeAmounts::ExactTokensOut {
                amount: sim[1].0.tokens,
                max_quote_cost: u64::MAX,
            },
            fees,
        )
        .unwrap();
        assert_eq!(q2.tokens, sim[1].0.tokens);
        assert_eq!(c.virtual_token, sim[1].1.virtual_token);
        assert!(pre_exec_buy(
            &mut c,
            TradeAmounts::Sell {
                amount: 1,
                min_quote_out: 0
            },
            fees
        )
        .is_none());
        assert!(pre_exec_buy(
            &mut c,
            TradeAmounts::ExactTokensOut {
                amount: 0,
                max_quote_cost: 1
            },
            fees
        )
        .is_none());
    }

    #[test]
    fn request_has_every_filter() {
        let w = Pubkey::new_unique();
        let m = Pubkey::new_unique();
        let cfg = GrpcConfig {
            wallet: Some(w),
            bundle_run: true,
            ..Default::default()
        };
        let r = build_request(&cfg, &[m]);
        assert_eq!(r.commitment, Some(CommitmentLevel::Processed as i32));
        let c = &r.transactions[F_CREATES];
        assert_eq!(c.account_include, vec![MINT_AUTHORITY.to_string()]);
        assert_eq!((c.vote, c.failed), (Some(false), Some(false)));
        let wf = &r.transactions[F_WALLET];
        assert_eq!(wf.account_include, vec![w.to_string()]);
        assert_eq!(wf.failed, None, "our failed buys are streamed too");
        assert_eq!(r.transactions[F_MINTS].account_include, vec![m.to_string()]);
        assert_eq!(
            r.transactions[F_PUMP].account_include,
            vec![PUMP_PROGRAM_ID.to_string()]
        );
        assert!(r.slots.contains_key(F_SLOTS));
        let bare = build_request(&GrpcConfig::default(), &[]);
        assert_eq!(bare.transactions.len(), 1, "creates only");
        assert!(ping_request(3).ping.is_some_and(|p| p.id == 3));
        let dbg = format!(
            "{:?}",
            GrpcConfig {
                x_token: Some("secret".into()),
                ..Default::default()
            }
        );
        assert!(!dbg.contains("secret"), "the token is never printed");
    }

    /// A legacy transaction with one pump instruction of `disc ++ args`.
    fn pump_tx(disc: [u8; 8], args: &[u8]) -> (Vec<u8>, Vec<Pubkey>) {
        use yellowstone_grpc_proto::prelude::{
            CompiledInstruction, Message, MessageHeader, Transaction,
        };
        let payer = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let keys = vec![payer, mint, MINT_AUTHORITY, PUMP_PROGRAM_ID];
        let mut data = disc.to_vec();
        data.extend_from_slice(args);
        let tx = Transaction {
            signatures: vec![vec![7; 64]],
            message: Some(Message {
                header: Some(MessageHeader {
                    num_required_signatures: 1,
                    num_readonly_signed_accounts: 0,
                    num_readonly_unsigned_accounts: 2,
                }),
                account_keys: keys.iter().map(|k| k.to_bytes().to_vec()).collect(),
                recent_blockhash: vec![3; 32],
                instructions: vec![CompiledInstruction {
                    program_id_index: 3,
                    accounts: vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                    data,
                }],
                ..Default::default()
            }),
        };
        (convert::tx_to_wire(&tx).unwrap(), keys)
    }

    #[test]
    fn malformed_create_is_counted_and_classified() {
        use crate::wirering::wire_ring;
        use pump_codec::constants::ix;
        use pump_codec::DecodeError;
        // borsh string of length 5 with one byte present
        let (wire, keys) = pump_tx(ix::CREATE_V2, &[5, 0, 0, 0, b'a']);
        let CreateCheck::Miss { err, info } = classify_create(&wire, &keys) else {
            panic!("a miss");
        };
        assert!(
            matches!(
                err,
                HitError::Decode(DecodeError::UnexpectedEof {
                    offset: 12,
                    needed: 4
                })
            ),
            "{err:?}"
        );
        assert_eq!(info.kind, MissKind::Eof);
        assert_eq!((info.n_programs, info.n_ix), (1, 1));
        assert_eq!(info.programs[0], PUMP_PROGRAM_ID);
        assert_eq!(info.sig, [7; 64]);
        assert_eq!(
            (info.lut, info.loaded, info.inner),
            (Some(false), Some(false), Some(false))
        );
        // valid name / symbol / uri / creator, then is_mayhem_mode = 2
        let mut args = vec![];
        for s in ["name", "SYM", "https://x"] {
            args.extend_from_slice(&(s.len() as u32).to_le_bytes());
            args.extend_from_slice(s.as_bytes());
        }
        args.extend_from_slice(&[9; 32]);
        args.push(2);
        let (bad_bool, keys2) = pump_tx(ix::CREATE_V2, &args);
        assert!(matches!(
            classify_create(&bad_bool, &keys2),
            CreateCheck::Miss {
                err: HitError::Decode(DecodeError::InvalidBool { value: 2, .. }),
                info: MissInfo {
                    kind: MissKind::InvalidBool,
                    ..
                }
            }
        ));
        // the legacy create takes the same path
        let (legacy, keys3) = pump_tx(ix::CREATE, &[5, 0, 0, 0, b'a']);
        assert!(matches!(
            classify_create(&legacy, &keys3),
            CreateCheck::Miss {
                info: MissInfo {
                    kind: MissKind::Eof,
                    ..
                },
                ..
            }
        ));
        // the adapter: counted by kind, the gate admits the first misses
        let (sink, _feed, _stop) = ring(FeedSrc::Grpc, 64, 0);
        let health = sink.health().clone();
        let (wsink, _wire) = wire_ring(FeedSrc::Grpc, 1 << 16, health.clone());
        let (btx, _brx) = std::sync::mpsc::sync_channel(4);
        let mut a = Adapter::new(GrpcConfig::default(), sink, wsink, Arc::default(), btx);
        assert_eq!(a.check_create(&wire, &keys, 1), None);
        assert_eq!(a.check_create(&wire, &keys, 1), None);
        assert_eq!(health.decode_miss.load(Ordering::Relaxed), 2);
        assert_eq!(
            health.decode_miss_kind[MissKind::Eof as usize].load(Ordering::Relaxed),
            2
        );
        assert_eq!(health.creates.load(Ordering::Relaxed), 0);
        let (info, err) = a.pending_miss.take().expect("gated miss parked");
        assert_eq!((info.ordinal, info.slot, info.kind), (2, 1, MissKind::Eof));
        let line = miss_line("grpc", &info, &err);
        assert!(
            line.contains(" n=2 slot=1 ") && line.contains(" kind=eof "),
            "{line}"
        );
        // an unknown pump instruction that mentions the mint authority is the
        // launcher-CPI shape: a create for the counters, never a miss
        let (plain, keys4) = pump_tx([0; 8], &[]);
        assert!(matches!(
            classify_create(&plain, &keys4),
            CreateCheck::NeedsMeta
        ));
        assert_eq!(a.check_create(&plain, &keys4, 2), None);
        assert_eq!(health.creates.load(Ordering::Relaxed), 1);
        assert_eq!(health.decode_miss.load(Ordering::Relaxed), 2);
        assert!(a.pending_miss.is_none());
    }
}
