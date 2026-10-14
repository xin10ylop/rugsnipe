//! rslog: the hot box's binary decision log (docs/06 section 5.3).
//!
//! Frame = 30-byte header + payload, little-endian:
//!
//! `magic u32 = 0x52534C47 | kind u16 | ver u16 | len u32 | mono_ns u64 |
//! wall_ns u64 | box u16`
//!
//! Payload layouts are fixed per kind and listed in `schema/rslog.toml`
//! (the test suite checks the encoder against it and against the golden
//! file `fixtures/golden.rslog`). The hot thread pushes [`Rec`]s into a
//! wait-free SPSC ring ([`rtrb`]); [`RslogWriter`] drains it on its own
//! thread into hourly files `rslog-<box>-<unix_hour>.bin`. A full ring drops
//! the record and counts it (the hot path never blocks on disk).

use pump_codec::create_hit::{CreateHit, HitSource};
use pump_codec::Pubkey;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const MAGIC: u32 = 0x5253_4C47;
pub const HEADER_LEN: usize = 30;
/// Payload schema version written for every kind.
pub const VER: u16 = 1;

/// Frame header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub kind: u16,
    pub ver: u16,
    pub len: u32,
    pub mono_ns: u64,
    pub wall_ns: u64,
    pub box_id: u16,
}

impl Header {
    pub fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&MAGIC.to_le_bytes());
        out.extend_from_slice(&self.kind.to_le_bytes());
        out.extend_from_slice(&self.ver.to_le_bytes());
        out.extend_from_slice(&self.len.to_le_bytes());
        out.extend_from_slice(&self.mono_ns.to_le_bytes());
        out.extend_from_slice(&self.wall_ns.to_le_bytes());
        out.extend_from_slice(&self.box_id.to_le_bytes());
    }
    pub fn decode(b: &[u8]) -> Option<Self> {
        if b.len() < HEADER_LEN || u32::from_le_bytes(b[0..4].try_into().ok()?) != MAGIC {
            return None;
        }
        Some(Self {
            kind: u16::from_le_bytes(b[4..6].try_into().ok()?),
            ver: u16::from_le_bytes(b[6..8].try_into().ok()?),
            len: u32::from_le_bytes(b[8..12].try_into().ok()?),
            mono_ns: u64::from_le_bytes(b[12..20].try_into().ok()?),
            wall_ns: u64::from_le_bytes(b[20..28].try_into().ok()?),
            box_id: u16::from_le_bytes(b[28..30].try_into().ok()?),
        })
    }
}

