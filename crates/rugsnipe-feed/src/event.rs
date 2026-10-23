//! Feed events: what every adapter produces. All payloads are `Copy` and
//! heap-free so they move through `rtrb` rings and [`crate::FeedBuf`] without
//! allocating.

use pump_codec::constants::WSOL_MINT;
use pump_codec::Pubkey;
use std::fmt;

/// Where an event came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum FeedSrc {
    /// `logsSubscribe` over a websocket (free; paper only).
    LogsWs = 0,
    /// PumpPortal `subscribeNewToken` (one connection per IP).
    PumpPortal = 1,
    /// Frames relayed by the collector over a Unix socket.
    Relay = 2,
    /// Recorded frames.
    Replay = 3,
    /// Yellowstone gRPC (feature `grpc`; post-execution, processed).
    Grpc = 4,
    /// Shreds (later).
    Shreds = 5,
    /// A second Yellowstone gRPC feed beside the first (`feeds.grpc_b`: a
    /// provider on trial). The same class as [`FeedSrc::Grpc`]; the
    /// first-seen report races the two copies of every create.
    GrpcB = 6,
    /// The entry stream of a shredstream proxy (Jito's `jito-shredstream-proxy`
    /// protocol: shreds from a block-engine style gateway, deshredded by the
    /// proxy and served as `ShredstreamProxy/SubscribeEntries`): every
    /// transaction of a slot before execution, the same class as
    /// [`FeedSrc::Shreds`]; the first-seen report races the two.
    ShredStream = 7,
}

/// Number of [`FeedSrc`] values.
pub const SRC_COUNT: usize = 8;

impl FeedSrc {
    pub const ALL: [FeedSrc; SRC_COUNT] = [
        FeedSrc::LogsWs,
        FeedSrc::PumpPortal,
        FeedSrc::Relay,
        FeedSrc::Replay,
        FeedSrc::Grpc,
        FeedSrc::Shreds,
        FeedSrc::GrpcB,
        FeedSrc::ShredStream,
    ];

    pub fn name(self) -> &'static str {
        match self {
            FeedSrc::LogsWs => "logsws",
            FeedSrc::PumpPortal => "pumpportal",
            FeedSrc::Relay => "relay",
            FeedSrc::Replay => "replay",
            FeedSrc::Grpc => "grpc",
            FeedSrc::Shreds => "shreds",
            FeedSrc::GrpcB => "grpc_b",
            FeedSrc::ShredStream => "shredstream",
        }
    }

    pub fn from_name(s: &str) -> Option<FeedSrc> {
        FeedSrc::ALL.iter().copied().find(|f| f.name() == s)
    }

    pub fn from_u8(v: u8) -> Option<FeedSrc> {
        FeedSrc::ALL.get(v as usize).copied()
    }

    /// A source that reports transactions before the chain executes them
    /// (shreds): its trades are quotes of what is about to run, not what
    /// ran. They feed the bundle gate only; a position marks itself on
    /// executed trades, and the mux never drops the executed copy of one
    /// of these as a duplicate.
    pub fn pre_exec(self) -> bool {
        matches!(self, FeedSrc::Shreds | FeedSrc::ShredStream)
    }
}

/// A transaction signature (64 raw bytes).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sig(pub [u8; 64]);

impl Default for Sig {
    fn default() -> Self {
        Sig([0; 64])
    }
}

impl Sig {
    /// Parse base58 (no heap for the result; bs58 decodes into the array).
    pub fn from_b58(s: &str) -> Option<Sig> {
        let mut out = [0u8; 64];
        match bs58::decode(s).onto(&mut out) {
            Ok(64) => Some(Sig(out)),
            _ => None,
        }
    }

    /// Base58 text (allocates: cold paths only).
    pub fn to_b58(&self) -> String {
        bs58::encode(&self.0).into_string()
    }

    pub fn is_zero(&self) -> bool {
        self.0 == [0; 64]
    }

    /// xxh3 of the signature.
    #[inline]
    pub fn hash(&self) -> u64 {
        xxhash_rust::xxh3::xxh3_64(&self.0)
    }
}

impl fmt::Debug for Sig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sig({})", self.to_b58())
    }
}

impl fmt::Display for Sig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_b58())
    }
}

