//! pump and PumpSwap Anchor events.
//!
//! Two transports carry the same borsh payload:
//! 1. **Self-CPI inner instructions** (`#[event_cpi]`): an inner instruction
//!    whose program is pump/PumpSwap, whose data is
//!    `EVENT_IX_TAG (e445a52e51cb9a1d) || event discriminator || borsh fields`.
//!    Prefer these: logs can be truncated.
//! 2. **`Program data: <base64>` log lines**: `event discriminator || borsh fields`,
//!    attributed to the program on top of the invoke stack.
//!
//! Discriminators are global per event *name* (`sha256("event:TradeEvent")`),
//! so other Anchor programs can emit colliding "TradeEvent"s: always check
//! the emitting program. Decoding borrows from the input (strings are `&str`,
//! no allocation) except for the convenience walkers that return `Vec`s.
//!
//! `TradeEvent` and the PumpSwap events grew fields over time; fields after
//! the original core are `Option`s: `None` means the payload ended before the
//! field (older transaction), a partially present field is an error.

use crate::bytes::Reader;
use crate::constants::{event as disc, EVENT_IX_TAG, PUMP_PROGRAM_ID, PUMP_SWAP_PROGRAM_ID};
use crate::error::{DecodeError, Result};
use base64::Engine as _;
use solana_pubkey::Pubkey;

/// pump `CreateEvent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateEvent<'a> {
    pub name: &'a str,
    pub symbol: &'a str,
    pub uri: &'a str,
    pub mint: Pubkey,
    pub bonding_curve: Pubkey,
    pub user: Pubkey,
    pub creator: Pubkey,
    pub timestamp: i64,
    pub virtual_token_reserves: u64,
    pub virtual_sol_reserves: u64,
    pub real_token_reserves: u64,
    pub token_total_supply: u64,
    pub token_program: Option<Pubkey>,
    pub is_mayhem_mode: Option<bool>,
    pub is_cashback_enabled: Option<bool>,
    pub quote_mint: Option<Pubkey>,
    pub virtual_quote_reserves: Option<u64>,
    pub creator_fee_bps: Option<u64>,
    pub is_holder_reward: Option<bool>,
    pub depth: Option<u8>,
}

/// One `(address, share_bps)` entry of `TradeEvent.shareholders`, kept as raw bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shareholders<'a> {
    raw: &'a [u8],
}

impl<'a> Shareholders<'a> {
    /// Number of entries.
    pub fn len(&self) -> usize {
        self.raw.len() / 34
    }
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }
    /// Iterate `(address, share_bps)`.
    pub fn iter(&self) -> impl Iterator<Item = (Pubkey, u16)> + 'a {
        self.raw.chunks_exact(34).map(|c| {
            let mut k = [0u8; 32];
            k.copy_from_slice(&c[..32]);
            (
                Pubkey::new_from_array(k),
                u16::from_le_bytes([c[32], c[33]]),
            )
        })
    }
}

/// pump `TradeEvent`. Reserves are **post-trade**. On v3 trades
/// `fee_recipient` is the zero key (the fee stays on the curve). Trades by
/// `Global.whitelist_pda` (the mayhem agent, = `MAYHEM_SOL_VAULT`) report
/// 0 bps and 0 fees; quote them with `FeeBps::ZERO`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeEvent<'a> {
    pub mint: Pubkey,
    /// Quote amount moved by the curve (lamports for SOL coins), before fees.
    pub sol_amount: u64,
    pub token_amount: u64,
    pub is_buy: bool,
    pub user: Pubkey,
    pub timestamp: i64,
    pub virtual_sol_reserves: u64,
    pub virtual_token_reserves: u64,
    pub real_sol_reserves: Option<u64>,
    pub real_token_reserves: Option<u64>,
    pub fee_recipient: Option<Pubkey>,
    pub fee_basis_points: Option<u64>,
    pub fee: Option<u64>,
    pub creator: Option<Pubkey>,
    pub creator_fee_basis_points: Option<u64>,
    pub creator_fee: Option<u64>,
    pub track_volume: Option<bool>,
    pub total_unclaimed_tokens: Option<u64>,
    pub total_claimed_tokens: Option<u64>,
    pub current_sol_volume: Option<u64>,
    pub last_update_timestamp: Option<i64>,
    /// Instruction that produced the trade. Observed on-chain 2026-10-14:
    /// `buy_v3` reports `"buy"` and `buy_exact_quote_in_v3` reports
    /// `"buy_exact_quote_in"` (TRADE_V3.md says `"buy_v3"` etc.); `sell_v3`
    /// was not observed. Do not rely on a `_v3` suffix.
    pub ix_name: Option<&'a str>,
    pub mayhem_mode: Option<bool>,
    pub cashback_fee_basis_points: Option<u64>,
    pub cashback: Option<u64>,
    pub buyback_fee_basis_points: Option<u64>,
    pub buyback_fee: Option<u64>,
    pub shareholders: Option<Shareholders<'a>>,
    pub quote_mint: Option<Pubkey>,
    pub quote_amount: Option<u64>,
    pub virtual_quote_reserves: Option<u64>,
    pub real_quote_reserves: Option<u64>,
    pub holder_rewards_bps: Option<u64>,
    pub holder_rewards: Option<u64>,
    pub creator_fee_unclaimed: Option<u64>,
}

