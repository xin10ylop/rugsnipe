//! Parse and build pump / PumpSwap instructions.
//!
//! **Parse** ([`classify`]): key on the program ID first (pump and PumpSwap
//! share the `buy`, `sell`, `buy_v2`, `sell_v2` and `buy_exact_quote_in_v2`
//! discriminators), then the 8-byte discriminator, then borsh args. Trailing
//! optional args (`OptionBool`/`OptionU64`) may be absent; extra trailing
//! bytes are ignored. Accounts are looked up through [`AccountLookup`], so a
//! v0 transaction whose ALT entries are not resolved still parses (named
//! accounts that live in a lookup table come back as `None`).
//!
//! **Build**: byte-level builders for the sniper path (`buy_v3`,
//! `buy_exact_quote_in_v3`, `sell_v3` on SOL-paired coins, legacy `buy` /
//! `buy_exact_sol_in` / `sell` with the two undocumented remaining accounts,
//! Token-2022 ATA create-idempotent, compute budget, tip transfer, durable
//! nonce advance). Each is checked against the official `pump-rust-client`
//! 0.4.1 / the Solana interface crates and against on-chain transactions in
//! the tests.

use crate::bytes::Reader;
use crate::constants::{self as c, amm_ix, ix};
use crate::error::{DecodeError, Result};
use crate::pda;
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;

// ================================================================ lookups
/// Read access to an instruction's account list (by position in the instruction).
pub trait AccountLookup {
    /// Number of accounts the instruction passes.
    fn account_count(&self) -> usize;
    /// Account at instruction position `i`; `None` when out of range or not
    /// resolvable (an unresolved address-lookup-table entry).
    fn account(&self, i: usize) -> Option<Pubkey>;
}

impl AccountLookup for [Pubkey] {
    fn account_count(&self) -> usize {
        self.len()
    }
    fn account(&self, i: usize) -> Option<Pubkey> {
        self.get(i).copied()
    }
}

impl AccountLookup for [Option<Pubkey>] {
    fn account_count(&self) -> usize {
        self.len()
    }
    fn account(&self, i: usize) -> Option<Pubkey> {
        self.get(i).copied().flatten()
    }
}

impl<const N: usize> AccountLookup for [Pubkey; N] {
    fn account_count(&self) -> usize {
        N
    }
    fn account(&self, i: usize) -> Option<Pubkey> {
        self.get(i).copied()
    }
}

impl<T: AccountLookup + ?Sized> AccountLookup for &T {
    fn account_count(&self) -> usize {
        (**self).account_count()
    }
    fn account(&self, i: usize) -> Option<Pubkey> {
        (**self).account(i)
    }
}

impl AccountLookup for Vec<Pubkey> {
    fn account_count(&self) -> usize {
        self.len()
    }
    fn account(&self, i: usize) -> Option<Pubkey> {
        self.get(i).copied()
    }
}

// ================================================================ parsing
/// Coin creation arguments (`create` uses only name/symbol/uri/creator).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateArgs<'a> {
    pub name: &'a str,
    pub symbol: &'a str,
    pub uri: &'a str,
    /// Fee recipient / "creator" argument (can differ from the signer). Not
    /// the curve's creator on holder-reward coins (`is_holder_reward ==
    /// Some(true)`): the program records a pump-controlled address instead, so
    /// never derive `creator_vault` from this argument (v3 trades need none).
    pub creator: Pubkey,
    /// `create_v2` only (always `false` for legacy `create`).
    pub is_mayhem_mode: bool,
    /// Deprecated, must be false/absent.
    pub is_cashback_enabled: Option<bool>,
    pub creator_fee_bps: Option<u64>,
    pub is_holder_reward: Option<bool>,
}

/// Which create instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateKind {
    /// Legacy `create`: SPL Token + Metaplex, 14 accounts (user #7, token_program #9).
    Legacy,
    /// `create_v2`: Token-2022, 16 accounts + optional quote tail.
    V2,
}

/// A parsed `create` / `create_v2`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PumpCreate<'a> {
    pub kind: CreateKind,
    pub args: CreateArgs<'a>,
    /// Account #0 (signer).
    pub mint: Option<Pubkey>,
    /// Account #2.
    pub bonding_curve: Option<Pubkey>,
    /// Account #3.
    pub associated_bonding_curve: Option<Pubkey>,
    /// Signer and payer: #5 (`create_v2`) / #7 (`create`).
    pub user: Option<Pubkey>,
    /// #7 (`create_v2`) / #9 (`create`).
    pub token_program: Option<Pubkey>,
    /// `create_v2` remaining account #16 (`quote_mint`) when a quote tail is present.
    pub quote_mint: Option<Pubkey>,
    /// Remaining accounts after the 16 IDL accounts (0, 3, or 4+ with `quote_control`
    /// and the deeper pump-coin-quote accounts).
    pub quote_tail_len: usize,
    /// `Some(true)` for a SOL-paired coin (no tail, or the tail passes wrapped
    /// SOL / the zero key), `Some(false)` for a token quote, `None` when the
    /// quote mint is in an unresolved lookup table.
    pub is_sol_quote: Option<bool>,
    pub num_accounts: usize,
}

/// pump trade instruction flavours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PumpTradeKind {
    Buy,
    BuyExactSolIn,
    Sell,
    BuyV2,
    SellV2,
    BuyExactQuoteInV2,
    BuyV3,
    SellV3,
    BuyExactQuoteInV3,
}

impl PumpTradeKind {
    /// `true` for buys.
    pub fn is_buy(self) -> bool {
        !matches!(self, Self::Sell | Self::SellV2 | Self::SellV3)
    }
    /// IDL instruction name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Buy => "buy",
            Self::BuyExactSolIn => "buy_exact_sol_in",
            Self::Sell => "sell",
            Self::BuyV2 => "buy_v2",
            Self::SellV2 => "sell_v2",
            Self::BuyExactQuoteInV2 => "buy_exact_quote_in_v2",
            Self::BuyV3 => "buy_v3",
            Self::SellV3 => "sell_v3",
            Self::BuyExactQuoteInV3 => "buy_exact_quote_in_v3",
        }
    }
    /// Account layout family.
    pub fn layout(self) -> TradeLayout {
        match self {
            Self::Buy | Self::BuyExactSolIn => TradeLayout::LegacyBuy,
            Self::Sell => TradeLayout::LegacySell,
            Self::BuyV2 | Self::BuyExactQuoteInV2 => TradeLayout::V2Buy,
            Self::SellV2 => TradeLayout::V2Sell,
            Self::BuyV3 | Self::SellV3 | Self::BuyExactQuoteInV3 => TradeLayout::V3,
        }
    }
}

