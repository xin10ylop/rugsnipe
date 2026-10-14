//! Inputs ([`ExitEvent`]) and outputs ([`ExitAction`]) of the decision function.

use pump_codec::events::TradeEvent;
use pump_codec::Pubkey;
use serde::{Deserialize, Serialize};

/// Base58 (de)serialization of `Pubkey` (the JSON form shared with Python).
pub mod b58 {
    use pump_codec::Pubkey;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(k: &Pubkey, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&k.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Pubkey, D::Error> {
        let s = String::deserialize(d)?;
        s.parse::<Pubkey>().map_err(serde::de::Error::custom)
    }
}

/// Where a sell executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Venue {
    /// pump bonding curve (`sell_v3`).
    Curve,
    /// Canonical PumpSwap pool after migration (`AmmTrade` sell).
    PumpSwap,
}

/// The clock of a decision: wall/monotonic milliseconds and the current slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Now {
    pub ms: u64,
    pub slot: u64,
}

/// A trade on the position's mint (pump `TradeEvent`, or a PumpSwap swap after migration).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trade {
    pub venue: Venue,
    #[serde(with = "b58")]
    pub mint: Pubkey,
    /// Trader (signer). Our own fills have `user == Position::owner`.
    #[serde(with = "b58")]
    pub user: Pubkey,
    pub is_buy: bool,
    /// Curve: `TradeEvent.sol_amount` (lamports into/out of the curve, fees excluded).
    /// PumpSwap: quote amount.
    pub sol_amount: u64,
    pub token_amount: u64,
    /// Fees paid on top (buy) or deducted (sell); used for our own cost/proceeds.
    pub fee_lamports: u64,
    /// Post-trade virtual SOL (curve) or pool quote reserve (PumpSwap).
    pub reserve_sol: u64,
    /// Post-trade virtual token (curve) or pool base reserve (PumpSwap).
    pub reserve_token: u64,
    pub slot: u64,
}

impl Trade {
    /// From a decoded pump `TradeEvent` (bonding curve venue).
    pub fn from_event(ev: &TradeEvent<'_>, slot: u64) -> Self {
        Trade {
            venue: Venue::Curve,
            mint: ev.mint,
            user: ev.user,
            is_buy: ev.is_buy,
            sol_amount: ev.sol_amount,
            token_amount: ev.token_amount,
            fee_lamports: ev.fee.unwrap_or(0).saturating_add(ev.creator_fee.unwrap_or(0)),
            reserve_sol: ev.virtual_sol_reserves,
            reserve_token: ev.virtual_token_reserves,
            slot,
        }
    }
}

/// A token transfer of the mint between owners (Token-2022 `TransferChecked`
/// out of a bundle wallet's ATA; paid feeds only, docs/06 section 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferEvent {
    #[serde(with = "b58")]
    pub mint: Pubkey,
    #[serde(with = "b58")]
    pub from_owner: Pubkey,
    #[serde(with = "b58")]
    pub to_owner: Pubkey,
    pub amount: u64,
    pub slot: u64,
}

/// Everything the exit engine reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExitEvent {
    /// A trade on the mint (ours or anyone's).
    Trade(Trade),
    /// Chain balance of our token account (authoritative size).
    Balance { tokens: u64, slot: u64 },
    /// A transfer of the mint (consolidation detector input).
    Transfer(TransferEvent),
    /// `CompleteEvent`: the curve sold out; sells move to PumpSwap.
    Complete { slot: u64 },
    /// Pool reserves after migration (from a PumpSwap event or an account read).
    PoolState {
        reserve_sol: u64,
        reserve_token: u64,
        slot: u64,
    },
    /// A sender reported a definitive failure of the order's last attempt.
    SellFailed { order_id: u32 },
    /// Timer: evaluates slot closes, time triggers and retries.
    Tick,
}

impl ExitEvent {
    /// Slot carried by the event, if any.
    pub fn slot(&self) -> Option<u64> {
        match self {
            ExitEvent::Trade(t) => Some(t.slot),
            ExitEvent::Balance { slot, .. }
            | ExitEvent::Complete { slot }
            | ExitEvent::PoolState { slot, .. } => Some(*slot),
            ExitEvent::Transfer(t) => Some(t.slot),
            ExitEvent::SellFailed { .. } | ExitEvent::Tick => None,
        }
    }
}