impl TradeEvent<'_> {
    /// Number of fields in the 2026-10 IDL.
    pub const FIELDS: usize = 35;

    /// Post-trade curve state (requires the real reserves and creator fields).
    pub fn post_state(&self) -> Option<crate::curve::CurveState> {
        Some(crate::curve::CurveState {
            virtual_sol: self.virtual_sol_reserves,
            virtual_token: self.virtual_token_reserves,
            real_sol: self.real_sol_reserves?,
            real_token: self.real_token_reserves?,
            complete: self.real_token_reserves? == 0,
            creator: self.creator.unwrap_or_default(),
        })
    }

    /// Pre-trade curve state rebuilt from the post-trade reserves and amounts.
    pub fn pre_state(&self) -> Option<crate::curve::CurveState> {
        let post = self.post_state()?;
        crate::curve::CurveState::pre_trade(&post, self.is_buy, self.sol_amount, self.token_amount)
    }

    /// `true` when the trader is the coin creator (one dev-sell signal; see the
    /// critique note C5: ~19% of creates have creator != signer).
    pub fn is_creator_trade(&self) -> bool {
        self.creator == Some(self.user)
    }
}

/// pump `CompleteEvent` (curve sold out).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteEvent {
    pub user: Pubkey,
    pub mint: Pubkey,
    pub bonding_curve: Pubkey,
    pub timestamp: i64,
    pub quote_mint: Option<Pubkey>,
}

/// pump `CompletePumpAmmMigrationEvent` (pool created on PumpSwap).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletePumpAmmMigrationEvent {
    pub user: Pubkey,
    pub mint: Pubkey,
    pub mint_amount: u64,
    pub sol_amount: u64,
    pub pool_migration_fee: u64,
    pub bonding_curve: Pubkey,
    pub timestamp: i64,
    pub pool: Pubkey,
    pub quote_mint: Option<Pubkey>,
}

/// pump `PostCompleteBuyEvent` (synthetic-migration leg of a v3 buy).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostCompleteBuyEvent {
    pub user: Pubkey,
    pub mint: Pubkey,
    pub bonding_curve: Pubkey,
    pub quote_mint: Pubkey,
    pub timestamp: i64,
    pub base_out: u64,
    pub quote_in: u64,
    pub fee_basis_points: u64,
    pub fee: u64,
    pub creator_fee_basis_points: u64,
    pub creator_fee: u64,
    pub buyback_fee: u64,
    pub pool_base_reserves_before: u64,
    pub pool_quote_reserves_before: u64,
    pub pool_base_reserves_after: u64,
    pub pool_quote_reserves_after: u64,
}