/// Account layout families of pump trades.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeLayout {
    /// `buy` / `buy_exact_sol_in`: 16 IDL accounts + 2 remaining.
    LegacyBuy,
    /// `sell`: 14 IDL accounts + 2 remaining (+1 before them on cashback coins).
    LegacySell,
    /// `buy_v2` / `buy_exact_quote_in_v2`: 27 accounts.
    V2Buy,
    /// `sell_v2`: 26 accounts.
    V2Sell,
    /// v3: 17 accounts.
    V3,
}

/// Amount semantics of a trade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeAmounts {
    /// Exactly `amount` tokens out, pay at most `max_quote_cost` (fees included).
    ExactTokensOut { amount: u64, max_quote_cost: u64 },
    /// Spend exactly `spendable_quote_in` (fees included), receive at least `min_tokens_out`.
    ExactQuoteIn {
        spendable_quote_in: u64,
        min_tokens_out: u64,
    },
    /// Sell `amount` tokens for at least `min_quote_out` (after fees).
    Sell { amount: u64, min_quote_out: u64 },
}

/// A parsed pump trade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PumpTrade {
    pub kind: PumpTradeKind,
    pub amounts: TradeAmounts,
    /// Legacy `buy`/`buy_exact_sol_in` only.
    pub track_volume: Option<bool>,
    /// Buys only (sells have no `partial_fill` argument).
    pub partial_fill: Option<bool>,
    pub mint: Option<Pubkey>,
    pub bonding_curve: Option<Pubkey>,
    pub user: Option<Pubkey>,
    pub base_token_program: Option<Pubkey>,
    /// v2/v3 only (`None` on legacy, which is always SOL).
    pub quote_mint: Option<Pubkey>,
    /// Legacy / v2 protocol fee recipient (v3 has none).
    pub fee_recipient: Option<Pubkey>,
    /// v2 #8, v3 #13, legacy remaining (last account). On a token-quoted v3
    /// trade #13 is the buyback wallet's quote ATA, not the wallet.
    pub buyback_fee_recipient: Option<Pubkey>,
    pub num_accounts: usize,
}

/// A classified pump instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PumpInstruction<'a> {
    Create(PumpCreate<'a>),
    Trade(PumpTrade),
    /// Any other pump instruction (migrate, extend_account, fee sweeps, ...).
    Other {
        discriminator: [u8; 8],
    },
}

/// PumpSwap trade flavours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmmTradeKind {
    Buy,
    Sell,
    BuyExactQuoteIn,
    BuyV2,
    SellV2,
    BuyExactQuoteInV2,
}

impl AmmTradeKind {
    pub fn is_buy(self) -> bool {
        !matches!(self, Self::Sell | Self::SellV2)
    }
}

/// A parsed PumpSwap trade. Accounts #0..#4 are the same in every flavour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmmTrade {
    pub kind: AmmTradeKind,
    pub amounts: TradeAmounts,
    pub track_volume: Option<bool>,
    pub pool: Option<Pubkey>,
    pub user: Option<Pubkey>,
    pub base_mint: Option<Pubkey>,
    pub quote_mint: Option<Pubkey>,
    pub num_accounts: usize,
}

/// A parsed PumpSwap `create_pool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmmCreatePool {
    pub index: u16,
    pub base_amount_in: u64,
    pub quote_amount_in: u64,
    pub coin_creator: Option<Pubkey>,
    pub is_mayhem_mode: Option<bool>,
    pub pool: Option<Pubkey>,
    pub creator: Option<Pubkey>,
    pub base_mint: Option<Pubkey>,
    pub quote_mint: Option<Pubkey>,
}

/// A classified PumpSwap instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmmInstruction {
    Trade(AmmTrade),
    CreatePool(AmmCreatePool),
    Other { discriminator: [u8; 8] },
}

/// Result of [`classify`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classified<'a> {
    Pump(PumpInstruction<'a>),
    PumpSwap(AmmInstruction),
    /// Not a pump / PumpSwap instruction.
    NotPump,
}

/// Classify an instruction by program ID, then discriminator, then args.
pub fn classify<'a, A: AccountLookup + ?Sized>(
    program_id: &Pubkey,
    data: &'a [u8],
    accounts: &A,
) -> Result<Classified<'a>> {
    if *program_id == c::PUMP_PROGRAM_ID {
        parse_pump_instruction(data, accounts).map(Classified::Pump)
    } else if *program_id == c::PUMP_SWAP_PROGRAM_ID {
        parse_pump_swap_instruction(data, accounts).map(Classified::PumpSwap)
    } else {
        Ok(Classified::NotPump)
    }
}

fn read_create_args<'a>(r: &mut Reader<'a>, v2: bool) -> Result<CreateArgs<'a>> {
    let name = r.string()?;
    let symbol = r.string()?;
    let uri = r.string()?;
    let creator = r.pubkey()?;
    if !v2 {
        return Ok(CreateArgs {
            name,
            symbol,
            uri,
            creator,
            is_mayhem_mode: false,
            is_cashback_enabled: None,
            creator_fee_bps: None,
            is_holder_reward: None,
        });
    }
    let is_mayhem_mode = r.bool()?;
    let is_cashback_enabled = r.opt_bool()?;
    // The trailing fields are flags appended over time and encoded as plain
    // values, not Option tags: clients that know only some of them send a
    // shorter tail. `creator_fee_bps` is present only when 8 bytes remain
    // for it; a single trailing byte is `is_holder_reward` (seen live on
    // 2026-10-22: `00 00 01` after the creator, 1.3% of creates, which read
    // the `01` as the first byte of a u64 and failed the whole create).
    let creator_fee_bps = if r.remaining() >= 8 {
        r.opt_u64()?
    } else {
        None
    };
    let is_holder_reward = r.opt_bool()?;
    Ok(CreateArgs {
        name,
        symbol,
        uri,
        creator,
        is_mayhem_mode,
        is_cashback_enabled,
        creator_fee_bps,
        is_holder_reward,
    })
}