/// Why a sell (or dump signal) happened. Dump reasons are [`SellKind::Dump`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitReason {
    BundleSell,
    Consolidation,
    BigBuy,
    /// Take-profit; the value is the highest ladder index crossed (0-based).
    TakeProfit(u8),
    Trailing,
    PhantomSl,
    NoActivity,
    MaxHold,
}

impl ExitReason {
    /// Dump triggers (1-3 in docs/06 section 8) sell 100% at floor bounds immediately.
    pub fn is_dump(self) -> bool {
        matches!(
            self,
            ExitReason::BundleSell | ExitReason::Consolidation | ExitReason::BigBuy
        )
    }

    /// Short label (`"tp1"` for the second level, ...).
    pub fn label(self) -> String {
        match self {
            ExitReason::BundleSell => "bundle_sell".into(),
            ExitReason::Consolidation => "consolidation".into(),
            ExitReason::BigBuy => "big_buy".into(),
            ExitReason::TakeProfit(i) => format!("tp{i}"),
            ExitReason::Trailing => "trailing".into(),
            ExitReason::PhantomSl => "phantom_sl".into(),
            ExitReason::NoActivity => "no_activity".into(),
            ExitReason::MaxHold => "max_hold".into(),
        }
    }
}

/// Bound family of a sell order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SellKind {
    /// `quote_sell(tokens, V_trigger) x (1 - tp_slip)`.
    Normal,
    /// `quote_sell(tokens, V = 30 SOL) x 0.98`: never fails on price.
    Dump,
}

/// One sell attempt to build and send.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SellOrder {
    /// Stable per position; retries reuse it (and the same sell nonce, so only one attempt lands).
    pub order_id: u32,
    pub reason: ExitReason,
    pub kind: SellKind,
    pub venue: Venue,
    pub tokens: u64,
    pub min_sol_output: u64,
    pub tip_lamports: u64,
    /// 1 for the first attempt.
    pub try_no: u32,
    /// Slot whose state fired the trigger.
    pub trigger_slot: u64,
    /// Virtual SOL (or pool quote) the normal bound was computed from.
    pub v_trigger: u64,
    /// Final sell: blockhash + `CloseAccount` (recovers the ATA rent). Partial sells use a sell nonce.
    pub close_account: bool,
}

/// How a position was opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Our own buy TradeEvent (an intent of ours landed).
    Intent,
    /// A non-zero token balance with no landed intent (recovery, late fill).
    Orphan,
}

/// Why a position closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "by", content = "reason", rename_all = "snake_case")]
pub enum CloseCause {
    /// Our sell(s) brought the balance to zero; the reason of the last filled order.
    Sold(ExitReason),
    /// The chain balance went to zero without one of our fills (guard, manual).
    BalanceZero,
}

/// Critical conditions for the alerting path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "alert", rename_all = "snake_case")]
pub enum Alert {
    /// An order hit `max_tries` or the per-position sell-spend cap.
    SellExhausted { order_id: u32, tries: u32 },
    /// A sell failed 5 times (docs/06 section 13 critical alert).
    SellFailing { order_id: u32, tries: u32 },
}

/// Output of [`crate::decide`]. Every decision is an action, so recording the
/// actions records every decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ExitAction {
    /// The position opened (first own fill or a non-zero balance).
    Opened {
        origin: Origin,
        tokens: u64,
        v_fill: u64,
    },
    /// First dump signal on the mint: burn the in-flight buy nonce (docs/06 section 7.3).
    DumpSignal { reason: ExitReason, slot: u64 },
    /// Build and send this sell (a retry has `try_no > 1`).
    Sell(SellOrder),
    /// Sells now route to PumpSwap.
    VenueChanged { venue: Venue },
    /// Balance is zero; the position is closed.
    Closed { cause: CloseCause, slot: u64 },
    /// Critical alert.
    Alert(Alert),
}
