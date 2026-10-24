//! `trait Sender` and its implementations.
//!
//! | sender | compiled | constructed | network |
//! |---|---|---|---|
//! | [`paper::Paper`] | always | always (the only sender in paper mode) | **never** (no socket code in the module) |
//! | `RpcSend` | feature `rpc` | probe/live after `Config::validate` passes | JSON-RPC `sendTransaction` over TLS, off the hot thread |
//! | `Helius`, `HeliusSwqos`, `Nozomi`, `NozomiBundle`, `Astralane`, `BlockRazor`, `Jito` (one per region), `NextBlock` | feature `senders` | same gate | fixed-buffer HTTP/1.1 over warm `TcpStream`s |
//!
//! Every real sender knows its tip accounts and minimum tip and refuses a
//! variant whose tip goes elsewhere or is too small **before** touching the
//! network, and sits behind a [`Breaker`] and a [`TokenBucket`].
//!
//! **One variant per sender.** Each provider has its own tip accounts, so
//! the sender thread re-targets every bid plan per sender
//! ([`tip_plan`]): the tip account is the provider's (rotated, and distinct
//! between senders that share a tip-account set, so every variant has its
//! own signature), the tip is `max(plan tip, provider minimum)` (a tip only
//! moves when the buy succeeds, so the floor costs nothing on failure and
//! Nozomi never silently drops a variant), and all of them ride the intent's
//! single durable nonce, so at most one executes.

pub mod paper;

#[cfg(feature = "senders")]
pub mod http;
#[cfg(feature = "senders")]
pub mod providers;
#[cfg(feature = "rpc")]
pub mod rpc_send;

use crate::template::Template;
use crate::MAX_TX_LEN;
use pump_codec::Pubkey;
use rugsnipe_core::config::{Config, Refusal, ValidateEnv};
use rugsnipe_core::Mode;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::*};
use std::sync::Arc;
use zeroize::Zeroizing;

/// Provider ids (also the rslog `Sent.sender` byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SenderId {
    Paper = 0,
    Rpc = 1,
    Helius = 2,
    Nozomi = 3,
    Astralane = 4,
    Jito = 5,
    NextBlock = 6,
    HeliusSwqos = 7,
    BlockRazor = 8,
    /// Nozomi Send Bundle: revert-protected buy copies ([`Sender::bundle`]).
    NozomiBundle = 9,
}

impl SenderId {
    /// Every id, in discriminant order (index `i` is the id with value `i`):
    /// size per-sender tables with `SenderId::COUNT` and index them by `id as usize`.
    pub const ALL: [SenderId; 10] = [
        Self::Paper,
        Self::Rpc,
        Self::Helius,
        Self::Nozomi,
        Self::Astralane,
        Self::Jito,
        Self::NextBlock,
        Self::HeliusSwqos,
        Self::BlockRazor,
        Self::NozomiBundle,
    ];
    pub const COUNT: usize = Self::ALL.len();

    pub fn name(self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Rpc => "rpcsend",
            Self::Helius => "helius",
            Self::Nozomi => "nozomi",
            Self::Astralane => "astralane",
            Self::Jito => "jito",
            Self::NextBlock => "nextblock",
            Self::HeliusSwqos => "helius_swqos",
            Self::BlockRazor => "blockrazor",
            Self::NozomiBundle => "nozomi_bundle",
        }
    }

    /// From the rslog / ledger byte.
    pub fn from_u8(b: u8) -> Option<Self> {
        Some(match b {
            0 => Self::Paper,
            1 => Self::Rpc,
            2 => Self::Helius,
            3 => Self::Nozomi,
            4 => Self::Astralane,
            5 => Self::Jito,
            6 => Self::NextBlock,
            7 => Self::HeliusSwqos,
            8 => Self::BlockRazor,
            9 => Self::NozomiBundle,
            _ => return None,
        })
    }
}

/// What a variant does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum VariantKind {
    Buy = 0,
    Sell = 1,
    /// AdvanceNonce-only burn.
    Burn = 2,
}