/// First account at position `>= start` that is a listed buyback recipient.
#[inline]
fn find_buyback(a: &impl Fn(usize) -> Option<Pubkey>, start: usize, n: usize) -> Option<Pubkey> {
    (start..n)
        .filter_map(a)
        .find(|k| c::BUYBACK_FEE_RECIPIENTS.contains(k))
}

/// Parse a pump-program instruction (caller has checked the program ID).
pub fn parse_pump_instruction<'a, A: AccountLookup + ?Sized>(
    data: &'a [u8],
    accounts: &A,
) -> Result<PumpInstruction<'a>> {
    let mut r = Reader::new(data);
    let d: [u8; 8] = r.array()?;
    let n = accounts.account_count();
    let a = |i: usize| accounts.account(i);
    let ix = match d {
        ix::CREATE => {
            let args = read_create_args(&mut r, false)?;
            PumpInstruction::Create(PumpCreate {
                kind: CreateKind::Legacy,
                args,
                mint: a(0),
                bonding_curve: a(2),
                associated_bonding_curve: a(3),
                user: a(7),
                token_program: a(9),
                quote_mint: None,
                quote_tail_len: 0,
                is_sol_quote: Some(true),
                num_accounts: n,
            })
        }
        ix::CREATE_V2 => {
            let args = read_create_args(&mut r, true)?;
            let tail = n.saturating_sub(16);
            let quote_mint = if tail > 0 { a(16) } else { None };
            let is_sol_quote = if tail == 0 {
                Some(true)
            } else {
                quote_mint.map(|q| c::is_sol_like_quote_mint(&q))
            };
            PumpInstruction::Create(PumpCreate {
                kind: CreateKind::V2,
                args,
                mint: a(0),
                bonding_curve: a(2),
                associated_bonding_curve: a(3),
                user: a(5),
                // the IDL fixes create_v2's token program to Token-2022: a
                // v0 create that loads it from a lookup table (most of them)
                // still resolves without the loaded addresses (pre-execution
                // bytes: the shredstream entries, a deshred copy without them)
                token_program: a(7).or(Some(c::TOKEN_2022_PROGRAM_ID)),
                quote_mint,
                quote_tail_len: tail,
                is_sol_quote,
                num_accounts: n,
            })
        }
        ix::BUY | ix::BUY_EXACT_SOL_IN => {
            let x = r.u64()?;
            let y = r.u64()?;
            let track_volume = r.opt_bool()?;
            let partial_fill = r.opt_bool()?;
            let (kind, amounts) = if d == ix::BUY {
                (
                    PumpTradeKind::Buy,
                    TradeAmounts::ExactTokensOut {
                        amount: x,
                        max_quote_cost: y,
                    },
                )
            } else {
                (
                    PumpTradeKind::BuyExactSolIn,
                    TradeAmounts::ExactQuoteIn {
                        spendable_quote_in: x,
                        min_tokens_out: y,
                    },
                )
            };
            PumpInstruction::Trade(PumpTrade {
                kind,
                amounts,
                track_volume,
                partial_fill,
                mint: a(2),
                bonding_curve: a(3),
                user: a(6),
                base_token_program: a(8),
                quote_mint: None,
                fee_recipient: a(1),
                buyback_fee_recipient: find_buyback(&a, 16, n).or_else(|| {
                    if n >= 18 {
                        a(17)
                    } else {
                        None
                    }
                }),
                num_accounts: n,
            })
        }
        ix::SELL => {
            let amount = r.u64()?;
            let min_quote_out = r.u64()?;
            // Remaining accounts: [user_volume_accumulator (cashback coins)],
            // bonding_curve_v2, buyback_fee_recipient (+ anything a sender
            // appends). No PDA derivation here: match the listed recipients.
            let buyback_fee_recipient =
                find_buyback(&a, 14, n).or_else(|| if n >= 16 { a(15) } else { None });
            PumpInstruction::Trade(PumpTrade {
                kind: PumpTradeKind::Sell,
                amounts: TradeAmounts::Sell {
                    amount,
                    min_quote_out,
                },
                track_volume: None,
                partial_fill: None,
                mint: a(2),
                bonding_curve: a(3),
                user: a(6),
                base_token_program: a(9),
                quote_mint: None,
                fee_recipient: a(1),
                buyback_fee_recipient,
                num_accounts: n,
            })
        }
        ix::BUY_V2 | ix::BUY_EXACT_QUOTE_IN_V2 | ix::SELL_V2 => {
            let x = r.u64()?;
            let y = r.u64()?;
            let (kind, amounts, partial_fill) = match d {
                ix::BUY_V2 => (
                    PumpTradeKind::BuyV2,
                    TradeAmounts::ExactTokensOut {
                        amount: x,
                        max_quote_cost: y,
                    },
                    r.opt_bool()?,
                ),
                ix::BUY_EXACT_QUOTE_IN_V2 => (
                    PumpTradeKind::BuyExactQuoteInV2,
                    TradeAmounts::ExactQuoteIn {
                        spendable_quote_in: x,
                        min_tokens_out: y,
                    },
                    r.opt_bool()?,
                ),
                _ => (
                    PumpTradeKind::SellV2,
                    TradeAmounts::Sell {
                        amount: x,
                        min_quote_out: y,
                    },
                    None,
                ),
            };
            PumpInstruction::Trade(PumpTrade {
                kind,
                amounts,
                track_volume: None,
                partial_fill,
                mint: a(1),
                bonding_curve: a(10),
                user: a(13),
                base_token_program: a(3),
                quote_mint: a(2),
                fee_recipient: a(6),
                buyback_fee_recipient: a(8),
                num_accounts: n,
            })
        }
        ix::BUY_V3 | ix::BUY_EXACT_QUOTE_IN_V3 | ix::SELL_V3 => {
            let x = r.u64()?;
            let y = r.u64()?;
            let (kind, amounts, partial_fill) = match d {
                ix::BUY_V3 => (
                    PumpTradeKind::BuyV3,
                    TradeAmounts::ExactTokensOut {
                        amount: x,
                        max_quote_cost: y,
                    },
                    r.opt_bool()?,
                ),
                ix::BUY_EXACT_QUOTE_IN_V3 => (
                    PumpTradeKind::BuyExactQuoteInV3,
                    TradeAmounts::ExactQuoteIn {
                        spendable_quote_in: x,
                        min_tokens_out: y,
                    },
                    r.opt_bool()?,
                ),
                _ => (
                    PumpTradeKind::SellV3,
                    TradeAmounts::Sell {
                        amount: x,
                        min_quote_out: y,
                    },
                    None,
                ),
            };
            PumpInstruction::Trade(PumpTrade {
                kind,
                amounts,
                track_volume: None,
                partial_fill,
                mint: a(1),
                bonding_curve: a(5),
                user: a(8),
                base_token_program: a(3),
                quote_mint: a(2),
                fee_recipient: None,
                buyback_fee_recipient: a(13),
                num_accounts: n,
            })
        }
        other => PumpInstruction::Other {
            discriminator: other,
        },
    };
    Ok(ix)
}