/// CreateHit flags (`CreateHitRec.flags`).
pub mod hit_flags {
    pub const MAYHEM: u16 = 1;
    pub const CASHBACK: u16 = 2;
    pub const SOL_QUOTE: u16 = 4;
    pub const EXACT: u16 = 8;
    pub const LOGS_SOURCE: u16 = 16;
    pub const LOGS_TRUNCATED: u16 = 32;
    pub const HOLDER_REWARD: u16 = 64;
    pub const TOKEN_2022: u16 = 128;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreateHitRec {
    pub src: u8,
    pub slot: u64,
    pub sig: [u8; 64],
    pub mint: Pubkey,
    pub user: Pubkey,
    /// Zero key when unknown (logs path).
    pub fee_payer: Pubkey,
    pub creator: Pubkey,
    /// Zero key for a direct create.
    pub launcher: Pubkey,
    pub intx_sol_in: u64,
    pub intx_tokens_out: u64,
    pub intx_buys: u8,
    pub n_signers: u8,
    pub flags: u16,
}

impl CreateHitRec {
    /// Snapshot a [`CreateHit`] (`src` = feed id).
    pub fn from_hit(src: u8, h: &CreateHit) -> Self {
        use hit_flags::*;
        let mut flags = 0;
        for (b, f) in [
            (h.mayhem, MAYHEM),
            (h.cashback, CASHBACK),
            (h.is_sol_quote(), SOL_QUOTE),
            (h.intx_exact, EXACT),
            (h.source == HitSource::Logs, LOGS_SOURCE),
            (h.logs_truncated, LOGS_TRUNCATED),
            (h.holder_reward, HOLDER_REWARD),
            (
                h.token_prog == pump_codec::constants::TOKEN_2022_PROGRAM_ID,
                TOKEN_2022,
            ),
        ] {
            if b {
                flags |= f;
            }
        }
        Self {
            src,
            slot: h.slot.unwrap_or(0),
            sig: h.sig,
            mint: h.mint,
            user: h.user,
            fee_payer: h.fee_payer.unwrap_or_default(),
            creator: h.creator_arg,
            launcher: h.launcher.program_id().unwrap_or_default(),
            intx_sol_in: h.intx_sol_in,
            intx_tokens_out: h.intx_tokens_out,
            intx_buys: h.intx_buys,
            n_signers: h.signers.len() as u8,
            flags,
        }
    }
}

/// One record (fixed-size payloads; `Copy` so the ring never allocates).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rec {
    FeedSeen {
        src: u8,
        slot: u64,
        recv_ns: u64,
        sig: [u8; 64],
    },
    CreateHit(CreateHitRec),
    Matched {
        intent: u64,
        op_id: u32,
        armset_ver: u64,
        via: u8,
        shadow: u8,
    },
    Decision {
        intent: u64,
        mint: Pubkey,
        mode: u8,
        code: u8,
        would_skip: u8,
        size: u64,
        v_max: u64,
        min_tokens: u64,
        nonce_idx: u16,
        decided_ns: u64,
    },
    Sent {
        intent: u64,
        variant: u8,
        sender: u8,
        prio: u64,
        tip: u64,
        sig: [u8; 64],
        result: u8,
        latency_ns: u64,
    },
    PosEvent {
        intent: u64,
        kind: u8,
        slot: u64,
        v: u64,
        tokens: u64,
    },
    ExitFired {
        intent: u64,
        reason: u8,
        tokens: u64,
        min_out: u64,
        sig: [u8; 64],
    },
    Risk {
        kind: u8,
        value: i64,
    },
    FeedHealth {
        src: u8,
        ok: u8,
        lag_ms: u32,
        decode_miss_ppm: u32,
    },
    Lease {
        name_hash: u32,
        owner: u16,
        epoch: u64,
        hb_unix_ms: u64,
    },
    Upgrade {
        program: Pubkey,
        slot: u64,
    },
    NonceEvt {
        idx: u16,
        from: u8,
        to: u8,
        slot: u64,
        value: [u8; 32],
        intent: u64,
    },
}

/// `(kind id, name)` of every record, in id order.
pub const KINDS: [(u16, &str); 12] = [
    (1, "FeedSeen"),
    (2, "CreateHit"),
    (3, "Matched"),
    (4, "Decision"),
    (5, "Sent"),
    (6, "PosEvent"),
    (7, "ExitFired"),
    (8, "Risk"),
    (9, "FeedHealth"),
    (10, "Lease"),
    (11, "Upgrade"),
    (12, "NonceEvt"),
];

struct W<'a>(&'a mut Vec<u8>);
impl W<'_> {
    fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u64(&mut self, v: u64) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn i64(&mut self, v: i64) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn b(&mut self, v: &[u8]) -> &mut Self {
        self.0.extend_from_slice(v);
        self
    }
    fn pk(&mut self, k: &Pubkey) -> &mut Self {
        self.b(k.as_ref())
    }
}

struct R<'a> {
    b: &'a [u8],
    o: usize,
}
impl R<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let v = self.b.get(self.o..self.o + N)?.try_into().ok()?;
        self.o += N;
        Some(v)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take::<1>()?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take().map(u16::from_le_bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        self.take().map(u32::from_le_bytes)
    }
    fn u64(&mut self) -> Option<u64> {
        self.take().map(u64::from_le_bytes)
    }
    fn i64(&mut self) -> Option<i64> {
        self.take().map(i64::from_le_bytes)
    }
    fn pk(&mut self) -> Option<Pubkey> {
        self.take::<32>().map(Pubkey::new_from_array)
    }
}