/// Fields common to PumpSwap `BuyEvent` and `SellEvent`, in IDL order of the
/// shared prefix. `amount_*` naming follows the event: for a buy `base_amount`
/// is `base_amount_out` and `limit_quote_amount` is `max_quote_amount_in`;
/// for a sell they are `base_amount_in` and `min_quote_amount_out`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmmSwapEvent<'a> {
    pub timestamp: i64,
    pub base_amount: u64,
    pub limit_quote_amount: u64,
    pub user_base_token_reserves: u64,
    pub user_quote_token_reserves: u64,
    /// Pool reserves as reported by the event (pre/post-swap semantics not
    /// verified here; PumpSwap also prices with `virtual_quote_reserves`).
    pub pool_base_token_reserves: u64,
    pub pool_quote_token_reserves: u64,
    /// Buy: `quote_amount_in`; sell: `quote_amount_out`.
    pub quote_amount: u64,
    pub lp_fee_basis_points: u64,
    pub lp_fee: u64,
    pub protocol_fee_basis_points: u64,
    pub protocol_fee: u64,
    /// Buy: `quote_amount_in_with_lp_fee`; sell: `quote_amount_out_without_lp_fee`.
    pub quote_amount_lp_adjusted: u64,
    /// Buy: `user_quote_amount_in`; sell: `user_quote_amount_out`.
    pub user_quote_amount: u64,
    pub pool: Pubkey,
    pub user: Pubkey,
    pub user_base_token_account: Pubkey,
    pub user_quote_token_account: Pubkey,
    pub protocol_fee_recipient: Pubkey,
    pub protocol_fee_recipient_token_account: Pubkey,
    pub coin_creator: Option<Pubkey>,
    pub coin_creator_fee_basis_points: Option<u64>,
    pub coin_creator_fee: Option<u64>,
    /// Buy only.
    pub track_volume: Option<bool>,
    /// Buy only.
    pub total_unclaimed_tokens: Option<u64>,
    /// Buy only.
    pub total_claimed_tokens: Option<u64>,
    /// Buy only.
    pub current_sol_volume: Option<u64>,
    /// Buy only.
    pub last_update_timestamp: Option<i64>,
    /// Buy only.
    pub min_base_amount_out: Option<u64>,
    /// Buy only.
    pub ix_name: Option<&'a str>,
    pub cashback_fee_basis_points: Option<u64>,
    pub cashback: Option<u64>,
    pub buyback_fee_basis_points: Option<u64>,
    pub buyback_fee: Option<u64>,
    pub virtual_quote_reserves: Option<i128>,
    pub can_boost: Option<bool>,
    pub base_supply: Option<u64>,
    pub holder_rewards_bps: Option<u64>,
    pub holder_rewards: Option<u64>,
    pub creator_fee_unclaimed: Option<u64>,
}

/// PumpSwap `CreatePoolEvent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatePoolEvent {
    pub timestamp: i64,
    pub index: u16,
    pub creator: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub base_mint_decimals: u8,
    pub quote_mint_decimals: u8,
    pub base_amount_in: u64,
    pub quote_amount_in: u64,
    pub pool_base_amount: u64,
    pub pool_quote_amount: u64,
    pub minimum_liquidity: u64,
    pub initial_liquidity: u64,
    pub lp_token_amount_out: u64,
    pub pool_bump: u8,
    pub pool: Pubkey,
    pub lp_mint: Pubkey,
    pub user_base_token_account: Pubkey,
    pub user_quote_token_account: Pubkey,
    pub coin_creator: Option<Pubkey>,
    pub is_mayhem_mode: Option<bool>,
    pub creator_fee_bps: Option<u64>,
    pub can_edit_creator_fee: Option<bool>,
    pub is_holder_reward: Option<bool>,
}

/// Which program emitted an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventSource {
    Pump,
    PumpSwap,
}

impl EventSource {
    /// Map a program ID to an event source.
    #[inline]
    pub fn from_program(program_id: &Pubkey) -> Option<Self> {
        if *program_id == PUMP_PROGRAM_ID {
            Some(Self::Pump)
        } else if *program_id == PUMP_SWAP_PROGRAM_ID {
            Some(Self::PumpSwap)
        } else {
            None
        }
    }
}

