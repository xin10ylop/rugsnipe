//! The position record (docs/06 section 5.1 `positions` table) and the state
//! the decision function keeps between events.

use crate::bounds::{mult_bps, Market};
use crate::policy::ExitPolicy;
use crate::types::{b58, ExitReason, Origin, SellOrder, Venue};
use pump_codec::curve::FeeBps;
use pump_codec::Pubkey;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Maximum bundle wallets tracked per position.
pub const MAX_BUNDLE: usize = 16;
/// Maximum sell orders in flight at once (a partial take-profit plus a dump of the rest).
pub const MAX_PENDING: usize = 2;
/// Transfers remembered for the consolidation detector.
pub const XFER_RING: usize = 32;

/// Lifecycle. `Pending` is the pre-open phase (intent sent, nothing landed);
/// the `positions` table only ever holds `open | exiting | closed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PositionState {
    Pending,
    Open,
    Exiting,
    Closed,
}

impl PositionState {
    /// `open`/`exiting`: we hold tokens and own the exit.
    pub fn is_active(self) -> bool {
        matches!(self, PositionState::Open | PositionState::Exiting)
    }
}

/// The bundle set: in-tx buyers and their token owners, slot-0 adjacent and
/// shared-tip signers, creator / signer / fee payer, funding-linked wallets.
/// Fixed capacity (no heap); serialized as a list of base58 strings.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct BundleSet {
    keys: [Pubkey; MAX_BUNDLE],
    len: u8,
}

impl BundleSet {
    /// Up to [`MAX_BUNDLE`] distinct keys (extra keys are ignored).
    pub fn new(keys: &[Pubkey]) -> Self {
        let mut s = BundleSet::default();
        for k in keys {
            s.insert(*k);
        }
        s
    }

    /// Add a key; `false` if full or already present.
    pub fn insert(&mut self, k: Pubkey) -> bool {
        if self.contains(&k) || self.len as usize >= MAX_BUNDLE {
            return false;
        }
        self.keys[self.len as usize] = k;
        self.len += 1;
        true
    }

    pub fn contains(&self, k: &Pubkey) -> bool {
        self.as_slice().contains(k)
    }

    pub fn as_slice(&self) -> &[Pubkey] {
        &self.keys[..self.len as usize]
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl fmt::Debug for BundleSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.as_slice()).finish()
    }
}

impl Serialize for BundleSet {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let v: Vec<String> = self.as_slice().iter().map(|k| k.to_string()).collect();
        v.serialize(s)
    }
}

impl<'de> Deserialize<'de> for BundleSet {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v: Vec<String> = Vec::deserialize(d)?;
        let mut s = BundleSet::default();
        for k in v {
            s.insert(k.parse().map_err(serde::de::Error::custom)?);
        }
        Ok(s)
    }
}

/// A sell order in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingSell {
    /// The last attempt sent.
    pub order: SellOrder,
    /// Virtual token reserve at the trigger (normal bounds keep the trigger state).
    pub t_trigger: u64,
    /// Slot of the last attempt (next retry at `sent_slot + every_slots`).
    pub sent_slot: u64,
    /// No more retries (max tries or spend cap reached); alert raised.
    pub exhausted: bool,
}

/// A remembered transfer (consolidation window).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct XferRec {
    #[serde(with = "b58")]
    pub to_owner: Pubkey,
    pub amount: u64,
    pub slot: u64,
}