impl Rec {
    pub fn kind(&self) -> u16 {
        match self {
            Self::FeedSeen { .. } => 1,
            Self::CreateHit(_) => 2,
            Self::Matched { .. } => 3,
            Self::Decision { .. } => 4,
            Self::Sent { .. } => 5,
            Self::PosEvent { .. } => 6,
            Self::ExitFired { .. } => 7,
            Self::Risk { .. } => 8,
            Self::FeedHealth { .. } => 9,
            Self::Lease { .. } => 10,
            Self::Upgrade { .. } => 11,
            Self::NonceEvt { .. } => 12,
        }
    }

    /// Append the payload bytes.
    pub fn encode_payload(&self, out: &mut Vec<u8>) {
        let mut w = W(out);
        match self {
            Self::FeedSeen {
                src,
                slot,
                recv_ns,
                sig,
            } => {
                w.u8(*src).u64(*slot).u64(*recv_ns).b(sig);
            }
            Self::CreateHit(h) => {
                w.u8(h.src)
                    .u64(h.slot)
                    .b(&h.sig)
                    .pk(&h.mint)
                    .pk(&h.user)
                    .pk(&h.fee_payer)
                    .pk(&h.creator)
                    .pk(&h.launcher)
                    .u64(h.intx_sol_in)
                    .u64(h.intx_tokens_out)
                    .u8(h.intx_buys)
                    .u8(h.n_signers)
                    .u16(h.flags);
            }
            Self::Matched {
                intent,
                op_id,
                armset_ver,
                via,
                shadow,
            } => {
                w.u64(*intent)
                    .u32(*op_id)
                    .u64(*armset_ver)
                    .u8(*via)
                    .u8(*shadow);
            }
            Self::Decision {
                intent,
                mint,
                mode,
                code,
                would_skip,
                size,
                v_max,
                min_tokens,
                nonce_idx,
                decided_ns,
            } => {
                w.u64(*intent)
                    .pk(mint)
                    .u8(*mode)
                    .u8(*code)
                    .u8(*would_skip)
                    .u64(*size)
                    .u64(*v_max)
                    .u64(*min_tokens)
                    .u16(*nonce_idx)
                    .u64(*decided_ns);
            }
            Self::Sent {
                intent,
                variant,
                sender,
                prio,
                tip,
                sig,
                result,
                latency_ns,
            } => {
                w.u64(*intent)
                    .u8(*variant)
                    .u8(*sender)
                    .u64(*prio)
                    .u64(*tip)
                    .b(sig)
                    .u8(*result)
                    .u64(*latency_ns);
            }
            Self::PosEvent {
                intent,
                kind,
                slot,
                v,
                tokens,
            } => {
                w.u64(*intent).u8(*kind).u64(*slot).u64(*v).u64(*tokens);
            }
            Self::ExitFired {
                intent,
                reason,
                tokens,
                min_out,
                sig,
            } => {
                w.u64(*intent).u8(*reason).u64(*tokens).u64(*min_out).b(sig);
            }
            Self::Risk { kind, value } => {
                w.u8(*kind).i64(*value);
            }
            Self::FeedHealth {
                src,
                ok,
                lag_ms,
                decode_miss_ppm,
            } => {
                w.u8(*src).u8(*ok).u32(*lag_ms).u32(*decode_miss_ppm);
            }
            Self::Lease {
                name_hash,
                owner,
                epoch,
                hb_unix_ms,
            } => {
                w.u32(*name_hash).u16(*owner).u64(*epoch).u64(*hb_unix_ms);
            }
            Self::Upgrade { program, slot } => {
                w.pk(program).u64(*slot);
            }
            Self::NonceEvt {
                idx,
                from,
                to,
                slot,
                value,
                intent,
            } => {
                w.u16(*idx)
                    .u8(*from)
                    .u8(*to)
                    .u64(*slot)
                    .b(value)
                    .u64(*intent);
            }
        }
    }