/// A decoded pump / PumpSwap event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PumpEvent<'a> {
    Create(CreateEvent<'a>),
    Trade(TradeEvent<'a>),
    Complete(CompleteEvent),
    CompletePumpAmmMigration(CompletePumpAmmMigrationEvent),
    PostCompleteBuy(PostCompleteBuyEvent),
    AmmBuy(AmmSwapEvent<'a>),
    AmmSell(AmmSwapEvent<'a>),
    AmmCreatePool(CreatePoolEvent),
    /// An event of a known program that this crate does not decode.
    Other {
        source: EventSource,
        discriminator: [u8; 8],
        payload: &'a [u8],
    },
}

impl PumpEvent<'_> {
    /// IDL name of the event (`"Other"` for undecoded ones).
    pub fn name(&self) -> &'static str {
        match self {
            Self::Create(_) => "CreateEvent",
            Self::Trade(_) => "TradeEvent",
            Self::Complete(_) => "CompleteEvent",
            Self::CompletePumpAmmMigration(_) => "CompletePumpAmmMigrationEvent",
            Self::PostCompleteBuy(_) => "PostCompleteBuyEvent",
            Self::AmmBuy(_) => "BuyEvent",
            Self::AmmSell(_) => "SellEvent",
            Self::AmmCreatePool(_) => "CreatePoolEvent",
            Self::Other { .. } => "Other",
        }
    }
}

// ---------------------------------------------------------------- decoders
fn decode_create<'a>(r: &mut Reader<'a>) -> Result<CreateEvent<'a>> {
    Ok(CreateEvent {
        name: r.string()?,
        symbol: r.string()?,
        uri: r.string()?,
        mint: r.pubkey()?,
        bonding_curve: r.pubkey()?,
        user: r.pubkey()?,
        creator: r.pubkey()?,
        timestamp: r.i64()?,
        virtual_token_reserves: r.u64()?,
        virtual_sol_reserves: r.u64()?,
        real_token_reserves: r.u64()?,
        token_total_supply: r.u64()?,
        token_program: r.opt_pubkey()?,
        is_mayhem_mode: r.opt_bool()?,
        is_cashback_enabled: r.opt_bool()?,
        quote_mint: r.opt_pubkey()?,
        virtual_quote_reserves: r.opt_u64()?,
        creator_fee_bps: r.opt_u64()?,
        is_holder_reward: r.opt_bool()?,
        depth: r.opt_u8()?,
    })
}

fn opt_shareholders<'a>(r: &mut Reader<'a>) -> Result<Option<Shareholders<'a>>> {
    if r.is_empty() {
        return Ok(None);
    }
    let n = r.u32()? as usize;
    let offset = r.pos();
    let bytes = n
        .checked_mul(34)
        .ok_or(DecodeError::InvalidLength { offset, value: n })?;
    Ok(Some(Shareholders {
        raw: r.take(bytes)?,
    }))
}

fn decode_trade<'a>(r: &mut Reader<'a>) -> Result<TradeEvent<'a>> {
    Ok(TradeEvent {
        mint: r.pubkey()?,
        sol_amount: r.u64()?,
        token_amount: r.u64()?,
        is_buy: r.bool()?,
        user: r.pubkey()?,
        timestamp: r.i64()?,
        virtual_sol_reserves: r.u64()?,
        virtual_token_reserves: r.u64()?,
        real_sol_reserves: r.opt_u64()?,
        real_token_reserves: r.opt_u64()?,
        fee_recipient: r.opt_pubkey()?,
        fee_basis_points: r.opt_u64()?,
        fee: r.opt_u64()?,
        creator: r.opt_pubkey()?,
        creator_fee_basis_points: r.opt_u64()?,
        creator_fee: r.opt_u64()?,
        track_volume: r.opt_bool()?,
        total_unclaimed_tokens: r.opt_u64()?,
        total_claimed_tokens: r.opt_u64()?,
        current_sol_volume: r.opt_u64()?,
        last_update_timestamp: r.opt_i64()?,
        ix_name: r.opt_string()?,
        mayhem_mode: r.opt_bool()?,
        cashback_fee_basis_points: r.opt_u64()?,
        cashback: r.opt_u64()?,
        buyback_fee_basis_points: r.opt_u64()?,
        buyback_fee: r.opt_u64()?,
        shareholders: opt_shareholders(r)?,
        quote_mint: r.opt_pubkey()?,
        quote_amount: r.opt_u64()?,
        virtual_quote_reserves: r.opt_u64()?,
        real_quote_reserves: r.opt_u64()?,
        holder_rewards_bps: r.opt_u64()?,
        holder_rewards: r.opt_u64()?,
        creator_fee_unclaimed: r.opt_u64()?,
    })
}

fn decode_complete(r: &mut Reader<'_>) -> Result<CompleteEvent> {
    Ok(CompleteEvent {
        user: r.pubkey()?,
        mint: r.pubkey()?,
        bonding_curve: r.pubkey()?,
        timestamp: r.i64()?,
        quote_mint: r.opt_pubkey()?,
    })
}

