//! [`FeedHealth`]: per-feed counters shared between the adapter thread and the
//! hot loop (relaxed atomics; read with [`FeedHealth::snapshot`]).

use crate::clock::mono_ns;
use crate::event::FeedSrc;
use pump_codec::{DecodeError, HitError, Pubkey};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};

/// Live counters of one feed.
#[derive(Debug)]
pub struct FeedHealth {
    pub src: FeedSrc,
    pub connected: AtomicBool,
    /// Raw frames received.
    pub frames: AtomicU64,
    /// Events produced.
    pub events: AtomicU64,
    /// Creates decoded (CreateEvent found).
    pub creates: AtomicU64,
    /// Frames that looked like creates (create instruction logged) but had no decodable CreateEvent.
    pub decode_miss: AtomicU64,
    /// `decode_miss` split by [`MissKind`] (index = `kind as usize`).
    pub decode_miss_kind: [AtomicU64; MISS_KIND_COUNT],
    /// Event payloads that failed to decode.
    pub decode_errors: AtomicU64,
    /// Failed transactions (`err != null`), ignored.
    pub failed_tx: AtomicU64,
    /// Truncated log sets.
    pub truncated: AtomicU64,
    pub reconnects: AtomicU64,
    /// Forced reconnects by the stale detector.
    pub stale_reconnects: AtomicU64,
    /// Events dropped because the ring to the hot loop was full.
    pub ring_drops: AtomicU64,
    /// Funding lookups (getTransaction) issued / failed / dropped (queue full).
    pub rpc_calls: AtomicU64,
    pub rpc_errors: AtomicU64,
    pub rpc_drops: AtomicU64,
    /// Monotonic time of the last frame / event (0 = never).
    pub last_frame_ns: AtomicU64,
    pub last_event_ns: AtomicU64,
    pub last_slot: AtomicU64,
    /// Latest measured notification lag versus block time (ms), set by the
    /// caller that measures it (e.g. `examples/tail.rs`); `u64::MAX` = unknown.
    pub lag_ms: AtomicU64,
    /// Stale threshold in ms (0 = disabled).
    pub stale_ms: AtomicU64,
}

/// Plain copy of [`FeedHealth`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthSnapshot {
    pub src: FeedSrc,
    pub connected: bool,
    pub frames: u64,
    pub events: u64,
    pub creates: u64,
    pub decode_miss: u64,
    /// `decode_miss` by [`MissKind`] (index = `kind as usize`).
    pub decode_miss_kind: [u64; MISS_KIND_COUNT],
    pub decode_errors: u64,
    pub failed_tx: u64,
    pub truncated: u64,
    pub reconnects: u64,
    pub stale_reconnects: u64,
    pub ring_drops: u64,
    pub rpc_calls: u64,
    pub rpc_errors: u64,
    pub rpc_drops: u64,
    /// ms since the last frame (`u64::MAX` = never).
    pub since_frame_ms: u64,
    pub last_slot: u64,
    pub lag_ms: u64,
    pub stale: bool,
}

impl HealthSnapshot {
    /// Decoded creates / (decoded + misses); 1.0 when nothing was seen.
    pub fn decode_success_rate(&self) -> f64 {
        let n = self.creates + self.decode_miss;
        if n == 0 {
            1.0
        } else {
            self.creates as f64 / n as f64
        }
    }
}

// ------------------------------------------------------------ decode misses
/// Why a create (a transaction the `creates` filter matched, or a logged
/// create instruction) had no decodable CreateEvent. One value per
/// [`DecodeError`] / [`HitError`] variant plus [`MissKind::Logs`] for the
/// logs path (which only knows "no event").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MissKind {
    /// `DecodeError::UnexpectedEof`: instruction data shorter than its layout.
    Eof = 0,
    BadDiscriminator = 1,
    InvalidBool = 2,
    InvalidUtf8 = 3,
    InvalidLength = 4,
    /// `DecodeError::InvalidTransaction`: the wire bytes did not parse as a
    /// transaction (a codec bug on our own re-serialized bytes; should be 0).
    InvalidTransaction = 5,
    InvalidBase64 = 6,
    /// `HitError::CpiCreateNeedsMeta`: a launcher CPI without inner
    /// instructions (hot thread only; the adapter counts these as creates).
    CpiNeedsMeta = 7,
    /// `HitError::UnresolvedAccounts`: a create account in an unresolved
    /// lookup table (no / short `loadedAddresses`).
    UnresolvedAccounts = 8,
    /// Logs path: a create instruction logged with no CreateEvent payload.
    Logs = 9,
}

/// Number of [`MissKind`] values.
pub const MISS_KIND_COUNT: usize = 10;

