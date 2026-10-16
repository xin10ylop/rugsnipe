//! Recovery before READY (docs/06 sections 12, 14 invariant I6): load the
//! open positions and nonce states from `state.db`, reconcile them against
//! the chain, and mark orphans.
//!
//! * **positions**: a DB position whose token balance is zero on chain is
//!   closed; a different balance resizes it (size always comes from the
//!   chain); a non-zero token account with no DB position becomes an
//!   `orphan` position (it still needs an exit owner, I2/I6);
//! * **nonces** (seeding the pool from chain): each pool account is read; a
//!   missing, uninitialized or foreign-authority account is `quarantined`
//!   (never fired on); a value different from the DB
//!   means an advance landed (a buy variant or a burn) while we were down,
//!   so the slot becomes `free` with the new value and
//!   `last_change_slot = now` (the no-double-buy rule then refuses every
//!   create older than now, docs/06 section 7.3);
//! * **paper** (no chain, mode `paper`): simulated positions of an earlier
//!   run cannot be resumed and are closed as abandoned.

use crate::chain::{ChainRead, TokenBalance};
use crate::ledger::Ledger;
use crate::rows::*;
use crate::{Result, StateError};
use pump_codec::constants::WSOL_MINT;
use pump_codec::Pubkey;
use rusqlite::Connection;

/// Inputs of [`recover`].
pub struct RecoverCtx<'a> {
    /// Hot wallet (token owner); `None` in paper mode.
    pub owner: Option<Pubkey>,
    /// `paper`, `probe` or `live`.
    pub mode: &'a str,
    /// Nonce pool accounts `(pubkey, pool, idx)`.
    pub nonce_accounts: &'a [(Pubkey, &'static str, u16)],
    pub chain: Option<&'a dyn ChainRead>,
}

/// What recovery found and did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecoveryReport {
    /// Positions still open after reconciliation (orphans included).
    pub open_positions: Vec<PositionRow>,
    /// Nonce rows after reconciliation.
    pub nonces: Vec<NonceRow>,
    pub orphans: Vec<TokenBalance>,
    /// DB positions with no tokens on chain (closed now).
    pub closed_missing: Vec<Pubkey>,
    /// `(mint, db_tokens, chain_tokens)`.
    pub resized: Vec<(Pubkey, u64, u64)>,
    /// Paper positions of an earlier run (closed now).
    pub abandoned_paper: Vec<Pubkey>,
    /// Nonce indexes whose value changed while we were down.
    pub nonce_changed: Vec<u16>,
    /// Real intents still `sent` (the keeper settles them from chain).
    pub inflight_intents: Vec<i64>,
    /// `true` when the chain was read.
    pub reconciled: bool,
    pub slot: Option<u64>,
}

fn pk(s: &str) -> rusqlite::Result<Pubkey> {
    s.parse().map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("bad pubkey {s:?}").into(),
        )
    })
}

fn hash32(s: &str) -> [u8; 32] {
    bs58::decode(s)
        .into_vec()
        .ok()
        .and_then(|v| v.try_into().ok())
        .unwrap_or([0; 32])
}