/// Parse a PumpSwap instruction (caller has checked the program ID).
pub fn parse_pump_swap_instruction<A: AccountLookup + ?Sized>(
    data: &[u8],
    accounts: &A,
) -> Result<AmmInstruction> {
    let mut r = Reader::new(data);
    let d: [u8; 8] = r.array()?;
    let n = accounts.account_count();
    let a = |i: usize| accounts.account(i);
    let trade = |kind, amounts, track_volume| {
        AmmInstruction::Trade(AmmTrade {
            kind,
            amounts,
            track_volume,
            pool: a(0),
            user: a(1),
            base_mint: a(3),
            quote_mint: a(4),
            num_accounts: n,
        })
    };
    Ok(match d {
        amm_ix::BUY | amm_ix::BUY_V2 => {
            let amount = r.u64()?;
            let max_quote_cost = r.u64()?;
            let (kind, tv) = if d == amm_ix::BUY {
                (AmmTradeKind::Buy, r.opt_bool()?)
            } else {
                (AmmTradeKind::BuyV2, None)
            };
            trade(
                kind,
                TradeAmounts::ExactTokensOut {
                    amount,
                    max_quote_cost,
                },
                tv,
            )
        }
        amm_ix::BUY_EXACT_QUOTE_IN | amm_ix::BUY_EXACT_QUOTE_IN_V2 => {
            let spendable_quote_in = r.u64()?;
            let min_tokens_out = r.u64()?;
            let (kind, tv) = if d == amm_ix::BUY_EXACT_QUOTE_IN {
                (AmmTradeKind::BuyExactQuoteIn, r.opt_bool()?)
            } else {
                (AmmTradeKind::BuyExactQuoteInV2, None)
            };
            trade(
                kind,
                TradeAmounts::ExactQuoteIn {
                    spendable_quote_in,
                    min_tokens_out,
                },
                tv,
            )
        }
        amm_ix::SELL | amm_ix::SELL_V2 => {
            let amount = r.u64()?;
            let min_quote_out = r.u64()?;
            let kind = if d == amm_ix::SELL {
                AmmTradeKind::Sell
            } else {
                AmmTradeKind::SellV2
            };
            trade(
                kind,
                TradeAmounts::Sell {
                    amount,
                    min_quote_out,
                },
                None,
            )
        }
        amm_ix::CREATE_POOL => {
            let index = r.u16()?;
            let base_amount_in = r.u64()?;
            let quote_amount_in = r.u64()?;
            let coin_creator = r.opt_pubkey()?;
            let is_mayhem_mode = r.opt_bool()?;
            AmmInstruction::CreatePool(AmmCreatePool {
                index,
                base_amount_in,
                quote_amount_in,
                coin_creator,
                is_mayhem_mode,
                pool: a(0),
                creator: a(2),
                base_mint: a(3),
                quote_mint: a(4),
            })
        }
        other => AmmInstruction::Other {
            discriminator: other,
        },
    })
}

// ================================================================ building
#[inline]
fn w(pk: Pubkey) -> AccountMeta {
    AccountMeta::new(pk, false)
}
#[inline]
fn ro(pk: Pubkey) -> AccountMeta {
    AccountMeta::new_readonly(pk, false)
}

/// Encode `disc || u64 || u64 [|| bool]` into `out`; returns the length (24 or 25).
#[inline]
pub fn encode_u64_u64_opt_bool(
    out: &mut [u8; 25],
    disc: [u8; 8],
    a: u64,
    b: u64,
    flag: Option<bool>,
) -> usize {
    out[..8].copy_from_slice(&disc);
    out[8..16].copy_from_slice(&a.to_le_bytes());
    out[16..24].copy_from_slice(&b.to_le_bytes());
    match flag {
        Some(f) => {
            out[24] = f as u8;
            25
        }
        None => 24,
    }
}

fn data_u64_u64_opt_bool(disc: [u8; 8], a: u64, b: u64, flag: Option<bool>) -> Vec<u8> {
    let mut buf = [0u8; 25];
    let n = encode_u64_u64_opt_bool(&mut buf, disc, a, b, flag);
    buf[..n].to_vec()
}

/// `buy_v3` instruction data: `07051dc4f5176550 || amount || max_sol_cost [|| partial_fill]`.
pub fn buy_v3_data(amount: u64, max_sol_cost: u64, partial_fill: Option<bool>) -> Vec<u8> {
    data_u64_u64_opt_bool(ix::BUY_V3, amount, max_sol_cost, partial_fill)
}
/// `buy_exact_quote_in_v3` data.
pub fn buy_exact_quote_in_v3_data(
    spendable_quote_in: u64,
    min_tokens_out: u64,
    partial_fill: Option<bool>,
) -> Vec<u8> {
    data_u64_u64_opt_bool(
        ix::BUY_EXACT_QUOTE_IN_V3,
        spendable_quote_in,
        min_tokens_out,
        partial_fill,
    )
}
/// `sell_v3` data.
pub fn sell_v3_data(amount: u64, min_sol_output: u64) -> Vec<u8> {
    data_u64_u64_opt_bool(ix::SELL_V3, amount, min_sol_output, None)
}

