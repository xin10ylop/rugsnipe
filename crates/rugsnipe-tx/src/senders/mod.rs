//! `trait Sender` and its implementations.
//!
//! | sender | compiled | constructed | network |
//! |---|---|---|---|
//! | [`paper::Paper`] | always | always (the only sender in paper mode) | **never** (no socket code in the module) |
//! | `RpcSend` | feature `rpc` | probe/live after `Config::validate` passes | JSON-RPC `sendTransaction` over TLS, off the hot thread |
//! | `Helius`, `Nozomi`, `Astralane`, `Jito`, `NextBlock` | feature `senders` | same gate | fixed-buffer HTTP/1.1 over warm `TcpStream`s |
//!
//! Every real sender knows its tip accounts and minimum tip and refuses a
//! variant whose tip goes elsewhere or is too small **before** touching the
//! network, and sits behind a [`Breaker`].

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
}

impl SenderId {
    pub fn name(self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Rpc => "rpcsend",
            Self::Helius => "helius",
            Self::Nozomi => "nozomi",
            Self::Astralane => "astralane",
            Self::Jito => "jito",
            Self::NextBlock => "nextblock",
        }
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
#[derive(Debug)]
pub struct Breaker {
    threshold: u32,
    cooldown_ms: u64,
    fails: AtomicU32,
    open_until_ms: AtomicU64,
}

impl Breaker {
    pub fn new(threshold: u32, cooldown_ms: u64) -> Self {
        Self {
            threshold: threshold.max(1),
            cooldown_ms,
            fails: AtomicU32::new(0),
            open_until_ms: AtomicU64::new(0),
        }
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
    real: Vec<Box<dyn Sender>>,
}

impl core::fmt::Debug for SenderSet {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SenderSet")
            .field("mode", &self.mode)
            .field(
                "real",
                &self.real.iter().map(|s| s.id().name()).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl SenderSet {
    /// Paper mode: only the paper sender, whatever the config enables.
    /// Probe/live: `cfg.validate(env)` must pass (signed `gate_passed`
    /// ArmSet, every cap, credentials, clock, post-exec feed, budget) and
    /// every enabled real sender must be compiled in.
    pub fn build(cfg: &Config, env: &ValidateEnv, keys: &ApiKeys) -> Result<Self, BuildError> {
        let paper = Arc::new(paper::Paper::new(4_096));
        let mode = cfg.mode.mode;
        if !mode.is_real() {
            return Ok(Self {
                mode,
                paper,
                real: vec![],
            });
        }
        let v = cfg.validate(env);
        if !v.ok() {
            return Err(BuildError::Refused(v.refusals));
        }
        let real = build_real(cfg, keys)?;
        Ok(Self { mode, paper, real })
    }

    /// Senders that can move money (empty in paper mode).
    pub fn real(&self) -> &[Box<dyn Sender>] {
        &self.real
    }

    /// Send through the paper recorder and, in a real mode, every real
    /// sender. Returns `(sender, result)` pairs.
    pub fn send_all(&self, v: &Variant) -> Vec<(SenderId, SendResult)> {
        let mut out = vec![(SenderId::Paper, self.paper.send(v))];
        if self.mode.is_real() {
            for s in &self.real {
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
    for (name, c) in [
        ("helius", &s.helius),
        ("nozomi", &s.nozomi),
        ("astralane", &s.astralane),
        ("jito", &s.jito),
        ("nextblock", &s.nextblock),
    ] {
        if !c.enabled {
            continue;
        }
        #[cfg(feature = "senders")]
        out.push(
            providers::build(name, c, keys).map_err(|why| BuildError::Config {
                sender: name_static(name),
                why,
            })?,
        );
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
        "nozomi" => "nozomi",
        "astralane" => "astralane",
        "jito" => "jito",
        _ => "nextblock",
    }
}