/// One signed transaction ready for one sender (no heap).
#[derive(Clone)]
pub struct Variant {
    pub intent: u64,
    pub idx: u8,
    pub kind: VariantKind,
    pub priority_lamports: u64,
    pub tip_lamports: u64,
    pub tip_account: Option<Pubkey>,
    pub sig: [u8; 64],
    buf: [u8; MAX_TX_LEN],
    len: u16,
}

impl core::fmt::Debug for Variant {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Variant")
            .field("intent", &self.intent)
            .field("idx", &self.idx)
            .field("kind", &self.kind)
            .field("sig", &bs58::encode(self.sig).into_string())
            .field("len", &self.len)
            .field("tip", &self.tip_lamports)
            .finish()
    }
}

impl Variant {
    /// Snapshot a patched (and normally signed) template.
    pub fn from_template(
        t: &Template,
        intent: u64,
        idx: u8,
        kind: VariantKind,
        priority_lamports: u64,
        tip_lamports: u64,
        tip_account: Option<Pubkey>,
    ) -> Self {
        Self::from_bytes(
            t.bytes(),
            t.signature(),
            intent,
            idx,
            kind,
            priority_lamports,
            tip_lamports,
            tip_account,
        )
    }

    /// From raw bytes (must be <= [`MAX_TX_LEN`]; longer input is truncated
    /// and then refused by every sender as `TooLarge`).
    pub fn from_bytes(
        bytes: &[u8],
        sig: [u8; 64],
        intent: u64,
        idx: u8,
        kind: VariantKind,
        priority_lamports: u64,
        tip_lamports: u64,
        tip_account: Option<Pubkey>,
    ) -> Self {
        let mut buf = [0u8; MAX_TX_LEN];
        let n = bytes.len().min(MAX_TX_LEN);
        buf[..n].copy_from_slice(&bytes[..n]);
        let len = if bytes.len() > MAX_TX_LEN {
            u16::MAX
        } else {
            n as u16
        };
        Self {
            intent,
            idx,
            kind,
            priority_lamports,
            tip_lamports,
            tip_account,
            sig,
            buf,
            len,
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.buf[..(self.len as usize).min(MAX_TX_LEN)]
    }
    pub fn too_large(&self) -> bool {
        self.len as usize > MAX_TX_LEN
    }
    pub fn signature_b58(&self) -> String {
        bs58::encode(self.sig).into_string()
    }
}

/// Why a send did not happen or failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    BreakerOpen,
    RateLimited,
    TooLarge,
    /// Tip below the provider minimum.
    TipTooLow {
        min: u64,
        got: u64,
    },
    /// Tip account is not one of the provider's.
    WrongTipAccount,
    /// Non-2xx HTTP status.
    Http(u16),
    Io(String),
    /// JSON-RPC error object.
    Rpc(String),
    /// This path does not carry this kind of variant (`rpcsend.sells_only`
    /// refuses buys); decided before any network.
    NotForThisSender,
}

/// Outcome of [`Sender::send`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendResult {
    /// Paper: recorded, nothing sent.
    Recorded,
    /// The provider accepted the request.
    Accepted {
        latency_ns: u64,
    },
    Failed(SendError),
}

impl SendResult {
    pub fn is_ok(&self) -> bool {
        !matches!(self, Self::Failed(_))
    }
    /// rslog `Sent.result` byte.
    pub fn code(&self) -> u8 {
        match self {
            Self::Recorded => 0,
            Self::Accepted { .. } => 1,
            Self::Failed(_) => 2,
        }
    }
}

/// A transaction submission path.
/// How long a sender's worker waits for work before renewing its connection
/// ([`Sender::keepalive`]): under every provider's idle close (Nozomi 65 s,
/// Helius about a minute).
pub const KEEPALIVE_EVERY: std::time::Duration = std::time::Duration::from_secs(20);