/// The 17 keys of a v3 trade on a **SOL-paired** coin, pure functions of
/// `(mint, user, base_token_program, buyback_fee_recipient)`. Derive once per
/// mint (5 PDA searches) and reuse for every buy/sell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SolV3Keys {
    pub mint: Pubkey,
    pub base_token_program: Pubkey,
    pub bonding_curve: Pubkey,
    pub associated_base_bonding_curve: Pubkey,
    pub associated_quote_bonding_curve: Pubkey,
    pub user: Pubkey,
    pub associated_base_user: Pubkey,
    pub associated_quote_user: Pubkey,
    pub user_volume_accumulator: Pubkey,
    /// One of [`c::BUYBACK_FEE_RECIPIENTS`] (the wallet itself on SOL coins).
    pub buyback_fee_recipient: Pubkey,
}

/// Per-wallet keys of v3 trades, derived once per wallet at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserKeys {
    pub user: Pubkey,
    /// `["user_volume_accumulator", user]`.
    pub user_volume_accumulator: Pubkey,
    /// The user's wrapped-SOL ATA (account #10 of SOL-coin v3 trades; unused there).
    pub wsol_ata: Pubkey,
}

impl UserKeys {
    pub fn derive(user: Pubkey) -> Self {
        Self {
            user,
            user_volume_accumulator: pda::user_volume_accumulator(&user),
            wsol_ata: pda::ata_spl(&user, &c::WSOL_MINT),
        }
    }
}

impl SolV3Keys {
    /// Derive all PDAs/ATAs (6 PDA searches, ~50-60 us in release on the
    /// build box). `base_token_program` is Token-2022 for `create_v2` coins
    /// and SPL Token for legacy `create` coins.
    pub fn derive(
        mint: Pubkey,
        user: Pubkey,
        base_token_program: Pubkey,
        buyback_fee_recipient: Pubkey,
    ) -> Self {
        let bonding_curve = pda::bonding_curve(&mint);
        let associated_base_bonding_curve =
            pda::associated_token_address(&bonding_curve, &mint, &base_token_program);
        Self::from_curve(
            mint,
            bonding_curve,
            associated_base_bonding_curve,
            &UserKeys::derive(user),
            base_token_program,
            buyback_fee_recipient,
        )
    }

    /// Hot-path variant: the curve and its base token account come straight
    /// from a `create`/`create_v2` instruction (accounts #2 and #3) and the
    /// wallet keys from a [`UserKeys`] cache, leaving 2 PDA searches (the
    /// curve's wSOL ATA, which the IDL constrains by seeds, and the user's
    /// base ATA).
    pub fn from_curve(
        mint: Pubkey,
        bonding_curve: Pubkey,
        associated_base_bonding_curve: Pubkey,
        user: &UserKeys,
        base_token_program: Pubkey,
        buyback_fee_recipient: Pubkey,
    ) -> Self {
        Self {
            mint,
            base_token_program,
            bonding_curve,
            associated_base_bonding_curve,
            associated_quote_bonding_curve: pda::ata_spl(&bonding_curve, &c::WSOL_MINT),
            user: user.user,
            associated_base_user: pda::associated_token_address(
                &user.user,
                &mint,
                &base_token_program,
            ),
            associated_quote_user: user.wsol_ata,
            user_volume_accumulator: user.user_volume_accumulator,
            buyback_fee_recipient,
        }
    }

    /// The four bumps that set the snipe's compute cost (hash-only recovery
    /// from these keys; see [`pda::SnipeBumps`]). `None` if a key is not
    /// the canonical PDA of its seeds.
    pub fn snipe_bumps(&self) -> Option<pda::SnipeBumps> {
        pda::SnipeBumps::of_keys(
            &self.mint,
            &self.bonding_curve,
            &self.associated_base_bonding_curve,
            &self.associated_quote_bonding_curve,
            &self.user,
            &self.associated_base_user,
            &self.base_token_program,
        )
    }

    /// [`Self::from_curve`] from a parsed create instruction; `None` if one of
    /// the needed accounts is unresolved (ALT) or the coin is not SOL-quoted.
    pub fn from_create(
        create: &PumpCreate<'_>,
        user: &UserKeys,
        buyback_fee_recipient: Pubkey,
    ) -> Option<Self> {
        if create.is_sol_quote != Some(true) {
            return None;
        }
        Some(Self::from_curve(
            create.mint?,
            create.bonding_curve?,
            create.associated_bonding_curve?,
            user,
            create.token_program?,
            buyback_fee_recipient,
        ))
    }

    /// The 17 account metas, in the order of TRADE_V3.md:
    ///
    /// | # | account | flags |
    /// |---|---|---|
    /// | 0 | `global` (`["global"]`) | ro |
    /// | 1 | `base_mint` | ro |
    /// | 2 | `quote_mint` = wrapped SOL for SOL coins | ro |
    /// | 3 | `base_token_program` (Token-2022 / SPL Token) | ro |
    /// | 4 | `quote_token_program` = SPL Token for wSOL | ro |
    /// | 5 | `bonding_curve` (`["bonding-curve", mint]`) | w |
    /// | 6 | `associated_base_bonding_curve` (ATA of curve, base) | w |
    /// | 7 | `associated_quote_bonding_curve` (curve's wSOL ATA; unused, need not exist) | w |
    /// | 8 | `user` | signer, w |
    /// | 9 | `associated_base_user` (**must exist**: prepend an idempotent ATA create) | w |
    /// | 10 | `associated_quote_user` (user's wSOL ATA; unused, need not exist) | w |
    /// | 11 | `user_volume_accumulator` (`["user_volume_accumulator", user]`, created if missing) | w |
    /// | 12 | `fee_config` (Pump Fees `["fee_config", pump]`) | ro |
    /// | 13 | `buyback_fee_recipient` (a listed buyback wallet; must exist) | w |
    /// | 14 | `system_program` | ro |
    /// | 15 | `event_authority` (`["__event_authority"]`) | ro |
    /// | 16 | `program` (pump) | ro |
    pub fn account_metas(&self) -> Vec<AccountMeta> {
        vec![
            ro(c::GLOBAL),
            ro(self.mint),
            ro(c::WSOL_MINT),
            ro(self.base_token_program),
            ro(c::SPL_TOKEN_PROGRAM_ID),
            w(self.bonding_curve),
            w(self.associated_base_bonding_curve),
            w(self.associated_quote_bonding_curve),
            AccountMeta::new(self.user, true),
            w(self.associated_base_user),
            w(self.associated_quote_user),
            w(self.user_volume_accumulator),
            ro(c::FEE_CONFIG),
            w(self.buyback_fee_recipient),
            ro(c::SYSTEM_PROGRAM_ID),
            ro(c::EVENT_AUTHORITY),
            ro(c::PUMP_PROGRAM_ID),
        ]
    }
}