fn decode_migration(r: &mut Reader<'_>) -> Result<CompletePumpAmmMigrationEvent> {
    Ok(CompletePumpAmmMigrationEvent {
        user: r.pubkey()?,
        mint: r.pubkey()?,
        mint_amount: r.u64()?,
        sol_amount: r.u64()?,
        pool_migration_fee: r.u64()?,
        bonding_curve: r.pubkey()?,
        timestamp: r.i64()?,
        pool: r.pubkey()?,
        quote_mint: r.opt_pubkey()?,
    })
}

fn decode_post_complete_buy(r: &mut Reader<'_>) -> Result<PostCompleteBuyEvent> {
    Ok(PostCompleteBuyEvent {
        user: r.pubkey()?,
        mint: r.pubkey()?,
        bonding_curve: r.pubkey()?,
        quote_mint: r.pubkey()?,
        timestamp: r.i64()?,
        base_out: r.u64()?,
        quote_in: r.u64()?,
        fee_basis_points: r.u64()?,
        fee: r.u64()?,
        creator_fee_basis_points: r.u64()?,
        creator_fee: r.u64()?,
        buyback_fee: r.u64()?,
        pool_base_reserves_before: r.u64()?,
        pool_quote_reserves_before: r.u64()?,
        pool_base_reserves_after: r.u64()?,
        pool_quote_reserves_after: r.u64()?,
    })
}

fn decode_amm_swap<'a>(r: &mut Reader<'a>, is_buy: bool) -> Result<AmmSwapEvent<'a>> {
    let mut e = AmmSwapEvent {
        timestamp: r.i64()?,
        base_amount: r.u64()?,
        limit_quote_amount: r.u64()?,
        user_base_token_reserves: r.u64()?,
        user_quote_token_reserves: r.u64()?,
        pool_base_token_reserves: r.u64()?,
        pool_quote_token_reserves: r.u64()?,
        quote_amount: r.u64()?,
        lp_fee_basis_points: r.u64()?,
        lp_fee: r.u64()?,
        protocol_fee_basis_points: r.u64()?,
        protocol_fee: r.u64()?,
        quote_amount_lp_adjusted: r.u64()?,
        user_quote_amount: r.u64()?,
        pool: r.pubkey()?,
        user: r.pubkey()?,
        user_base_token_account: r.pubkey()?,
        user_quote_token_account: r.pubkey()?,
        protocol_fee_recipient: r.pubkey()?,
        protocol_fee_recipient_token_account: r.pubkey()?,
        coin_creator: r.opt_pubkey()?,
        coin_creator_fee_basis_points: r.opt_u64()?,
        coin_creator_fee: r.opt_u64()?,
        track_volume: None,
        total_unclaimed_tokens: None,
        total_claimed_tokens: None,
        current_sol_volume: None,
        last_update_timestamp: None,
        min_base_amount_out: None,
        ix_name: None,
        cashback_fee_basis_points: None,
        cashback: None,
        buyback_fee_basis_points: None,
        buyback_fee: None,
        virtual_quote_reserves: None,
        can_boost: None,
        base_supply: None,
        holder_rewards_bps: None,
        holder_rewards: None,
        creator_fee_unclaimed: None,
    };
    if is_buy {
        e.track_volume = r.opt_bool()?;
        e.total_unclaimed_tokens = r.opt_u64()?;
        e.total_claimed_tokens = r.opt_u64()?;
        e.current_sol_volume = r.opt_u64()?;
        e.last_update_timestamp = r.opt_i64()?;
        e.min_base_amount_out = r.opt_u64()?;
        e.ix_name = r.opt_string()?;
    }
    e.cashback_fee_basis_points = r.opt_u64()?;
    e.cashback = r.opt_u64()?;
    e.buyback_fee_basis_points = r.opt_u64()?;
    e.buyback_fee = r.opt_u64()?;
    e.virtual_quote_reserves = r.opt_i128()?;
    e.can_boost = r.opt_bool()?;
    e.base_supply = r.opt_u64()?;
    e.holder_rewards_bps = r.opt_u64()?;
    e.holder_rewards = r.opt_u64()?;
    e.creator_fee_unclaimed = r.opt_u64()?;
    Ok(e)
}