impl MissKind {
    pub const ALL: [MissKind; MISS_KIND_COUNT] = [
        MissKind::Eof,
        MissKind::BadDiscriminator,
        MissKind::InvalidBool,
        MissKind::InvalidUtf8,
        MissKind::InvalidLength,
        MissKind::InvalidTransaction,
        MissKind::InvalidBase64,
        MissKind::CpiNeedsMeta,
        MissKind::UnresolvedAccounts,
        MissKind::Logs,
    ];

    /// Metric label / log token.
    pub fn name(self) -> &'static str {
        match self {
            MissKind::Eof => "eof",
            MissKind::BadDiscriminator => "bad_discriminator",
            MissKind::InvalidBool => "invalid_bool",
            MissKind::InvalidUtf8 => "invalid_utf8",
            MissKind::InvalidLength => "invalid_length",
            MissKind::InvalidTransaction => "invalid_transaction",
            MissKind::InvalidBase64 => "invalid_base64",
            MissKind::CpiNeedsMeta => "cpi_needs_meta",
            MissKind::UnresolvedAccounts => "unresolved_accounts",
            MissKind::Logs => "logs",
        }
    }

    /// Kind of a [`DecodeError`] (a match: no heap, hot-thread safe).
    #[inline]
    pub fn of_decode(e: &DecodeError) -> MissKind {
        match e {
            DecodeError::UnexpectedEof { .. } => MissKind::Eof,
            DecodeError::BadDiscriminator { .. } => MissKind::BadDiscriminator,
            DecodeError::InvalidBool { .. } => MissKind::InvalidBool,
            DecodeError::InvalidUtf8 { .. } => MissKind::InvalidUtf8,
            DecodeError::InvalidLength { .. } => MissKind::InvalidLength,
            DecodeError::InvalidTransaction(_) => MissKind::InvalidTransaction,
            DecodeError::InvalidBase64 => MissKind::InvalidBase64,
        }
    }

    /// Kind of a [`HitError`] (a match: no heap, hot-thread safe).
    #[inline]
    pub fn of_hit(e: &HitError) -> MissKind {
        match e {
            HitError::Decode(e) => MissKind::of_decode(e),
            HitError::CpiCreateNeedsMeta { .. } => MissKind::CpiNeedsMeta,
            HitError::UnresolvedAccounts => MissKind::UnresolvedAccounts,
        }
    }
}

/// Whether the `n`-th miss (1-based, per feed and per run) gets a stderr
/// line: the first 50, then every 100th. The running `decode_miss` total
/// is the ordinal, so there is no other rate-limit state.
#[inline]
pub fn log_gate(n: u64) -> bool {
    n <= 50 || n % 100 == 0
}

/// Everything a decode-miss log line says, collected on the stack at the
/// miss site (`Copy`, no heap: the hot thread can fill one too) and
/// formatted later by [`miss_line`] on a cold thread.
#[derive(Debug, Clone, Copy)]
pub struct MissInfo {
    pub slot: u64,
    /// First signature of the transaction (`[0; 64]` when unknown).
    pub sig: [u8; 64],
    pub kind: MissKind,
    /// Top-level program ids, the first `n_programs` (up to 4).
    pub programs: [Pubkey; 4],
    pub n_programs: u8,
    /// Top-level instruction count (saturated at 255).
    pub n_ix: u8,
    /// Versioned (v0 / v1) transaction; `None` = unknown.
    pub lut: Option<bool>,
    /// ALT-loaded keys were available to the decoder; `None` = unknown.
    pub loaded: Option<bool>,
    /// Inner instructions were available to the decoder; `None` = unknown.
    pub inner: Option<bool>,
    /// 1-based ordinal of this miss at its site ([`FeedHealth::miss`]).
    pub ordinal: u64,
}

impl MissInfo {
    /// A miss of `kind` with nothing else known.
    pub const fn blank(kind: MissKind) -> MissInfo {
        MissInfo {
            slot: 0,
            sig: [0; 64],
            kind,
            programs: [Pubkey::new_from_array([0; 32]); 4],
            n_programs: 0,
            n_ix: 0,
            lut: None,
            loaded: None,
            inner: None,
            ordinal: 0,
        }
    }
}

fn flag(v: Option<bool>) -> &'static str {
    match v {
        Some(true) => "1",
        Some(false) => "0",
        None => "?",
    }
}