/// `buy_v3`: exactly `amount` tokens for at most `max_sol_cost` lamports
/// (fees included). `partial_fill` matters only on mayhem coins.
pub fn buy_v3(
    keys: &SolV3Keys,
    amount: u64,
    max_sol_cost: u64,
    partial_fill: Option<bool>,
) -> Instruction {
    Instruction {
        program_id: c::PUMP_PROGRAM_ID,
        accounts: keys.account_metas(),
        data: buy_v3_data(amount, max_sol_cost, partial_fill),
    }
}

/// `buy_exact_quote_in_v3`: spend `spendable_sol_in` (fees included) for at least `min_tokens_out`.
pub fn buy_exact_quote_in_v3(
    keys: &SolV3Keys,
    spendable_sol_in: u64,
    min_tokens_out: u64,
    partial_fill: Option<bool>,
) -> Instruction {
    Instruction {
        program_id: c::PUMP_PROGRAM_ID,
        accounts: keys.account_metas(),
        data: buy_exact_quote_in_v3_data(spendable_sol_in, min_tokens_out, partial_fill),
    }
}

/// `sell_v3`: sell `amount` tokens for at least `min_sol_output` (after fees).
pub fn sell_v3(keys: &SolV3Keys, amount: u64, min_sol_output: u64) -> Instruction {
    Instruction {
        program_id: c::PUMP_PROGRAM_ID,
        accounts: keys.account_metas(),
        data: sell_v3_data(amount, min_sol_output),
    }
}

/// Keys of a legacy `buy` / `buy_exact_sol_in` / `sell`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyTradeKeys {
    pub mint: Pubkey,
    pub token_program: Pubkey,
    pub bonding_curve: Pubkey,
    pub associated_bonding_curve: Pubkey,
    pub user: Pubkey,
    pub associated_user: Pubkey,
    /// `["creator-vault", BondingCurve.creator]`.
    pub creator_vault: Pubkey,
    /// Normal recipient ([`c::FEE_RECIPIENTS`]); a reserved one for mayhem coins.
    pub fee_recipient: Pubkey,
    /// Remaining account #2 (writable).
    pub buyback_fee_recipient: Pubkey,
    pub user_volume_accumulator: Pubkey,
    /// Remaining account #1 (read-only): `["bonding-curve-v2", mint]`.
    pub bonding_curve_v2: Pubkey,
}

impl LegacyTradeKeys {
    /// Derive every PDA. `creator` is the *current* `BondingCurve.creator`:
    /// `CreateEvent.creator` right after the create (not the `create`
    /// argument, which holder-reward coins override), until a pump-fees
    /// `migrate_bonding_curve_creator` moves it to `pda::fee_sharing_config(mint)`.
    pub fn derive(
        mint: Pubkey,
        user: Pubkey,
        creator: Pubkey,
        token_program: Pubkey,
        fee_recipient: Pubkey,
        buyback_fee_recipient: Pubkey,
    ) -> Self {
        let bonding_curve = pda::bonding_curve(&mint);
        Self {
            mint,
            token_program,
            bonding_curve,
            associated_bonding_curve: pda::associated_token_address(
                &bonding_curve,
                &mint,
                &token_program,
            ),
            user,
            associated_user: pda::associated_token_address(&user, &mint, &token_program),
            creator_vault: pda::creator_vault(&creator),
            fee_recipient,
            buyback_fee_recipient,
            user_volume_accumulator: pda::user_volume_accumulator(&user),
            bonding_curve_v2: pda::bonding_curve_v2(&mint),
        }
    }

    /// IDL `buy` accounts (16) + `bonding_curve_v2` (ro) + `buyback_fee_recipient` (w).
    pub fn buy_metas(&self) -> Vec<AccountMeta> {
        vec![
            ro(c::GLOBAL),
            w(self.fee_recipient),
            ro(self.mint),
            w(self.bonding_curve),
            w(self.associated_bonding_curve),
            w(self.associated_user),
            AccountMeta::new(self.user, true),
            ro(c::SYSTEM_PROGRAM_ID),
            ro(self.token_program),
            w(self.creator_vault),
            ro(c::EVENT_AUTHORITY),
            ro(c::PUMP_PROGRAM_ID),
            ro(c::GLOBAL_VOLUME_ACCUMULATOR),
            w(self.user_volume_accumulator),
            ro(c::FEE_CONFIG),
            ro(c::PUMP_FEES_PROGRAM_ID),
            ro(self.bonding_curve_v2),
            w(self.buyback_fee_recipient),
        ]
    }

    /// IDL `sell` accounts (14) + [`user_volume_accumulator` (w) on cashback coins]
    /// + `bonding_curve_v2` (ro) + `buyback_fee_recipient` (w).
    pub fn sell_metas(&self, is_cashback_coin: bool) -> Vec<AccountMeta> {
        let mut v = vec![
            ro(c::GLOBAL),
            w(self.fee_recipient),
            ro(self.mint),
            w(self.bonding_curve),
            w(self.associated_bonding_curve),
            w(self.associated_user),
            AccountMeta::new(self.user, true),
            ro(c::SYSTEM_PROGRAM_ID),
            w(self.creator_vault),
            ro(self.token_program),
            ro(c::EVENT_AUTHORITY),
            ro(c::PUMP_PROGRAM_ID),
            ro(c::FEE_CONFIG),
            ro(c::PUMP_FEES_PROGRAM_ID),
        ];
        if is_cashback_coin {
            v.push(w(self.user_volume_accumulator));
        }
        v.push(ro(self.bonding_curve_v2));
        v.push(w(self.buyback_fee_recipient));
        v
    }
}