/// One position. Fields mirror the `positions` table (`mint, intent_id,
/// opened_slot, tokens, cost, v_fill, peak_v, bundle_json, policy_json, origin,
/// state, sell_spend, closed_slot`) plus the runtime state of the triggers.
/// `Copy`, no heap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    #[serde(with = "b58")]
    pub mint: Pubkey,
    /// Our wallet (signer of our trades).
    #[serde(with = "b58")]
    pub owner: Pubkey,
    pub intent_id: u64,
    pub origin: Option<Origin>,
    pub state: PositionState,
    pub policy: ExitPolicy,
    pub bundle: BundleSet,
    /// Curve creator (fee recipient; zero key = no creator fee).
    #[serde(with = "b58")]
    pub creator: Pubkey,
    pub fee_protocol_bps: u64,
    pub fee_creator_bps: u64,
    /// Token supply in base units (1e15; 2e15 for mayhem coins).
    pub supply: u64,
    pub venue: Venue,
    pub opened_slot: u64,
    pub opened_ms: u64,
    /// Chain-derived token balance (base units).
    pub tokens: u64,
    /// Lamports paid for the tokens (curve amount + fees; 0 for orphans).
    pub cost: u64,
    /// Net lamports received from our sells.
    pub proceeds: u64,
    pub v_fill: u64,
    pub t_fill: u64,
    pub peak_v: u64,
    pub peak_mult_bps: u64,
    /// Latest reserves (curve virtual or pool) and the slot they belong to.
    pub cur_v: u64,
    pub cur_t: u64,
    pub cur_slot: u64,
    pub last_trade_ms: u64,
    /// Sum of sell tips committed (upper bound of what sells cost in tips).
    pub sell_spend: u64,
    pub closed_slot: Option<u64>,
    /// Bit i set once take-profit level i fired.
    pub tp_done: u8,
    pub trail_armed: bool,
    /// First dump trigger seen (the position then only sells at dump bounds).
    pub dumped: Option<ExitReason>,
    pub dump_signalled: bool,
    pub pending: [Option<PendingSell>; MAX_PENDING],
    pub next_order_id: u32,
    /// Pool reserves came from a pool event (not the completion estimate).
    pub pool_known: bool,
    pub last_fill_reason: Option<ExitReason>,
    pub xfers: [XferRec; XFER_RING],
    pub xfer_head: u8,
}

impl Position {
    /// A position waiting for its first fill (intent sent). It opens on our own
    /// buy `TradeEvent` or a non-zero balance, never on a send.
    pub fn pending(
        mint: Pubkey,
        owner: Pubkey,
        intent_id: u64,
        policy: ExitPolicy,
        bundle: BundleSet,
        creator: Pubkey,
        fees: FeeBps,
        supply: u64,
    ) -> Self {
        Position {
            mint,
            owner,
            intent_id,
            origin: None,
            state: PositionState::Pending,
            policy,
            bundle,
            creator,
            fee_protocol_bps: fees.protocol,
            fee_creator_bps: fees.creator,
            supply,
            venue: Venue::Curve,
            opened_slot: 0,
            opened_ms: 0,
            tokens: 0,
            cost: 0,
            proceeds: 0,
            v_fill: 0,
            t_fill: 0,
            peak_v: 0,
            peak_mult_bps: 0,
            cur_v: 0,
            cur_t: 0,
            cur_slot: 0,
            last_trade_ms: 0,
            sell_spend: 0,
            closed_slot: None,
            tp_done: 0,
            trail_armed: false,
            dumped: None,
            dump_signalled: false,
            pending: [None; MAX_PENDING],
            next_order_id: 1,
            pool_known: false,
            last_fill_reason: None,
            xfers: [XferRec::default(); XFER_RING],
            xfer_head: 0,
        }
    }

    pub fn fees(&self) -> FeeBps {
        FeeBps {
            protocol: self.fee_protocol_bps,
            creator: self.fee_creator_bps,
        }
    }

    /// Current market state for quotes.
    pub fn market(&self) -> Market {
        Market {
            venue: self.venue,
            v: self.cur_v,
            t: self.cur_t,
            creator: self.creator,
            fees: self.fees(),
        }
    }

    /// Current price multiple versus the fill (bps).
    pub fn mult_bps(&self) -> u64 {
        mult_bps(self.venue, self.cur_v, self.cur_t, self.v_fill, self.t_fill)
    }

    /// Tokens committed to orders in flight.
    pub fn inflight_tokens(&self) -> u64 {
        self.pending
            .iter()
            .flatten()
            .map(|p| p.order.tokens)
            .fold(0u64, |a, b| a.saturating_add(b))
    }

    /// Tokens not committed to any order.
    pub fn available_tokens(&self) -> u64 {
        self.tokens.saturating_sub(self.inflight_tokens())
    }

    pub fn has_pending(&self) -> bool {
        self.pending.iter().any(|p| p.is_some())
    }

    /// Realized P&L so far (proceeds - cost), lamports.
    pub fn realized_lamports(&self) -> i128 {
        self.proceeds as i128 - self.cost as i128
    }

    /// `bundle_json` column.
    pub fn bundle_json(&self) -> String {
        serde_json::to_string(&self.bundle).unwrap_or_default()
    }

    /// `policy_json` column.
    pub fn policy_json(&self) -> String {
        serde_json::to_string(&self.policy).unwrap_or_default()
    }
}