/// The one-line stderr record of a decode miss (cold / adapter threads only:
/// it allocates). `err` is the decoder's error (`Display`; newlines are
/// replaced by spaces so the line stays one line).
///
/// `decode miss: src=<src> n=<ordinal> slot=<slot> sig=<b58> kind=<kind>
/// err="<err>" programs=<b58,...>[+k] v0=<1|0|?> loaded=<1|0|?> inner=<1|0|?>`
pub fn miss_line(src: &str, m: &MissInfo, err: &dyn core::fmt::Display) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(384);
    let _ = write!(
        s,
        "decode miss: src={src} n={} slot={} sig={} kind={} err=\"",
        m.ordinal,
        m.slot,
        bs58::encode(&m.sig[..]).into_string(),
        m.kind.name()
    );
    for c in err.to_string().chars() {
        s.push(if c == '\n' || c == '\r' { ' ' } else { c });
    }
    s.push_str("\" programs=");
    let n = (m.n_programs as usize).min(m.programs.len());
    for (i, p) in m.programs[..n].iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = write!(s, "{p}");
    }
    if m.n_ix as usize > m.programs.len() {
        let _ = write!(s, "+{}", m.n_ix as usize - m.programs.len());
    }
    let _ = write!(
        s,
        " v0={} loaded={} inner={}",
        flag(m.lut),
        flag(m.loaded),
        flag(m.inner)
    );
    s
}

impl FeedHealth {
    pub fn new(src: FeedSrc, stale_ms: u64) -> FeedHealth {
        FeedHealth {
            src,
            connected: AtomicBool::new(false),
            frames: AtomicU64::new(0),
            events: AtomicU64::new(0),
            creates: AtomicU64::new(0),
            decode_miss: AtomicU64::new(0),
            decode_miss_kind: std::array::from_fn(|_| AtomicU64::new(0)),
            decode_errors: AtomicU64::new(0),
            failed_tx: AtomicU64::new(0),
            truncated: AtomicU64::new(0),
            reconnects: AtomicU64::new(0),
            stale_reconnects: AtomicU64::new(0),
            ring_drops: AtomicU64::new(0),
            rpc_calls: AtomicU64::new(0),
            rpc_errors: AtomicU64::new(0),
            rpc_drops: AtomicU64::new(0),
            last_frame_ns: AtomicU64::new(0),
            last_event_ns: AtomicU64::new(0),
            last_slot: AtomicU64::new(0),
            lag_ms: AtomicU64::new(u64::MAX),
            stale_ms: AtomicU64::new(stale_ms),
        }
    }

    #[inline]
    pub fn inc(c: &AtomicU64) {
        c.fetch_add(1, Relaxed);
    }

    /// Count one decode miss of `kind`; returns the 1-based ordinal of this
    /// miss at this feed (the running `decode_miss` total), which
    /// [`log_gate`] turns into "log it or not".
    #[inline]
    pub fn miss(&self, kind: MissKind) -> u64 {
        self.decode_miss_kind[kind as usize].fetch_add(1, Relaxed);
        self.decode_miss.fetch_add(1, Relaxed) + 1
    }

    /// Record a received frame now.
    #[inline]
    pub fn frame(&self) {
        Self::inc(&self.frames);
        self.last_frame_ns.store(mono_ns(), Relaxed);
    }

    /// Record a produced event.
    #[inline]
    pub fn event(&self, slot: u64) {
        Self::inc(&self.events);
        self.last_event_ns.store(mono_ns(), Relaxed);
        if slot > self.last_slot.load(Relaxed) {
            self.last_slot.store(slot, Relaxed);
        }
    }

    /// No frame for `stale_ms` while connected (or never connected since start).
    pub fn is_stale_at(&self, now_ns: u64) -> bool {
        let stale = self.stale_ms.load(Relaxed);
        if stale == 0 {
            return false;
        }
        let last = self.last_frame_ns.load(Relaxed);
        last != 0 && now_ns.saturating_sub(last) > stale * 1_000_000
    }