/// Encode `disc || u64 || u64 [|| bool [|| bool]]` (legacy buy args).
fn data_legacy_buy(
    disc: [u8; 8],
    a: u64,
    b: u64,
    track_volume: Option<bool>,
    partial_fill: Option<bool>,
) -> Result<Vec<u8>> {
    let mut v = Vec::with_capacity(26);
    v.extend_from_slice(&disc);
    v.extend_from_slice(&a.to_le_bytes());
    v.extend_from_slice(&b.to_le_bytes());
    match (track_volume, partial_fill) {
        (None, None) => {}
        (Some(t), None) => v.push(t as u8),
        (Some(t), Some(p)) => {
            v.push(t as u8);
            v.push(p as u8);
        }
        // Borsh trailing options cannot skip a middle one.
        (None, Some(_)) => {
            return Err(DecodeError::InvalidTransaction(
                "partial_fill requires track_volume",
            ))
        }
    }
    Ok(v)
}

/// Legacy `buy` (exact tokens out) with the 2 undocumented remaining accounts.
pub fn legacy_buy(
    keys: &LegacyTradeKeys,
    amount: u64,
    max_sol_cost: u64,
    track_volume: Option<bool>,
    partial_fill: Option<bool>,
) -> Result<Instruction> {
    Ok(Instruction {
        program_id: c::PUMP_PROGRAM_ID,
        accounts: keys.buy_metas(),
        data: data_legacy_buy(ix::BUY, amount, max_sol_cost, track_volume, partial_fill)?,
    })
}

/// Legacy `buy_exact_sol_in` (same accounts as `buy`).
pub fn legacy_buy_exact_sol_in(
    keys: &LegacyTradeKeys,
    spendable_sol_in: u64,
    min_tokens_out: u64,
    track_volume: Option<bool>,
    partial_fill: Option<bool>,
) -> Result<Instruction> {
    Ok(Instruction {
        program_id: c::PUMP_PROGRAM_ID,
        accounts: keys.buy_metas(),
        data: data_legacy_buy(
            ix::BUY_EXACT_SOL_IN,
            spendable_sol_in,
            min_tokens_out,
            track_volume,
            partial_fill,
        )?,
    })
}

/// Legacy `sell` with the remaining accounts.
pub fn legacy_sell(
    keys: &LegacyTradeKeys,
    amount: u64,
    min_sol_output: u64,
    is_cashback_coin: bool,
) -> Instruction {
    let mut data = Vec::with_capacity(24);
    data.extend_from_slice(&ix::SELL);
    data.extend_from_slice(&amount.to_le_bytes());
    data.extend_from_slice(&min_sol_output.to_le_bytes());
    Instruction {
        program_id: c::PUMP_PROGRAM_ID,
        accounts: keys.sell_metas(is_cashback_coin),
        data,
    }
}

// ----------------------------------------------------- generic Solana ixs
/// ATA program `CreateIdempotent` (data `[1]`): creates `owner`'s ATA for
/// `mint` under `token_program` if missing. Use Token-2022 for `create_v2` coins.
pub fn create_associated_token_account_idempotent(
    payer: &Pubkey,
    owner: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
) -> Instruction {
    let ata = pda::associated_token_address(owner, mint, token_program);
    create_associated_token_account_idempotent_with_address(payer, owner, mint, token_program, &ata)
}

/// As [`create_associated_token_account_idempotent`] with a precomputed ATA address.
pub fn create_associated_token_account_idempotent_with_address(
    payer: &Pubkey,
    owner: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
    ata: &Pubkey,
) -> Instruction {
    Instruction {
        program_id: c::ASSOCIATED_TOKEN_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*payer, true),
            w(*ata),
            ro(*owner),
            ro(*mint),
            ro(c::SYSTEM_PROGRAM_ID),
            ro(*token_program),
        ],
        data: vec![1],
    }
}

/// Compute Budget `SetComputeUnitLimit` (tag 2, u32 LE).
pub fn set_compute_unit_limit(units: u32) -> Instruction {
    let mut data = vec![2u8];
    data.extend_from_slice(&units.to_le_bytes());
    Instruction {
        program_id: c::COMPUTE_BUDGET_PROGRAM_ID,
        accounts: vec![],
        data,
    }
}

/// Compute Budget `SetComputeUnitPrice` (tag 3, u64 LE micro-lamports per CU).
pub fn set_compute_unit_price(micro_lamports: u64) -> Instruction {
    let mut data = vec![3u8];
    data.extend_from_slice(&micro_lamports.to_le_bytes());
    Instruction {
        program_id: c::COMPUTE_BUDGET_PROGRAM_ID,
        accounts: vec![],
        data,
    }
}

/// Compute Budget `SetLoadedAccountsDataSizeLimit` (tag 4, u32 LE bytes).
pub fn set_loaded_accounts_data_size_limit(bytes: u32) -> Instruction {
    let mut data = vec![4u8];
    data.extend_from_slice(&bytes.to_le_bytes());
    Instruction {
        program_id: c::COMPUTE_BUDGET_PROGRAM_ID,
        accounts: vec![],
        data,
    }
}

/// System `Transfer` (u32 LE tag 2, u64 LE lamports).
pub fn system_transfer(from: &Pubkey, to: &Pubkey, lamports: u64) -> Instruction {
    let mut data = Vec::with_capacity(12);
    data.extend_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(&lamports.to_le_bytes());
    Instruction {
        program_id: c::SYSTEM_PROGRAM_ID,
        accounts: vec![AccountMeta::new(*from, true), w(*to)],
        data,
    }
}

/// Tip transfer to Jito tip account `JITO_TIP_ACCOUNTS[index % 8]`.
pub fn jito_tip(from: &Pubkey, index: usize, lamports: u64) -> Instruction {
    system_transfer(
        from,
        &c::JITO_TIP_ACCOUNTS[index % c::JITO_TIP_ACCOUNTS.len()],
        lamports,
    )
}

/// System `AdvanceNonceAccount` (u32 LE tag 4). Must be the first instruction
/// of a durable-nonce transaction; the tx's blockhash is the nonce value.
pub fn advance_nonce_account(nonce_account: &Pubkey, nonce_authority: &Pubkey) -> Instruction {
    Instruction {
        program_id: c::SYSTEM_PROGRAM_ID,
        accounts: vec![
            w(*nonce_account),
            ro(c::RECENT_BLOCKHASHES_SYSVAR_ID),
            AccountMeta::new_readonly(*nonce_authority, true),
        ],
        data: 4u32.to_le_bytes().to_vec(),
    }
}