    /// Decode a payload of `kind`; `None` if unknown or malformed.
    pub fn decode_payload(kind: u16, b: &[u8]) -> Option<Self> {
        let mut r = R { b, o: 0 };
        let rec = match kind {
            1 => Self::FeedSeen {
                src: r.u8()?,
                slot: r.u64()?,
                recv_ns: r.u64()?,
                sig: r.take()?,
            },
            2 => Self::CreateHit(CreateHitRec {
                src: r.u8()?,
                slot: r.u64()?,
                sig: r.take()?,
                mint: r.pk()?,
                user: r.pk()?,
                fee_payer: r.pk()?,
                creator: r.pk()?,
                launcher: r.pk()?,
                intx_sol_in: r.u64()?,
                intx_tokens_out: r.u64()?,
                intx_buys: r.u8()?,
                n_signers: r.u8()?,
                flags: r.u16()?,
            }),
            3 => Self::Matched {
                intent: r.u64()?,
                op_id: r.u32()?,
                armset_ver: r.u64()?,
                via: r.u8()?,
                shadow: r.u8()?,
            },
            4 => Self::Decision {
                intent: r.u64()?,
                mint: r.pk()?,
                mode: r.u8()?,
                code: r.u8()?,
                would_skip: r.u8()?,
                size: r.u64()?,
                v_max: r.u64()?,
                min_tokens: r.u64()?,
                nonce_idx: r.u16()?,
                decided_ns: r.u64()?,
            },
            5 => Self::Sent {
                intent: r.u64()?,
                variant: r.u8()?,
                sender: r.u8()?,
                prio: r.u64()?,
                tip: r.u64()?,
                sig: r.take()?,
                result: r.u8()?,
                latency_ns: r.u64()?,
            },
            6 => Self::PosEvent {
                intent: r.u64()?,
                kind: r.u8()?,
                slot: r.u64()?,
                v: r.u64()?,
                tokens: r.u64()?,
            },
            7 => Self::ExitFired {
                intent: r.u64()?,
                reason: r.u8()?,
                tokens: r.u64()?,
                min_out: r.u64()?,
                sig: r.take()?,
            },
            8 => Self::Risk {
                kind: r.u8()?,
                value: r.i64()?,
            },
            9 => Self::FeedHealth {
                src: r.u8()?,
                ok: r.u8()?,
                lag_ms: r.u32()?,
                decode_miss_ppm: r.u32()?,
            },
            10 => Self::Lease {
                name_hash: r.u32()?,
                owner: r.u16()?,
                epoch: r.u64()?,
                hb_unix_ms: r.u64()?,
            },
            11 => Self::Upgrade {
                program: r.pk()?,
                slot: r.u64()?,
            },
            12 => Self::NonceEvt {
                idx: r.u16()?,
                from: r.u8()?,
                to: r.u8()?,
                slot: r.u64()?,
                value: r.take()?,
                intent: r.u64()?,
            },
            _ => return None,
        };
        (r.o == b.len()).then_some(rec)
    }

    /// Full frame (header + payload).
    pub fn encode_frame(&self, mono_ns: u64, wall_ns: u64, box_id: u16, out: &mut Vec<u8>) {
        let start = out.len();
        out.resize(start + HEADER_LEN, 0);
        self.encode_payload(out);
        let len = (out.len() - start - HEADER_LEN) as u32;
        let mut h = Vec::with_capacity(HEADER_LEN);
        Header {
            kind: self.kind(),
            ver: VER,
            len,
            mono_ns,
            wall_ns,
            box_id,
        }
        .encode(&mut h);
        out[start..start + HEADER_LEN].copy_from_slice(&h);
    }
}

/// Decode a byte stream of frames. Stops at the first malformed frame and
/// returns what was read plus the byte offset where it stopped.
pub fn decode_frames(b: &[u8]) -> (Vec<(Header, Rec)>, usize) {
    let mut out = vec![];
    let mut o = 0;
    while let Some(h) = Header::decode(&b[o..]) {
        let end = o + HEADER_LEN + h.len as usize;
        let Some(p) = b.get(o + HEADER_LEN..end) else {
            break;
        };
        let Some(r) = Rec::decode_payload(h.kind, p) else {
            break;
        };
        out.push((h, r));
        o = end;
    }
    (out, o)
}