/// Bits of [`CreateHitLite::flags`].
pub mod hit_flags {
    /// `creator` comes from the CreateEvent.
    pub const CREATOR_KNOWN: u16 = 1 << 0;
    /// `token_program` is known (CreateEvent field, or legacy `create` = SPL Token).
    pub const TOKEN_PROGRAM_KNOWN: u16 = 1 << 1;
    /// `quote_mint` is known.
    pub const QUOTE_KNOWN: u16 = 1 << 2;
    /// The runtime truncated the logs: in-tx buys may be missing.
    pub const LOGS_TRUNCATED: u16 = 1 << 3;
    /// Cashback-enabled coin.
    pub const CASHBACK: u16 = 1 << 4;
    /// Legacy `create` instruction (SPL Token mint).
    pub const LEGACY_CREATE: u16 = 1 << 5;
    /// Partial record from PumpPortal (no creator / token program / quote / slot).
    pub const PARTIAL: u16 = 1 << 6;
    /// `abc` was derived (needs a known token program).
    pub const ABC_KNOWN: u16 = 1 << 7;
}

/// A create seen on a free feed: the CreateEvent plus the same transaction's
/// TradeEvents (every pump buy of the new mint inside the create tx).
/// `Copy`, no heap (docs/06 section 7.1 "free mode"; the full `CreateHit` of
/// section 3 needs wire bytes for fee payer, signers and token owners).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CreateHitLite {
    pub mint: Pubkey,
    /// Bonding curve.
    pub bc: Pubkey,
    /// Associated bonding curve = ATA(bc, mint, token_program) (zero if unknown).
    pub abc: Pubkey,
    /// `CreateEvent.user`: the create's signer (the dev wallet that pays).
    pub user: Pubkey,
    /// `CreateEvent.creator` (fee creator; differs from `user` for launchers).
    pub creator: Pubkey,
    pub token_program: Pubkey,
    /// Quote mint; `Pubkey::default()` (system program) means SOL.
    pub quote_mint: Pubkey,
    pub mayhem: bool,
    pub slot: u64,
    pub sig: Sig,
    /// Sum of `sol_amount` (lamports into the curve, fees excluded) of the in-tx buys.
    pub intx_sol_in: u64,
    /// Sum of `token_amount` of the in-tx buys.
    pub intx_tokens_out: u64,
    /// Number of in-tx buys of the mint.
    pub intx_buys: u8,
    /// Virtual SOL after the last in-tx buy (0 if no buy and no reserve info).
    pub v_after: u64,
    /// [`hit_flags`].
    pub flags: u16,
}

impl CreateHitLite {
    /// SOL-quoted (quote unknown counts as not SOL).
    pub fn is_sol_quote(&self) -> bool {
        self.flags & hit_flags::QUOTE_KNOWN != 0
            && (self.quote_mint == Pubkey::default() || self.quote_mint == WSOL_MINT)
    }

    pub fn has(&self, flag: u16) -> bool {
        self.flags & flag != 0
    }
}

/// A funder sent lamports to a recipient (funding watcher, docs/06 section 9.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FundingEvent {
    pub funder: Pubkey,
    pub recipient: Pubkey,
    pub lamports: u64,
    pub slot: u64,
    pub sig: Sig,
    /// Recipient balance before the transaction (`u64::MAX` if unknown).
    pub recipient_pre_lamports: u64,
}

/// A pump trade on a watched mint (exit engine input).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TradeLite {
    pub mint: Pubkey,
    pub user: Pubkey,
    pub is_buy: bool,
    pub sol_amount: u64,
    pub token_amount: u64,
    /// Protocol + creator fee.
    pub fee: u64,
    pub virtual_sol_reserves: u64,
    pub virtual_token_reserves: u64,
    pub slot: u64,
    pub sig: Sig,
    /// Ordinal of this TradeEvent in its transaction (dedup key part).
    pub ordinal: u8,
    /// Transaction index in its block (`u32::MAX` when the feed gave none:
    /// the logs path). The bundle gate needs it: a bundle buy sits within
    /// `RUN_MAX_AFTER` indices after the create.
    pub index: u32,
}

/// Wire bytes of a transaction, stored in a [`crate::FeedBuf`] arena:
/// `len` bytes at `off`, the raw transaction first and, when `meta_len > 0`,
/// a post-execution meta trailer of `meta_len` bytes after it
/// ([`crate::wiremeta`]: transaction index, failure, fee, ALT-loaded
/// addresses, inner instructions; gRPC only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WireRef {
    pub off: u32,
    pub len: u32,
    pub meta_len: u32,
}

impl WireRef {
    /// Length of the raw transaction (without the meta trailer).
    #[inline]
    pub fn tx_len(&self) -> u32 {
        self.len.saturating_sub(self.meta_len)
    }
}

/// A duplicate that another feed delivered first (the original payload is
/// replaced; record it as `Dup{src, delay}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DupInfo {
    pub first_src: FeedSrc,
    /// `recv_ns - first recv_ns` (0 if this copy was not later).
    pub delay_ns: u64,
    pub key: u64,
}