/// The usual v3 snipe body: idempotent Token-2022 (or SPL) ATA create for
/// the user's base account, then `buy_v3`. Prepend compute budget / nonce
/// and append a tip as needed.
pub fn buy_v3_with_ata(
    keys: &SolV3Keys,
    amount: u64,
    max_sol_cost: u64,
    partial_fill: Option<bool>,
) -> [Instruction; 2] {
    [
        create_associated_token_account_idempotent_with_address(
            &keys.user,
            &keys.user,
            &keys.mint,
            &keys.base_token_program,
            &keys.associated_base_user,
        ),
        buy_v3(keys, amount, max_sol_cost, partial_fill),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `create_v2` argument bytes of a live launch (slot 460008810,
    /// 2026-10-22, a rapidlaunch coin): after the creator the tail is
    /// `00 00 01` = mayhem false, cashback false, holder reward true, with no
    /// `creator_fee_bps`. Before the tolerant tail this was a decode miss.
    #[test]
    fn create_v2_args_with_a_holder_reward_tail_and_no_fee() {
        let mut d = Vec::new();
        for s in ["Poopy", "POOP", "https://m.rapidlaunch.io/m/3uRJc0aJ4"] {
            d.extend_from_slice(&(s.len() as u32).to_le_bytes());
            d.extend_from_slice(s.as_bytes());
        }
        let creator: [u8; 32] = [
            0xb4, 0x85, 0x6f, 0xee, 0xab, 0xd3, 0x48, 0xc6, 0x0c, 0x43, 0x10, 0x02, 0x13, 0x00,
            0x1b, 0x03, 0xf4, 0xb8, 0xc3, 0xbb, 0x25, 0xe6, 0xc0, 0xf5, 0x3a, 0x2d, 0x4d, 0xac,
            0xf5, 0x60, 0xc5, 0xbf,
        ];
        d.extend_from_slice(&creator);
        d.extend_from_slice(&[0x00, 0x00, 0x01]);
        assert_eq!(d.len(), 92);
        let mut r = Reader::new(&d);
        let a = read_create_args(&mut r, true).unwrap();
        assert_eq!((a.name, a.symbol), ("Poopy", "POOP"));
        assert_eq!(a.creator, Pubkey::new_from_array(creator));
        assert!(!a.is_mayhem_mode);
        assert_eq!(a.is_cashback_enabled, Some(false));
        assert_eq!(a.creator_fee_bps, None);
        assert_eq!(a.is_holder_reward, Some(true));
        // the shapes that already parsed keep their meaning
        for (tail, cash, fee, holder) in [
            (vec![0x00u8], None, None, None),
            (vec![0x00, 0x01], Some(true), None, None),
            (
                vec![0x00, 0x00, 30, 0, 0, 0, 0, 0, 0, 0, 0x01],
                Some(false),
                Some(30),
                Some(true),
            ),
        ] {
            let mut d2 = d[..d.len() - 3].to_vec();
            d2.extend_from_slice(&tail);
            let mut r2 = Reader::new(&d2);
            let a2 = read_create_args(&mut r2, true).unwrap();
            assert_eq!(
                (
                    a2.is_cashback_enabled,
                    a2.creator_fee_bps,
                    a2.is_holder_reward
                ),
                (cash, fee, holder)
            );
        }
    }

    #[test]
    fn buy_v3_data_bytes() {
        let d = buy_v3_data(1_000_000, 10_000_000, Some(true));
        let hex: String = d.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "07051dc4f5176550".to_owned() + "40420f0000000000" + "8096980000000000" + "01"
        );
        assert_eq!(buy_v3_data(1, 2, None).len(), 24);
    }

    #[test]
    fn parse_round_trips_v3() {
        let mint = Pubkey::new_unique();
        let user = Pubkey::new_unique();
        let keys = SolV3Keys::derive(
            mint,
            user,
            c::TOKEN_2022_PROGRAM_ID,
            c::BUYBACK_FEE_RECIPIENTS[0],
        );
        let ixn = buy_v3(&keys, 5, 6, Some(false));
        let accts: Vec<Pubkey> = ixn.accounts.iter().map(|m| m.pubkey).collect();
        match classify(&ixn.program_id, &ixn.data, accts.as_slice()).unwrap() {
            Classified::Pump(PumpInstruction::Trade(t)) => {
                assert_eq!(t.kind, PumpTradeKind::BuyV3);
                assert_eq!(
                    t.amounts,
                    TradeAmounts::ExactTokensOut {
                        amount: 5,
                        max_quote_cost: 6
                    }
                );
                assert_eq!(t.partial_fill, Some(false));
                assert_eq!(t.mint, Some(mint));
                assert_eq!(t.user, Some(user));
                assert_eq!(t.bonding_curve, Some(pda::bonding_curve(&mint)));
                assert_eq!(t.quote_mint, Some(c::WSOL_MINT));
                assert_eq!(t.buyback_fee_recipient, Some(c::BUYBACK_FEE_RECIPIENTS[0]));
            }
            other => panic!("{other:?}"),
        }
        // Same bytes under the PumpSwap program are NOT a pump buy_v3.
        let r = classify(&c::PUMP_SWAP_PROGRAM_ID, &ixn.data, accts.as_slice()).unwrap();
        assert!(matches!(
            r,
            Classified::PumpSwap(AmmInstruction::Other { .. })
        ));
    }

    #[test]
    fn legacy_buy_rejects_skipped_option() {
        let k = LegacyTradeKeys::derive(
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            c::TOKEN_2022_PROGRAM_ID,
            c::FEE_RECIPIENTS[0],
            c::BUYBACK_FEE_RECIPIENTS[0],
        );
        assert!(legacy_buy(&k, 1, 2, None, Some(true)).is_err());
        assert_eq!(
            legacy_buy(&k, 1, 2, Some(true), Some(true))
                .unwrap()
                .data
                .len(),
            26
        );
        assert_eq!(legacy_sell(&k, 1, 2, true).accounts.len(), 17);
        assert_eq!(legacy_sell(&k, 1, 2, false).accounts.len(), 16);
    }
}