fn mono_base() -> Instant {
    static BASE: OnceLock<Instant> = OnceLock::new();
    *BASE.get_or_init(Instant::now)
}

/// Monotonic ns since process start (comparable within one process).
pub fn mono_ns() -> u64 {
    mono_base().elapsed().as_nanos() as u64
}

/// Wall-clock ns since the Unix epoch.
pub fn wall_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy)]
struct Stamped {
    mono_ns: u64,
    wall_ns: u64,
    rec: Rec,
}

/// Hot-side handle: wait-free `push`.
pub struct RslogProducer {
    tx: rtrb::Producer<Stamped>,
    dropped: Arc<AtomicU64>,
}

impl core::fmt::Debug for RslogProducer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RslogProducer")
            .field("dropped", &self.dropped())
            .finish()
    }
}

impl RslogProducer {
    /// Stamp and enqueue; returns `false` (and counts a drop) if the ring is full.
    pub fn push(&mut self, rec: Rec) -> bool {
        let s = Stamped {
            mono_ns: mono_ns(),
            wall_ns: wall_ns(),
            rec,
        };
        if self.tx.push(s).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            false
        } else {
            true
        }
    }
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// Writer thread handle; join it after dropping the producer.
#[derive(Debug)]
pub struct RslogWriter {
    handle: std::thread::JoinHandle<std::io::Result<u64>>,
}

impl RslogWriter {
    /// Start the writer thread. Files go to `dir/rslog-<box_name>-<unix_hour>.bin`.
    pub fn spawn(
        dir: impl AsRef<Path>,
        box_name: &str,
        box_id: u16,
        capacity: usize,
    ) -> std::io::Result<(RslogProducer, Self)> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        let (tx, mut rx) = rtrb::RingBuffer::<Stamped>::new(capacity);
        let dropped = Arc::new(AtomicU64::new(0));
        let name = box_name.to_string();
        let handle = std::thread::Builder::new()
            .name("rslog-writer".into())
            .spawn(move || -> std::io::Result<u64> {
                let mut written = 0u64;
                let mut cur: Option<(u64, std::io::BufWriter<std::fs::File>)> = None;
                let mut buf = Vec::with_capacity(512);
                loop {
                    let mut n = 0;
                    while let Ok(s) = rx.pop() {
                        let hour = s.wall_ns / 3_600_000_000_000;
                        if cur.as_ref().map(|c| c.0) != Some(hour) {
                            if let Some((_, mut f)) = cur.take() {
                                f.flush()?;
                            }
                            let p: PathBuf = dir.join(format!("rslog-{name}-{hour}.bin"));
                            let f = std::fs::OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(p)?;
                            cur = Some((hour, std::io::BufWriter::with_capacity(1 << 16, f)));
                        }
                        buf.clear();
                        s.rec.encode_frame(s.mono_ns, s.wall_ns, box_id, &mut buf);
                        cur.as_mut().unwrap().1.write_all(&buf)?;
                        written += 1;
                        n += 1;
                    }
                    if n == 0 {
                        if let Some((_, f)) = cur.as_mut() {
                            f.flush()?;
                        }
                        if rx.is_abandoned() && rx.is_empty() {
                            return Ok(written);
                        }
                        std::thread::sleep(Duration::from_millis(2));
                    }
                }
            })?;
        Ok((RslogProducer { tx, dropped }, Self { handle }))
    }

    /// Wait for the writer to drain (drop the producer first). Returns records written.
    pub fn join(self) -> std::io::Result<u64> {
        self.handle
            .join()
            .map_err(|_| std::io::Error::other("rslog writer panicked"))?
    }
}

/// Stable 16-bit box id from its name (header `box`).
pub fn box_id(name: &str) -> u16 {
    (xxhash_rust::xxh3::xxh3_64(name.as_bytes()) & 0xffff) as u16
}