/// The creation slot's bundle run of `mint` is complete: the stream showed
/// a pump transaction of the same slot at `index`, more than
/// [`crate::bundle::RUN_MAX_AFTER`] transactions after the create, so no
/// later transaction can be part of the bundle. The hot loop's bundle gate
/// fires a parked create on it, at the post-bundle state it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunDoneLite {
    pub mint: Pubkey,
    pub slot: u64,
    pub index: u32,
}

/// Payload of a [`FeedEvent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Payload {
    /// Raw transaction bytes (gRPC / shreds / recorded wire frames).
    Wire(WireRef),
    /// A create from a free feed.
    Hit(CreateHitLite),
    /// A funding transfer by a watched funder.
    Funding(FundingEvent),
    /// A trade on a watched mint.
    Trade(TradeLite),
    /// A recent create's bundle run is complete (gRPC `bundle_run`).
    RunDone(RunDoneLite),
    /// Duplicate of an earlier event.
    Dup(DupInfo),
}

/// One feed event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedEvent {
    pub src: FeedSrc,
    /// Monotonic receive time ([`crate::clock::mono_ns`]).
    pub recv_ns: u64,
    pub slot: u64,
    /// Alpenglow bank id (0 until the cutover).
    pub bank_id: u64,
    pub payload: Payload,
}

/// First signature of a raw transaction (compact-u16 count, then 64-byte signatures).
pub fn wire_first_sig(raw: &[u8]) -> Option<&[u8]> {
    let (&n, rest) = raw.split_first()?;
    if n == 0 || n & 0x80 != 0 {
        // 0 signatures, or >= 128 (not a valid transaction)
        return None;
    }
    rest.get(..64)
}

impl FeedEvent {
    /// Cross-feed dedup key: xxh3 of the signature, tagged by payload kind (and
    /// the event ordinal for trades). `wire` resolves [`Payload::Wire`].
    pub fn dedup_key(&self, wire: impl Fn(WireRef) -> Option<u64>) -> Option<u64> {
        const K_HIT: u64 = 0x9e37_79b9_7f4a_7c15;
        const K_FUND: u64 = 0xc2b2_ae3d_27d4_eb4f;
        const K_TRADE: u64 = 0x1656_67b1_9e37_79f9;
        let k = match &self.payload {
            Payload::Wire(w) => wire(*w)?,
            Payload::Hit(h) => {
                if h.sig.is_zero() {
                    return None;
                }
                h.sig.hash() ^ K_HIT
            }
            Payload::Funding(f) => {
                f.sig.hash() ^ K_FUND ^ xxhash_rust::xxh3::xxh3_64(f.recipient.as_ref())
            }
            Payload::Trade(t) => t.sig.hash() ^ K_TRADE.wrapping_mul(t.ordinal as u64 + 1),
            // one source, never a duplicate
            Payload::RunDone(_) | Payload::Dup(_) => return None,
        };
        Some(k)
    }

    /// The mint this event is about, if any.
    pub fn mint(&self) -> Option<Pubkey> {
        match &self.payload {
            Payload::Hit(h) => Some(h.mint),
            Payload::Trade(t) => Some(t.mint),
            Payload::RunDone(r) => Some(r.mint),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn src_names_roundtrip() {
        for s in FeedSrc::ALL {
            assert_eq!(FeedSrc::from_name(s.name()), Some(s));
            assert_eq!(FeedSrc::from_u8(s as u8), Some(s));
        }
        assert_eq!(FeedSrc::from_u8(6), Some(FeedSrc::GrpcB));
        assert_eq!(FeedSrc::from_u8(7), Some(FeedSrc::ShredStream));
        assert_eq!(FeedSrc::from_u8(8), None);
    }

    #[test]
    fn sig_b58_roundtrip() {
        let s = "5J7T55JQzepJ1UQF5rsgjav1SXWoKr8jzSJx2dDBTcidSy3nXUV8PBcqveo8X859JYUQesZh7TFUXd7bAPxJe5rg";
        let sig = Sig::from_b58(s).unwrap();
        assert_eq!(sig.to_b58(), s);
        assert!(Sig::from_b58("abc").is_none());
        assert!(Sig::from_b58("0OIl").is_none());
    }

    #[test]
    fn payload_is_small_and_copy() {
        fn is_copy<T: Copy>() {}
        is_copy::<FeedEvent>();
        assert!(std::mem::size_of::<FeedEvent>() <= 384);
    }

    #[test]
    fn wire_sig() {
        let mut raw = vec![1u8];
        raw.extend_from_slice(&[7u8; 64]);
        raw.extend_from_slice(&[0u8; 10]);
        assert_eq!(wire_first_sig(&raw), Some(&[7u8; 64][..]));
        assert_eq!(wire_first_sig(&[0]), None);
        assert_eq!(wire_first_sig(&[1, 2, 3]), None);
    }
}