    pub fn snapshot(&self) -> HealthSnapshot {
        let now = mono_ns();
        let last = self.last_frame_ns.load(Relaxed);
        HealthSnapshot {
            src: self.src,
            connected: self.connected.load(Relaxed),
            frames: self.frames.load(Relaxed),
            events: self.events.load(Relaxed),
            creates: self.creates.load(Relaxed),
            decode_miss: self.decode_miss.load(Relaxed),
            decode_miss_kind: std::array::from_fn(|k| self.decode_miss_kind[k].load(Relaxed)),
            decode_errors: self.decode_errors.load(Relaxed),
            failed_tx: self.failed_tx.load(Relaxed),
            truncated: self.truncated.load(Relaxed),
            reconnects: self.reconnects.load(Relaxed),
            stale_reconnects: self.stale_reconnects.load(Relaxed),
            ring_drops: self.ring_drops.load(Relaxed),
            rpc_calls: self.rpc_calls.load(Relaxed),
            rpc_errors: self.rpc_errors.load(Relaxed),
            rpc_drops: self.rpc_drops.load(Relaxed),
            since_frame_ms: if last == 0 {
                u64::MAX
            } else {
                now.saturating_sub(last) / 1_000_000
            },
            last_slot: self.last_slot.load(Relaxed),
            lag_ms: self.lag_ms.load(Relaxed),
            stale: self.is_stale_at(now),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_detection_and_rates() {
        let h = FeedHealth::new(FeedSrc::LogsWs, 10);
        assert!(!h.is_stale_at(mono_ns()), "never received: not stale yet");
        h.frame();
        let t = h.last_frame_ns.load(Relaxed);
        assert!(!h.is_stale_at(t + 5_000_000));
        assert!(h.is_stale_at(t + 11_000_000));
        FeedHealth::inc(&h.creates);
        FeedHealth::inc(&h.creates);
        FeedHealth::inc(&h.creates);
        FeedHealth::inc(&h.decode_miss);
        h.event(42);
        h.event(41);
        let s = h.snapshot();
        assert_eq!(s.last_slot, 42);
        assert!((s.decode_success_rate() - 0.75).abs() < 1e-9);
    }

    #[test]
    fn log_gate_first_50_then_every_100th() {
        assert!((1..=50).all(log_gate));
        assert!((51..100).all(|n| !log_gate(n)));
        assert!(log_gate(100));
        assert!(!log_gate(101));
        assert!(!log_gate(199));
        assert!(log_gate(200));
        assert!(log_gate(1_000));
        assert!(!log_gate(1_001));
    }

    #[test]
    fn miss_counts_by_kind() {
        let h = FeedHealth::new(FeedSrc::Grpc, 0);
        assert_eq!(h.miss(MissKind::Eof), 1);
        assert_eq!(h.miss(MissKind::Eof), 2);
        assert_eq!(h.miss(MissKind::Logs), 3);
        FeedHealth::inc(&h.creates);
        let s = h.snapshot();
        assert_eq!(s.decode_miss, 3);
        assert_eq!(s.decode_miss_kind[MissKind::Eof as usize], 2);
        assert_eq!(s.decode_miss_kind[MissKind::Logs as usize], 1);
        assert_eq!(s.decode_miss_kind.iter().sum::<u64>(), s.decode_miss);
        assert!((s.decode_success_rate() - 0.25).abs() < 1e-9);
        assert_eq!(MissKind::ALL.len(), MISS_KIND_COUNT);
        for (i, k) in MissKind::ALL.iter().enumerate() {
            assert_eq!(*k as usize, i, "{}", k.name());
        }
        assert_eq!(
            MissKind::of_hit(&HitError::UnresolvedAccounts),
            MissKind::UnresolvedAccounts
        );
        assert_eq!(
            MissKind::of_hit(&HitError::Decode(DecodeError::InvalidBase64)),
            MissKind::InvalidBase64
        );
    }

    #[test]
    fn miss_line_has_every_field() {
        use pump_codec::constants::{MINT_AUTHORITY, PUMP_PROGRAM_ID};
        let mut programs = [Pubkey::default(); 4];
        programs[0] = PUMP_PROGRAM_ID;
        programs[1] = MINT_AUTHORITY;
        let m = MissInfo {
            slot: 123_456,
            sig: [7; 64],
            kind: MissKind::Eof,
            programs,
            n_programs: 2,
            n_ix: 6,
            lut: Some(true),
            loaded: Some(true),
            inner: Some(false),
            ordinal: 9,
        };
        let err = DecodeError::UnexpectedEof {
            offset: 12,
            needed: 4,
        };
        let line = miss_line("grpc", &m, &err);
        let sig = bs58::encode(&[7u8; 64][..]).into_string();
        for want in [
            "decode miss: src=grpc ",
            " n=9 ",
            " slot=123456 ",
            &format!(" sig={sig} "),
            " kind=eof ",
            " err=\"unexpected end of data at offset 12: 4 more byte(s) needed\" ",
            &format!(" programs={PUMP_PROGRAM_ID},{MINT_AUTHORITY}+2 "),
            " v0=1 loaded=1 inner=0",
        ] {
            assert!(line.contains(want), "{want:?} in {line:?}");
        }
        assert!(line.contains("programs=6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P,TSLvdd"));
        assert!(!line.contains('\n'));
        // unknown flags and a multi-line error stay one line
        let b = MissInfo::blank(MissKind::Logs);
        let line = miss_line("logsws", &b, &"a\nb\rc");
        assert!(
            line.ends_with(" programs= v0=? loaded=? inner=?"),
            "{line:?}"
        );
        assert!(line.contains("err=\"a b c\""));
        assert!(!line.contains('\n'));
    }
}