/// Open positions (`open` / `exiting`).
pub fn load_open_positions(c: &Connection) -> Result<Vec<PositionRow>> {
    let mut st = c.prepare(
        "SELECT mint, intent_id, opened_slot, tokens, cost, v_fill, peak_v, bundle_json, policy_json, origin, state,
                sell_spend, closed_slot FROM positions WHERE state IN ('open','exiting') ORDER BY mint",
    )?;
    let rows = st
        .query_map([], |r| {
            let origin: String = r.get(9)?;
            let state: String = r.get(10)?;
            Ok(PositionRow {
                mint: pk(&r.get::<_, String>(0)?)?,
                intent_id: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                opened_slot: r.get::<_, Option<i64>>(2)?.unwrap_or(0) as u64,
                tokens: r.get::<_, Option<i64>>(3)?.unwrap_or(0) as u64,
                cost: r.get::<_, Option<i64>>(4)?.unwrap_or(0) as u64,
                v_fill: r.get::<_, Option<i64>>(5)?.unwrap_or(0) as u64,
                peak_v: r.get::<_, Option<i64>>(6)?.unwrap_or(0) as u64,
                bundle_json: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                policy_json: r.get::<_, Option<String>>(8)?.unwrap_or_default(),
                origin: if origin == "orphan" {
                    Origin::Orphan
                } else {
                    Origin::Intent
                },
                state: PosState::parse(&state).unwrap_or(PosState::Open),
                sell_spend: r.get::<_, Option<i64>>(11)?.unwrap_or(0) as u64,
                closed_slot: r.get::<_, Option<i64>>(12)?.map(|x| x as u64),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Every nonce row.
pub fn load_nonces(c: &Connection) -> Result<Vec<NonceRow>> {
    let mut st = c.prepare(
        "SELECT pubkey, pool, idx, value, last_change_slot, state, intent_id, since_slot FROM nonces ORDER BY pool, idx",
    )?;
    let rows = st
        .query_map([], |r| {
            let pool: String = r.get(1)?;
            let state: String = r.get(5)?;
            Ok(NonceRow {
                pubkey: pk(&r.get::<_, String>(0)?)?,
                pool: if pool == "sell" { "sell" } else { "buy" },
                idx: r.get::<_, i64>(2)? as u16,
                value: hash32(&r.get::<_, Option<String>>(3)?.unwrap_or_default()),
                last_change_slot: r.get::<_, Option<i64>>(4)?.map(|x| x as u64),
                state: NonceRowState::parse(&state).unwrap_or(NonceRowState::Quarantined),
                intent_id: r.get(6)?,
                since_slot: r.get::<_, Option<i64>>(7)?.map(|x| x as u64),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Run recovery. Writes go through `ledger` and are flushed before return.
pub fn recover(c: &Connection, ledger: &Ledger, ctx: &RecoverCtx<'_>) -> Result<RecoveryReport> {
    let mut rep = RecoveryReport::default();
    let mut positions = load_open_positions(c)?;
    let mut nonces = load_nonces(c)?;
    {
        let mut st = c.prepare("SELECT intent_id FROM intents WHERE state = 'sent'")?;
        rep.inflight_intents = st
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<i64>>>()?;
    }

    match (ctx.chain, ctx.owner) {
        (Some(chain), Some(owner)) => {
            let slot = chain.slot().map_err(StateError::Chain)?;
            rep.slot = Some(slot);
            let balances: Vec<TokenBalance> = chain
                .token_balances(&owner)
                .map_err(StateError::Chain)?
                .into_iter()
                .filter(|b| b.mint != WSOL_MINT)
                .collect();
            for p in positions.iter_mut() {
                let chain_amt: u64 = balances
                    .iter()
                    .filter(|b| b.mint == p.mint)
                    .map(|b| b.amount)
                    .sum();
                if chain_amt == 0 {
                    p.state = PosState::Closed;
                    p.closed_slot = Some(slot);
                    p.tokens = 0;
                    rep.closed_missing.push(p.mint);
                    ledger.position(p.clone());
                } else if chain_amt != p.tokens {
                    rep.resized.push((p.mint, p.tokens, chain_amt));
                    p.tokens = chain_amt;
                    ledger.position(p.clone());
                }
            }
            for b in &balances {
                if positions.iter().any(|p| p.mint == b.mint) {
                    continue;
                }
                rep.orphans.push(*b);
                let row = PositionRow {
                    mint: b.mint,
                    intent_id: 0,
                    opened_slot: slot,
                    tokens: b.amount,
                    cost: 0,
                    v_fill: 0,
                    peak_v: 0,
                    bundle_json: "[]".into(),
                    policy_json: String::new(),
                    origin: Origin::Orphan,
                    state: PosState::Open,
                    sell_spend: 0,
                    closed_slot: None,
                };
                ledger.position(row.clone());
                positions.push(row);
            }
            // nonces
            let keys: Vec<Pubkey> = ctx.nonce_accounts.iter().map(|n| n.0).collect();
            if !keys.is_empty() {
                let datas = chain.accounts(&keys).map_err(StateError::Chain)?;
                for ((key, pool, idx), data) in ctx.nonce_accounts.iter().zip(datas) {
                    // a nonce held by another authority can never be advanced
                    // by our variants or burns: treat it like a missing one
                    let parsed = data
                        .as_deref()
                        .and_then(rugsnipe_tx::nonce::parse_nonce_account)
                        .filter(|(auth, _, _)| *auth == owner);
                    let existing = nonces.iter().position(|n| n.pubkey == *key);
                    let row = match (parsed, existing) {
                        (None, Some(i)) => {
                            // account missing or not initialized: never fire on it
                            let mut r = nonces[i].clone();
                            r.state = NonceRowState::Quarantined;
                            r
                        }
                        (None, None) => NonceRow {
                            pubkey: *key,
                            pool,
                            idx: *idx,
                            value: [0; 32],
                            last_change_slot: None,
                            state: NonceRowState::Quarantined,
                            intent_id: None,
                            since_slot: None,
                        },
                        (Some((_auth, value, _fee)), Some(i)) => {
                            let mut r = nonces[i].clone();
                            if r.value != value {
                                rep.nonce_changed.push(*idx);
                                r.value = value;
                                r.last_change_slot = Some(slot);
                                if r.state != NonceRowState::Quarantined {
                                    r.state = NonceRowState::Free;
                                }
                                r.intent_id = None;
                                r.since_slot = None;
                            }
                            r
                        }
                        (Some((_auth, value, _fee)), None) => NonceRow {
                            pubkey: *key,
                            pool,
                            idx: *idx,
                            value,
                            // first sight: treat as changed now (never fire on older creates)
                            last_change_slot: Some(slot),
                            state: NonceRowState::Free,
                            intent_id: None,
                            since_slot: None,
                        },
                    };
                    ledger.nonce(row.clone());
                    match existing {
                        Some(i) => nonces[i] = row,
                        None => nonces.push(row),
                    }
                }
            }
            rep.reconciled = true;
        }
        _ => {
            if ctx.mode == "paper" {
                for p in positions.iter_mut() {
                    p.state = PosState::Closed;
                    rep.abandoned_paper.push(p.mint);
                    ledger.position(p.clone());
                }
            }
        }
    }
    rep.open_positions = positions
        .into_iter()
        .filter(|p| p.state != PosState::Closed)
        .collect();
    rep.nonces = nonces;
    ledger.flush();
    Ok(rep)
}