fn decode_create_pool(r: &mut Reader<'_>) -> Result<CreatePoolEvent> {
    Ok(CreatePoolEvent {
        timestamp: r.i64()?,
        index: r.u16()?,
        creator: r.pubkey()?,
        base_mint: r.pubkey()?,
        quote_mint: r.pubkey()?,
        base_mint_decimals: r.u8()?,
        quote_mint_decimals: r.u8()?,
        base_amount_in: r.u64()?,
        quote_amount_in: r.u64()?,
        pool_base_amount: r.u64()?,
        pool_quote_amount: r.u64()?,
        minimum_liquidity: r.u64()?,
        initial_liquidity: r.u64()?,
        lp_token_amount_out: r.u64()?,
        pool_bump: r.u8()?,
        pool: r.pubkey()?,
        lp_mint: r.pubkey()?,
        user_base_token_account: r.pubkey()?,
        user_quote_token_account: r.pubkey()?,
        coin_creator: r.opt_pubkey()?,
        is_mayhem_mode: r.opt_bool()?,
        creator_fee_bps: r.opt_u64()?,
        can_edit_creator_fee: r.opt_bool()?,
        is_holder_reward: r.opt_bool()?,
    })
}

/// Decode `event discriminator || borsh payload` (the `Program data:` form)
/// emitted by `source`. Unknown discriminators give [`PumpEvent::Other`].
pub fn decode_event_payload(source: EventSource, data: &[u8]) -> Result<PumpEvent<'_>> {
    let mut r = Reader::new(data);
    let d: [u8; 8] = r.array()?;
    let ev = match (source, d) {
        (EventSource::Pump, disc::CREATE) => PumpEvent::Create(decode_create(&mut r)?),
        (EventSource::Pump, disc::TRADE) => PumpEvent::Trade(decode_trade(&mut r)?),
        (EventSource::Pump, disc::COMPLETE) => PumpEvent::Complete(decode_complete(&mut r)?),
        (EventSource::Pump, disc::COMPLETE_PUMP_AMM_MIGRATION) => {
            PumpEvent::CompletePumpAmmMigration(decode_migration(&mut r)?)
        }
        (EventSource::Pump, disc::POST_COMPLETE_BUY) => {
            PumpEvent::PostCompleteBuy(decode_post_complete_buy(&mut r)?)
        }
        (EventSource::PumpSwap, disc::AMM_BUY) => PumpEvent::AmmBuy(decode_amm_swap(&mut r, true)?),
        (EventSource::PumpSwap, disc::AMM_SELL) => {
            PumpEvent::AmmSell(decode_amm_swap(&mut r, false)?)
        }
        (EventSource::PumpSwap, disc::AMM_CREATE_POOL) => {
            PumpEvent::AmmCreatePool(decode_create_pool(&mut r)?)
        }
        _ => PumpEvent::Other {
            source,
            discriminator: d,
            payload: r.rest(),
        },
    };
    Ok(ev)
}

/// Decode a self-CPI event instruction: `program_id` must be pump or
/// PumpSwap and `ix_data` must start with [`EVENT_IX_TAG`]. Returns
/// `Ok(None)` for anything that is not such an event instruction.
pub fn decode_cpi_event<'a>(
    program_id: &Pubkey,
    ix_data: &'a [u8],
) -> Result<Option<PumpEvent<'a>>> {
    let Some(source) = EventSource::from_program(program_id) else {
        return Ok(None);
    };
    match ix_data.get(..8) {
        Some(tag) if tag == EVENT_IX_TAG => {}
        _ => return Ok(None),
    }
    decode_event_payload(source, &ix_data[8..]).map(Some)
}

/// An event located in a transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRecord<'a> {
    /// Index of the top-level instruction whose CPIs produced the event.
    pub outer_index: u8,
    /// Position inside that instruction's inner-instruction list.
    pub inner_index: usize,
    pub source: EventSource,
    pub event: PumpEvent<'a>,
}

/// One inner (CPI) instruction, as found in transaction meta
/// (`meta.innerInstructions[*].instructions[*]` with base58 data decoded,
/// or Yellowstone `InnerInstruction` with raw data).
#[derive(Debug, Clone, Copy)]
pub struct InnerInstruction<'a> {
    /// Index of the top-level instruction this CPI belongs to.
    pub outer_index: u8,
    /// Index into the transaction's full account list (static keys, then
    /// loaded writable, then loaded readonly addresses).
    pub program_id_index: u8,
    pub accounts: &'a [u8],
    pub data: &'a [u8],
}