pub trait Sender: Send + Sync + core::fmt::Debug {
    fn id(&self) -> SenderId;
    /// Submit one variant (blocking; call from a sender thread).
    fn send(&self, variant: &Variant) -> SendResult;
    /// `false` only for [`paper::Paper`].
    fn is_real(&self) -> bool {
        true
    }
    /// Tip accounts this path accepts (empty = no tip required).
    fn tip_accounts(&self) -> &[Pubkey] {
        &[]
    }
    /// Minimum tip in lamports (0 = none).
    fn min_tip(&self) -> u64 {
        0
    }
    /// Name in the ledger and the summary (`jito_frankfurt`, ...).
    fn label(&self) -> &'static str {
        self.id().name()
    }
    /// Buy variants of one intent this path takes (Jito: 1, the
    /// pay-on-success primary, because of its 1 req/s per IP and region).
    fn buy_variants(&self) -> u8 {
        2
    }
    /// A revert-protected bundle path (Nozomi Send Bundle): a builder drops
    /// a bundle whose transaction would fail, so a buy copy sent here pays
    /// nothing when it loses. It takes only the buy variants that bid for
    /// it (`FireJob::bundle_prio`), each as its own one-transaction bundle
    /// with the bid as the priority fee and the provider's minimum tip,
    /// and never a sell.
    fn bundle(&self) -> bool {
        false
    }
    /// Open the connection ahead of time (no-op where there is none).
    fn warm(&self) -> Result<(), SendError> {
        Ok(())
    }
    /// Keep the connection warm while idle (the worker calls it every
    /// [`KEEPALIVE_EVERY`] without work); no-op where there is none.
    fn keepalive(&self) {}
    /// Several variants in one request where the wire allows it (Nozomi
    /// batch send); one result per variant.
    fn send_batch(&self, vs: &[&Variant]) -> Vec<SendResult> {
        vs.iter().map(|v| self.send(v)).collect()
    }
    /// May a variant of `kind` go out at `now_ms`? `false` only when the
    /// path's breaker is open and, for a sell, its one half-open probe is
    /// already in flight ([`Breaker::allows`]). Decided before any network.
    fn ready(&self, _kind: VariantKind, _now_ms: u64) -> bool {
        true
    }
}

/// Tip account and tip of variant `idx` of `intent` for one sender.
/// `ordinal` is the sender's position in the set: senders that share a tip
/// account set (Jito regions, the two Helius legs) get different accounts,
/// so no two variants of one intent share a signature. The tip is raised to
/// the provider's minimum (it reverts with a failed buy). `None` for a path
/// without tips (`rpcsend`).
pub fn tip_plan(
    s: &dyn Sender,
    ordinal: usize,
    idx: u8,
    intent: u64,
    plan_tip: u64,
) -> Option<(Pubkey, u64)> {
    let accts = s.tip_accounts();
    if accts.is_empty() {
        return None;
    }
    let k = (intent as usize % accts.len() + 3 * ordinal + (idx & 0x7f) as usize) % accts.len();
    Some((accts[k], plan_tip.max(s.min_tip())))
}

/// Token bucket: `rate` tokens per second, at most `burst` saved.
#[derive(Debug)]
pub struct TokenBucket {
    rate: f64,
    burst: f64,
    state: std::sync::Mutex<(f64, std::time::Instant)>,
}