/// Walk a transaction's inner instructions in order and return every
/// pump / PumpSwap self-CPI event. `account_keys` is the full account list
/// (static + ALT-loaded writable + ALT-loaded readonly). With
/// `require_event_authority`, the CPI's first account must be the program's
/// event authority (as `emit_cpi!` does), which rules out look-alike data.
pub fn events_from_inner_instructions<'a, I>(
    account_keys: &[Pubkey],
    inner: I,
    require_event_authority: bool,
) -> Result<Vec<EventRecord<'a>>>
where
    I: IntoIterator<Item = InnerInstruction<'a>>,
{
    let mut out = Vec::new();
    let mut last_outer: Option<u8> = None;
    let mut pos = 0usize;
    for ix in inner {
        if last_outer != Some(ix.outer_index) {
            last_outer = Some(ix.outer_index);
            pos = 0;
        } else {
            pos += 1;
        }
        let Some(program) = account_keys.get(ix.program_id_index as usize) else {
            continue;
        };
        let Some(source) = EventSource::from_program(program) else {
            continue;
        };
        if require_event_authority {
            let want = match source {
                EventSource::Pump => crate::constants::EVENT_AUTHORITY,
                EventSource::PumpSwap => crate::constants::PUMP_SWAP_EVENT_AUTHORITY,
            };
            let first = ix
                .accounts
                .first()
                .and_then(|i| account_keys.get(*i as usize));
            if first != Some(&want) {
                continue;
            }
        }
        if let Some(event) = decode_cpi_event(program, ix.data)? {
            out.push(EventRecord {
                outer_index: ix.outer_index,
                inner_index: pos,
                source,
                event,
            });
        }
    }
    Ok(out)
}

/// A `Program data:` payload attributed to its emitting program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogData {
    /// Program on top of the invoke stack when the line was logged.
    pub program: Pubkey,
    /// Invoke depth (1 = top-level instruction).
    pub depth: u8,
    /// Base64-decoded bytes (`event discriminator || payload` for Anchor events).
    pub data: Vec<u8>,
}

/// Result of scanning log messages.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogScan {
    pub data: Vec<LogData>,
    /// `true` if the runtime truncated the logs ("Log truncated"): events may be missing.
    pub truncated: bool,
}

/// Scan `logMessages`, tracking the invoke stack, and return every
/// `Program data:` payload with the program that logged it.
pub fn scan_program_data_logs<S: AsRef<str>>(logs: &[S]) -> Result<LogScan> {
    let mut stack: Vec<Pubkey> = Vec::with_capacity(8);
    let mut scan = LogScan::default();
    for line in logs {
        let line = line.as_ref();
        if let Some(rest) = line.strip_prefix("Program data: ") {
            let Some(program) = stack.last() else {
                continue;
            };
            // Anchor logs one base64 blob per line; tolerate several separated by spaces.
            let mut data = Vec::new();
            for part in rest.split(' ').filter(|p| !p.is_empty()) {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(part)
                    .map_err(|_| DecodeError::InvalidBase64)?;
                data.extend_from_slice(&bytes);
            }
            scan.data.push(LogData {
                program: *program,
                depth: stack.len() as u8,
                data,
            });
        } else if line == "Log truncated" {
            scan.truncated = true;
        } else if let Some(rest) = line.strip_prefix("Program ") {
            // "Program <id> invoke [n]" / "Program <id> success" / "Program <id> failed: ..."
            let mut it = rest.splitn(2, ' ');
            let (Some(id), Some(tail)) = (it.next(), it.next()) else {
                continue;
            };
            if tail.starts_with("invoke [") {
                if let Ok(pk) = id.parse::<Pubkey>() {
                    stack.push(pk);
                }
            } else if tail == "success" || tail.starts_with("failed") {
                if let Ok(pk) = id.parse::<Pubkey>() {
                    if stack.last() == Some(&pk) {
                        stack.pop();
                    }
                }
            }
        }
    }
    Ok(scan)
}

/// Decode the pump / PumpSwap events in a [`LogScan`] (others are skipped).
pub fn events_from_log_scan(scan: &LogScan) -> Result<Vec<(EventSource, PumpEvent<'_>)>> {
    let mut out = Vec::new();
    for d in &scan.data {
        if let Some(src) = EventSource::from_program(&d.program) {
            out.push((src, decode_event_payload(src, &d.data)?));
        }
    }
    Ok(out)
}