impl TokenBucket {
    /// `rate <= 0` = unlimited.
    pub fn new(rate: f64, burst: f64) -> Self {
        let burst = burst.max(1.0);
        Self {
            rate,
            burst,
            state: std::sync::Mutex::new((burst, std::time::Instant::now())),
        }
    }
    pub fn rate(&self) -> f64 {
        self.rate
    }
    /// Take one token now if there is one.
    pub fn try_take(&self) -> bool {
        self.try_take_at(std::time::Instant::now())
    }
    pub fn try_take_at(&self, now: std::time::Instant) -> bool {
        if self.rate <= 0.0 {
            return true;
        }
        let mut g = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let dt = now.saturating_duration_since(g.1).as_secs_f64();
        g.0 = (g.0 + dt * self.rate).min(self.burst);
        g.1 = now;
        if g.0 >= 1.0 {
            g.0 -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Shared pre-network check for tip-requiring senders.
pub fn check_tip(v: &Variant, accounts: &[Pubkey], min: u64) -> Result<(), SendError> {
    if v.too_large() {
        return Err(SendError::TooLarge);
    }
    if accounts.is_empty() || v.kind == VariantKind::Burn {
        return Ok(());
    }
    match v.tip_account {
        Some(a) if accounts.contains(&a) => {}
        _ => return Err(SendError::WrongTipAccount),
    }
    if v.tip_lamports < min {
        return Err(SendError::TipTooLow {
            min,
            got: v.tip_lamports,
        });
    }
    Ok(())
}

/// Circuit breaker: `threshold` consecutive failures open it for `cooldown_ms`.
///
/// It guards the **buy** fan-out (a dead path is skipped instead of eating
/// the latency budget). A **sell** is never blocked by it: while it is open
/// one sell at a time goes through as a half-open probe ([`Breaker::allows`],
/// [`Breaker::try_probe`]); a sell's success closes it, a sell's failure
/// does not count toward opening it (2026-10-23: a run of `http 408` on
/// buys opened `helius_swqos`, and the three tries of a hold_to sell were
/// spent on `breaker_open` without a byte leaving the box).
#[derive(Debug)]
pub struct Breaker {
    threshold: u32,
    cooldown_ms: u64,
    fails: AtomicU32,
    open_until_ms: AtomicU64,
    probing: std::sync::atomic::AtomicBool,
}

impl Breaker {
    pub fn new(threshold: u32, cooldown_ms: u64) -> Self {
        Self {
            threshold: threshold.max(1),
            cooldown_ms,
            fails: AtomicU32::new(0),
            open_until_ms: AtomicU64::new(0),
            probing: std::sync::atomic::AtomicBool::new(false),
        }
    }
    /// May a variant of `kind` go out at `now_ms`: the breaker is closed, or
    /// it is a sell and no sell probe is in flight.
    pub fn allows(&self, kind: VariantKind, now_ms: u64) -> bool {
        self.allow(now_ms) || (kind == VariantKind::Sell && !self.probing.load(Acquire))
    }
    /// Claim the one half-open probe of an open breaker (`false`: one is in
    /// flight). Give it back with [`Breaker::end_probe`].
    pub fn try_probe(&self) -> bool {
        self.probing
            .compare_exchange(false, true, AcqRel, Acquire)
            .is_ok()
    }
    pub fn end_probe(&self) {
        self.probing.store(false, Release);
    }
    /// May a request go out at `now_ms`? (After the cooldown one probe is allowed.)
    pub fn allow(&self, now_ms: u64) -> bool {
        now_ms >= self.open_until_ms.load(Acquire)
    }
    pub fn is_open(&self, now_ms: u64) -> bool {
        !self.allow(now_ms)
    }
    pub fn on_success(&self) {
        self.fails.store(0, Release);
        self.open_until_ms.store(0, Release);
    }
    pub fn on_failure(&self, now_ms: u64) {
        let n = self.fails.fetch_add(1, AcqRel) + 1;
        if n >= self.threshold {
            self.open_until_ms.store(now_ms + self.cooldown_ms, Release);
        }
    }
    pub fn consecutive_failures(&self) -> u32 {
        self.fails.load(Acquire)
    }
}

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// API keys from the `api-keys` credential (TOML `name = "value"`), held in
/// zeroizing strings and never printed.
#[derive(Default)]
pub struct ApiKeys(BTreeMap<String, Zeroizing<String>>);

impl core::fmt::Debug for ApiKeys {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("ApiKeys")
            .field(&self.0.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl ApiKeys {
    /// Parse `name = "value"` lines (comments with `#`).
    pub fn parse(text: &Zeroizing<String>) -> Result<Self, String> {
        let mut m = BTreeMap::new();
        for (n, line) in text.lines().enumerate() {
            let l = line.trim();
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            let (k, v) = l
                .split_once('=')
                .ok_or(format!("line {}: expected name = \"value\"", n + 1))?;
            let v = v.trim();
            let v = v
                .strip_prefix('"')
                .and_then(|x| x.strip_suffix('"'))
                .ok_or(format!("line {}: value must be quoted", n + 1))?;
            m.insert(k.trim().to_string(), Zeroizing::new(v.to_string()));
        }
        Ok(Self(m))
    }
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(|s| s.as_str())
    }
    pub fn insert(&mut self, name: &str, value: &str) {
        self.0
            .insert(name.to_string(), Zeroizing::new(value.to_string()));
    }
}

/// Why [`SenderSet::build`] refused.
#[derive(Debug, Clone, PartialEq)]
pub enum BuildError {
    /// `Config::validate` refused probe/live.
    Refused(Vec<Refusal>),
    /// A sender is enabled but this binary was built without its feature.
    FeatureMissing {
        sender: &'static str,
        feature: &'static str,
    },
    /// A sender's configuration is unusable (bad URL, missing key...).
    Config { sender: &'static str, why: String },
}

/// The senders a process may use, built behind the safety gate.
pub struct SenderSet {
    pub mode: Mode,
    pub paper: Arc<paper::Paper>,
    /// `--probe-dry`: the real senders are built (their tip accounts and
    /// minimum tips shape the signed variants) but never used: every send
    /// goes to the paper recorder and [`SenderSet::real`] is empty.
    pub dry: bool,
    /// `senders.quarantine_min` in ms: a sender above the send-error ratio
    /// is skipped this long by the sender thread.
    pub quarantine_ms: u64,
    /// `risk.send_error_window` / `risk.send_error_kill_ratio`: the
    /// per-sender ratio that quarantines a sender.
    pub send_error_window: usize,
    pub send_error_ratio: f64,
    /// `[sells] senders`: the labels a real sell goes to (empty = all).
    pub sells: rugsnipe_core::config::SellsCfg,
    real: Vec<Box<dyn Sender>>,
}

impl core::fmt::Debug for SenderSet {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SenderSet")
            .field("mode", &self.mode)
            .field("dry", &self.dry)
            .field(
                "real",
                &self.real.iter().map(|s| s.id().name()).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl SenderSet {
    /// Paper mode: only the paper sender, whatever the config enables.
    /// Probe/live: `cfg.validate(env)` must pass (signed ArmSet with
    /// `probe_allowed` for probe or `gate_passed` for live, every cap,
    /// credentials, clock, post-exec feed, budget) and
    /// every enabled real sender must be compiled in.
    pub fn build(cfg: &Config, env: &ValidateEnv, keys: &ApiKeys) -> Result<Self, BuildError> {
        let paper = Arc::new(paper::Paper::new(4_096));
        let mode = cfg.mode.mode;
        if !mode.is_real() {
            return Ok(Self {
                mode,
                paper,
                dry: false,
                quarantine_ms: cfg.senders.quarantine_ms(),
                send_error_window: cfg.risk.send_error_window.max(1) as usize,
                send_error_ratio: cfg.risk.send_error_kill_ratio,
                sells: cfg.sells.clone(),
                real: vec![],
            });
        }
        let v = cfg.validate(env);
        if !v.ok() {
            return Err(BuildError::Refused(v.refusals));
        }
        let real = build_real(cfg, keys)?;
        Ok(Self {
            mode,
            paper,
            dry: false,
            quarantine_ms: cfg.senders.quarantine_ms(),
            send_error_window: cfg.risk.send_error_window.max(1) as usize,
            send_error_ratio: cfg.risk.send_error_kill_ratio,
            sells: cfg.sells.clone(),
            real,
        })
    }

    /// `--probe-dry` (probe mode only): validated like [`SenderSet::build`]
    /// (`env` is the caller's, with the post-execution feed waived for a run
    /// that sends nothing), the real senders are constructed for their tip
    /// accounts and minimum tips but no request ever leaves: they are not
    /// warmed, [`SenderSet::real`] is empty and every send is recorded.
    pub fn build_dry(cfg: &Config, env: &ValidateEnv, keys: &ApiKeys) -> Result<Self, BuildError> {
        if cfg.mode.mode != Mode::Probe {
            return Err(BuildError::Config {
                sender: "dry",
                why: "--probe-dry needs mode = probe".into(),
            });
        }
        let mut s = Self::build(cfg, env, keys)?;
        s.dry = true;
        Ok(s)
    }

    /// Senders that can move money (empty in paper mode and in a dry run).
    pub fn real(&self) -> &[Box<dyn Sender>] {
        if self.dry {
            &[]
        } else {
            &self.real
        }
    }

    /// The senders variants are shaped for (tip accounts, minimum tips):
    /// the real ones, also in a dry run. Never send through these directly;
    /// use [`SenderSet::real`].
    pub fn targets(&self) -> &[Box<dyn Sender>] {
        &self.real
    }

    /// Target `k` of [`SenderSet::targets`] takes real sells (`[sells] senders`;
    /// never a bundle path).
    pub fn sells_via(&self, k: usize) -> bool {
        self.real
            .get(k)
            .is_some_and(|s| !s.bundle() && self.sells.allows(s.label()))
    }

    /// Send through the paper recorder and, in a real mode, every real
    /// sender. Returns `(sender, result)` pairs.
    pub fn send_all(&self, v: &Variant) -> Vec<(SenderId, SendResult)> {
        let mut out = vec![(SenderId::Paper, self.paper.send(v))];
        if self.mode.is_real() {
            for s in self.real() {
                out.push((s.id(), s.send(v)));
            }
        }
        out
    }
}

#[allow(unused_variables, unused_mut)]
fn build_real(cfg: &Config, keys: &ApiKeys) -> Result<Vec<Box<dyn Sender>>, BuildError> {
    let mut out: Vec<Box<dyn Sender>> = vec![];
    let s = &cfg.senders;
    if s.rpcsend.enabled {
        #[cfg(feature = "rpc")]
        out.push(Box::new(
            rpc_send::RpcSend::new(&s.rpcsend.url, s.rpcsend.sells_only).map_err(|why| {
                BuildError::Config {
                    sender: "rpcsend",
                    why,
                }
            })?,
        ));
        #[cfg(not(feature = "rpc"))]
        return Err(BuildError::FeatureMissing {
            sender: "rpcsend",
            feature: "rpc",
        });
    }
    for (name, c) in s.http() {
        if !c.enabled {
            continue;
        }
        #[cfg(feature = "senders")]
        {
            let mut c = c.clone();
            // second legs share their provider's credential name
            c.key = s.key_of(name, &c).to_string();
            if name == "helius_swqos" {
                c.swqos_only = true;
            }
            let built = if name == "jito" && !c.regions.is_empty() {
                c.regions
                    .iter()
                    .map(|r| {
                        let mut one = c.clone();
                        one.region = r.clone();
                        providers::build(name, &one, keys)
                    })
                    .collect::<Result<Vec<_>, _>>()
            } else {
                // the swqos leg is a Helius sender with `swqos_only`
                let kind = if name == "helius_swqos" {
                    "helius"
                } else {
                    name
                };
                providers::build(kind, &c, keys).map(|s| vec![s])
            };
            out.extend(built.map_err(|why| BuildError::Config {
                sender: name_static(name),
                why,
            })?);
        }
        #[cfg(not(feature = "senders"))]
        return Err(BuildError::FeatureMissing {
            sender: name_static(name),
            feature: "senders",
        });
    }
    if s.tpu.enabled {
        return Err(BuildError::FeatureMissing {
            sender: "tpu",
            feature: "tpu (not implemented yet)",
        });
    }
    Ok(out)
}

fn name_static(n: &str) -> &'static str {
    match n {
        "helius" => "helius",
        "helius_swqos" => "helius_swqos",
        "nozomi" => "nozomi",
        "nozomi_bundle" => "nozomi_bundle",
        "astralane" => "astralane",
        "blockrazor" => "blockrazor",
        "jito" => "jito",
        _ => "nextblock",
    }
}

#[cfg(test)]
mod id_tests {
    use super::SenderId;

    #[test]
    fn all_ids_are_dense_and_in_discriminant_order() {
        // a per-sender table of SenderId::COUNT entries indexed by `id as usize`
        // must have a slot for every id (the hot sender thread panicked on
        // BlockRazor = 8 with a 7-entry table, fra1 2026-10-21)
        for (i, id) in SenderId::ALL.iter().enumerate() {
            assert_eq!(*id as usize, i, "{id:?}");
        }
        assert_eq!(SenderId::NozomiBundle as usize + 1, SenderId::COUNT);
        for id in SenderId::ALL {
            assert_eq!(SenderId::from_u8(id as u8), Some(id));
        }
    }
}
