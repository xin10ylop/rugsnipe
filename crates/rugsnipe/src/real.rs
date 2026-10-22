//! The real fill lane (probe; docs/06 sections 7.3, 8 and 10): a position
//! opens only on **our own** TradeEvent (our wallet as the buyer) or a
//! confirmed token balance, never on a send.
//!
//! One async task on the cold thread's runtime owns the lane's JSON-RPC
//! connection (inside its own [`RpcBudget`]) and every in-flight attempt:
//!
//! 1. every sent variant's signature is polled with `getSignatureStatuses`
//!    (one call for all of them, every [`POLL_MS`]);
//! 2. a confirmed one is fetched (`getTransaction`, `maxSupportedTransactionVersion = 1`):
//!    * **landed and succeeded**: our TradeEvent (pump-codec events) gives the
//!      tokens, the SOL paid and the post-trade reserves; the landing is
//!      ranked in its slot ([`analyze_landing`]: `getSignaturesForAddress` on
//!      the curve, `getBlock` signatures-only for the transaction indices,
//!      `getTransaction` for the slot's curve transactions, the E1 lane's
//!      gap-3 bundle reconstruction without our transaction): our block
//!      index, the third-party buyers ahead of and behind us, the premium
//!      over the post-bundle price;
//!    * **landed and failed**: the real cost is `meta.fee`
//!      ([`crate::e1::failed_tx_fee`]) and the error code is kept (6002 /
//!      6042 = slippage, anything else `custom_<code>` or the error name);
//! 3. **nothing landed**: the keeper burns the nonce at
//!    `since + tx.burn_after_slots` (an AdvanceNonce-only transaction, sent
//!    here); once the nonce account shows a new value and a last status check
//!    finds none of our signatures, the attempt is **dropped** (cost 0). A
//!    variant that lands after the burn started is a late fill (it opens a
//!    position like any other);
//! 4. sells: the signatures of sell variants are polled the same way; our
//!    sell TradeEvent goes to the hot loop (the exit engine closes on it), and
//!    every held mint's token balance is read (a zero balance closes too);
//! 5. the wallet balance (risk gate), a recent blockhash (real sells) and the
//!    nonce accounts of in-flight attempts are refreshed on a timer.
//!
//! The pure parts ([`own_trade`], [`fail_reason`], [`rank_in_slot`],
//! [`premium_bps`], [`parse_statuses`]) are tested on recorded shapes; the
//! task against a mock JSON-RPC server (`tests/real_lane.rs`).

use crate::e1::{analyze_slot0, block_signatures, failed_tx_fee, tx_obs, TxObs};
use crate::e2::parse_signatures;
use crate::msgs::ShadowInfo;
use base64::Engine as _;
use pump_codec::{pda, Pubkey};
use rugsnipe_state::RpcBudget;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// Status poll period.
pub const POLL_MS: u64 = 400;
/// Wallet balance, held-mint balances and blockhash refresh period.
pub const SLOW_MS: u64 = 2_000;
/// A sell signature unseen this many slots after its send is given up
/// (its blockhash has expired).
pub const SELL_GIVE_UP_SLOTS: u64 = 160;
/// Slot-analysis fetch cap (transactions per landing).
pub const MAX_TX: usize = 40;

/// Lane settings.
#[derive(Debug, Clone)]
pub struct LaneCfg {
    pub rpc_url: String,
    pub budget: Arc<RpcBudget>,
    pub owner: Pubkey,
    /// `--probe-dry`: never send a burn.
    pub dry: bool,
}

/// What the lane needs about one real intent.
#[derive(Debug, Clone, Copy)]
pub struct Pending {
    pub intent: u64,
    pub mint: Pubkey,
    pub bc: Pubkey,
    pub create_sig: [u8; 64],
    pub slot: u64,
    pub creator: Pubkey,
    pub user: Pubkey,
    pub nonce_idx: u16,
    pub nonce_account: Pubkey,
    pub nonce_value: [u8; 32],
    pub spend: u64,
}

/// One sent variant of an intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SentVar {
    pub sig: [u8; 64],
    pub variant: u8,
    pub label: &'static str,
    pub prio: u64,
    pub tip: u64,
}

/// Cold thread -> lane.
#[derive(Debug, Clone)]
pub enum LaneCmd {
    Track(Pending),
    /// A variant a real sender accepted (buy: polled for landing).
    Sent {
        intent: u64,
        var: SentVar,
    },
    /// A sell variant a real sender accepted.
    SellSent {
        mint: Pubkey,
        sig: [u8; 64],
        slot: u64,
    },
    /// Poll this mint's token balance (a real position is held).
    Held {
        intent: u64,
        mint: Pubkey,
    },
    /// The position closed: stop polling its balance.
    Released {
        mint: Pubkey,
    },
    /// Burn nonce `idx` (`value` = the value our variants carry); the
    /// attempt on it (if any) is marked as racing the burn.
    Burn {
        idx: u16,
        account: Pubkey,
        value: [u8; 32],
        signer: Arc<rugsnipe_tx::signer::HotSigner>,
    },
    /// Watch nonce `idx` until its value differs from `value`.
    WatchNonce {
        idx: u16,
        account: Pubkey,
        value: [u8; 32],
    },
    /// The hot loop's current slot.
    Slot(u64),
}

/// Rank and premium of a landed transaction in its slot.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rank {
    /// Our index in the block.
    pub tx_index: Option<u32>,
    /// Distinct third-party buyers ahead of / behind us (not the creator
    /// set, not the bundle wallets, not us); ahead includes every
    /// third-party buyer of the creation slot when we landed later.
    pub tp_ahead: u32,
    pub tp_behind: u32,
    /// SOL of the third-party buys ahead of us.
    pub rival_sol_ahead: u64,
    /// Premium over the post-bundle (bundle-only) price, bps.
    pub premium_bps: Option<u32>,
    /// The creation slot as reconstructed (shadow row).
    pub info: ShadowInfo,
    pub error: Option<String>,
}

/// Our TradeEvent in a landed transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnTrade {
    pub is_buy: bool,
    pub sol: u64,
    pub tokens: u64,
    /// Protocol + creator fee (lamports), 1.25% of the SOL when the event
    /// carries no fee fields.
    pub fee: u64,
    /// Post-trade virtual reserves.
    pub v: u64,
    pub t: u64,
}

/// A landed buy (succeeded or failed).
#[derive(Debug, Clone, PartialEq)]
pub struct Landed {
    pub intent: u64,
    pub sig: [u8; 64],
    pub var: Option<SentVar>,
    pub slot: u64,
    /// `meta.fee` (base + priority; a failed buy's whole cost).
    pub fee: u64,
    /// `None` = the buy succeeded.
    pub fail: Option<(String, String)>,
    /// Our TradeEvent (a success without one: sized from the token balance).
    pub trade: Option<OwnTrade>,
    pub rank: Rank,
    /// The burn had started (late fill).
    pub late: bool,
}

/// Lane -> cold thread.
#[derive(Debug, Clone)]
pub enum LaneEvent {
    Landed(Landed),
    /// Nothing of ours landed and the nonce moved on.
    Dropped {
        intent: u64,
        primary: Option<SentVar>,
        burned: bool,
    },
    /// Our own trade (a sell, or a buy on a held mint) from a landed sell.
    OwnTrade {
        sig: [u8; 64],
        slot: u64,
        mint: Pubkey,
        trade: OwnTrade,
    },
    /// A sell landed and failed (fee paid, position untouched).
    SellFailed {
        sig: [u8; 64],
        fee: u64,
        reason: String,
    },
    /// A held mint's token balance. `exists`: the token account was found
    /// (a missing one reads as 0 tokens; see [`balance_is_trustworthy`]).
    Balance {
        mint: Pubkey,
        tokens: u64,
        slot: u64,
        exists: bool,
    },
    /// A watched nonce now holds `value`.
    Nonce {
        idx: u16,
        value: [u8; 32],
        slot: u64,
    },
    /// The burn transaction was sent (`ok`) or not.
    BurnSent {
        idx: u16,
        sig: [u8; 64],
        ok: bool,
        err: String,
    },
    Wallet {
        lamports: u64,
    },
    Blockhash {
        hash: [u8; 32],
        slot: u64,
    },
    Error(String),
    /// RPC calls made since the last report.
    Calls(u32),
}

// ------------------------------------------------------------------ pure parts

/// `getSignatureStatuses` result: per requested signature, `Some((slot,
/// err))` once confirmed (`err` = `meta.err` JSON, `None` = success).
pub fn parse_statuses(result: &Value, n: usize) -> Vec<Option<(u64, Option<Value>)>> {
    let list = result["value"].as_array().cloned().unwrap_or_default();
    (0..n)
        .map(|i| {
            let e = list.get(i)?;
            if e.is_null() {
                return None;
            }
            let st = e["confirmationStatus"].as_str().unwrap_or("");
            if st != "confirmed" && st != "finalized" {
                return None;
            }
            let slot = e["slot"].as_u64()?;
            let err = e.get("err").filter(|x| !x.is_null()).cloned();
            Some((slot, err))
        })
        .collect()
}

/// Our TradeEvent on `mint` in a `getTransaction` result (the first one by
/// `owner`; `None` for a failed transaction or none of ours).
pub fn own_trade(result: &Value, mint: &Pubkey, owner: &Pubkey, slot: u64) -> Option<OwnTrade> {
    let trades = crate::e2::trades_from_tx(result, mint, slot);
    trades.iter().find(|t| t.user == *owner).map(|t| OwnTrade {
        is_buy: t.is_buy,
        sol: t.sol,
        tokens: t.tok,
        fee: t.sol * 125 / 10_000,
        v: t.v,
        t: t.t,
    })
}

/// Why a landed transaction failed: `(fail_reason, error text)`.
/// `6002` / `6042` (pump's slippage errors) are `slippage`; another custom
/// program error is `custom_<code>`; anything else is the error's name.
pub fn fail_reason(err: &Value) -> (String, String) {
    let text: String = err.to_string().chars().take(200).collect();
    let custom = err
        .get("InstructionError")
        .and_then(|e| e.get(1))
        .and_then(|e| e.get("Custom"))
        .and_then(|c| c.as_u64());
    let reason = match custom {
        Some(6002) | Some(6042) => "slippage".to_string(),
        Some(c) => format!("custom_{c}"),
        None => match err {
            Value::String(s) => to_snake(s),
            Value::Object(o) => o
                .keys()
                .next()
                .map(|k| to_snake(k))
                .unwrap_or_else(|| "failed".into()),
            _ => "failed".into(),
        },
    };
    (reason, text)
}

fn to_snake(s: &str) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else if ch.is_ascii_alphanumeric() {
            out.push(ch);
        }
    }
    out
}

/// Premium of a buy whose pre-trade token reserve was `t_pre` over the
/// post-bundle reference `t_ref` (price ∝ V² ∝ 1/T²), bps, at least 0.
pub fn premium_bps(t_ref: u64, t_pre: u64) -> Option<u32> {
    if t_ref == 0 || t_pre == 0 {
        return None;
    }
    let r = (t_ref as f64 / t_pre as f64).powi(2) - 1.0;
    Some((r * 10_000.0).round().clamp(0.0, u32::MAX as f64) as u32)
}

/// Third-party buyers ahead of / behind `our_index` among `txs` (one slot,
/// with block indices): `(ahead, behind, sol_ahead)`; `insiders` are the
/// creator set and the bundle wallets.
pub fn rank_in_slot(
    txs: &[TxObs],
    our_index: u32,
    owner: &Pubkey,
    insiders: &HashSet<Pubkey>,
) -> (Vec<Pubkey>, Vec<Pubkey>, u64) {
    let (mut ahead, mut behind, mut sol) = (vec![], vec![], 0u64);
    for t in txs {
        let Some(ix) = t.index else { continue };
        for tr in &t.trades {
            if !tr.is_buy || tr.user == *owner || insiders.contains(&tr.user) {
                continue;
            }
            if ix < our_index {
                sol = sol.saturating_add(tr.sol);
                if !ahead.contains(&tr.user) {
                    ahead.push(tr.user);
                }
            } else if ix > our_index && !behind.contains(&tr.user) {
                behind.push(tr.user);
            }
        }
    }
    (ahead, behind, sol)
}

// ------------------------------------------------------------------ RPC

async fn budgeted(budget: &RpcBudget) {
    loop {
        if budget.try_acquire("all") {
            return;
        }
        tokio::time::sleep(budget.wait_time("all").max(Duration::from_millis(5))).await;
    }
}

/// One JSON-RPC call inside the budget (transient errors retried twice).
pub async fn call(
    rpc: &mut rugsnipe_feed::ws::RpcClient,
    budget: &RpcBudget,
    calls: &mut u32,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let body = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
        .to_string();
    let mut attempt = 0u32;
    loop {
        budgeted(budget).await;
        *calls += 1;
        let r = async {
            let text = rpc.call(&body).await.map_err(|e| e.to_string())?;
            let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            if let Some(e) = v.get("error") {
                return Err(e.to_string());
            }
            Ok(v.get("result").cloned().unwrap_or(Value::Null))
        }
        .await;
        match r {
            Err(e) if attempt < 2 && crate::e1::transient(&e) => {
                tokio::time::sleep(Duration::from_millis(100 << attempt)).await;
                attempt += 1;
            }
            r => return r,
        }
    }
}

fn b58(sig: &[u8]) -> String {
    bs58::encode(sig).into_string()
}

fn get_tx(sig: &str) -> Value {
    serde_json::json!([sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}])
}

/// Rank a landed transaction of `p` (our `sig` in slot `land_slot`) in its
/// slot: the creation slot reconstructed without our transaction (gap-3
/// bundle, post-bundle reference), our block index, the third-party
/// buyers ahead of / behind us and the premium of `trade` over the
/// post-bundle price. Errors leave the rank partly unknown (`error`).
pub async fn analyze_landing(
    rpc: &mut rugsnipe_feed::ws::RpcClient,
    budget: &RpcBudget,
    calls: &mut u32,
    p: &Pending,
    sig: &[u8; 64],
    land_slot: u64,
    trade: Option<OwnTrade>,
    owner: &Pubkey,
    snipers: &Mutex<HashSet<Pubkey>>,
) -> Rank {
    let mut rank = Rank {
        info: ShadowInfo {
            create_slot: p.slot,
            ..Default::default()
        },
        ..Default::default()
    };
    if let Err(e) = analyze_inner(
        rpc, budget, calls, p, sig, land_slot, trade, owner, snipers, &mut rank,
    )
    .await
    {
        rank.error = Some(e);
    }
    rank
}

#[allow(clippy::too_many_arguments)]
async fn analyze_inner(
    rpc: &mut rugsnipe_feed::ws::RpcClient,
    budget: &RpcBudget,
    calls: &mut u32,
    p: &Pending,
    sig: &[u8; 64],
    land_slot: u64,
    trade: Option<OwnTrade>,
    owner: &Pubkey,
    snipers: &Mutex<HashSet<Pubkey>>,
    rank: &mut Rank,
) -> Result<(), String> {
    let ours = b58(sig);
    let create = b58(&p.create_sig);
    let s0 = p.slot;
    // 1. the curve's signatures back to the creation slot
    let mut sigs: Vec<(String, u64, bool)> = vec![];
    let mut before: Option<String> = None;
    for _ in 0..3 {
        let mut opts = serde_json::json!({"limit": 1000, "commitment": "confirmed"});
        if let Some(b) = &before {
            opts["before"] = Value::String(b.clone());
        }
        let page = parse_signatures(
            &call(
                rpc,
                budget,
                calls,
                "getSignaturesForAddress",
                serde_json::json!([p.bc.to_string(), opts]),
            )
            .await?,
        );
        let n = page.len();
        let oldest = page.last().map(|x| x.1).unwrap_or(0);
        before = page.last().map(|x| x.0.clone());
        sigs.extend(page);
        if n < 1000 || oldest <= s0 {
            break;
        }
    }
    // 2. block order of the creation slot and of ours
    let mut index_of: HashMap<u64, HashMap<String, u32>> = HashMap::new();
    let mut block_len: HashMap<u64, u32> = HashMap::new();
    for slot in [s0, land_slot] {
        if index_of.contains_key(&slot) {
            continue;
        }
        let b = call(
            rpc,
            budget,
            calls,
            "getBlock",
            serde_json::json!([slot, {"transactionDetails": "signatures", "rewards": false,
                "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}]),
        )
        .await?;
        let order = block_signatures(&b);
        block_len.insert(slot, order.len() as u32);
        index_of.insert(
            slot,
            order
                .into_iter()
                .enumerate()
                .map(|(i, s)| (s, i as u32))
                .collect(),
        );
    }
    let our_index = index_of.get(&land_slot).and_then(|m| m.get(&ours)).copied();
    rank.tx_index = our_index;
    rank.info.our_index = our_index;
    // 3. the successful curve transactions of both slots (not ours)
    let mut by_slot: HashMap<u64, Vec<TxObs>> = HashMap::new();
    let mut want: Vec<(&String, u64)> = sigs
        .iter()
        .filter(|(s, sl, ok)| *ok && (*sl == s0 || *sl == land_slot) && *s != ours)
        .map(|(s, sl, _)| (s, *sl))
        .collect();
    want.sort_by_key(|(s, sl)| (*sl, **s != create));
    if want.len() > MAX_TX {
        return Err(format!("more than {MAX_TX} curve transactions to rank"));
    }
    for (s, slot) in want {
        let r = call(rpc, budget, calls, "getTransaction", get_tx(s)).await?;
        if let Some(mut t) = tx_obs(&r, &p.mint, s, slot) {
            t.index = index_of.get(&slot).and_then(|m| m.get(s.as_str())).copied();
            by_slot.entry(slot).or_default().push(t);
        }
    }
    let empty = vec![];
    let s0_txs = by_slot.get(&s0).unwrap_or(&empty);
    let create_tx = s0_txs.iter().find(|t| t.sig == create);
    let mut cset: HashSet<Pubkey> = create_tx
        .map(|t| t.signers.iter().copied().collect())
        .unwrap_or_default();
    cset.insert(p.creator);
    cset.insert(p.user);
    if let Some(fp) = create_tx.and_then(|t| t.fee_payer) {
        cset.insert(fp);
    }
    cset.remove(&p.mint);
    cset.remove(owner);
    let known: HashSet<Pubkey> = snipers.lock().map(|s| s.clone()).unwrap_or_default();
    let slot0 = analyze_slot0(
        &create,
        s0_txs,
        block_len.get(&s0).copied().unwrap_or(0),
        &cset,
        &known,
    );
    let mut insiders = cset.clone();
    if let Some(s) = &slot0 {
        insiders.extend(s.bundle_wallets.iter().copied());
        rank.info.create_index = Some(s.create_index);
        rank.info.block_txs = Some(s.block_txs);
        rank.info.bundle_trades = s.bundle_trades;
        rank.info.bundle_wallets = s.bundle_wallets.len() as u32;
        rank.info.post_bundle_v = s.ref_v;
        rank.info.post_bundle_t = s.ref_t;
        rank.info.post_bundle_mcap = s.mcap();
        if let Ok(mut k) = snipers.lock() {
            k.extend(s.third_party.iter().copied());
        }
    }
    rank.info.s0_failed = sigs.iter().filter(|(_, sl, ok)| *sl == s0 && !ok).count() as u32;
    // 4. third-party buyers ahead / behind
    let land_txs = by_slot.get(&land_slot).unwrap_or(&empty);
    let (mut ahead, behind, mut sol) = match our_index {
        Some(ix) => rank_in_slot(land_txs, ix, owner, &insiders),
        None => (vec![], vec![], 0),
    };
    if land_slot > s0 {
        // every third-party buyer of the creation slot was ahead of us too
        let (a, b, s) = rank_in_slot(s0_txs, u32::MAX, owner, &insiders);
        debug_assert!(b.is_empty());
        for k in a {
            if !ahead.contains(&k) {
                ahead.push(k);
            }
        }
        sol = sol.saturating_add(s);
    }
    rank.tp_ahead = ahead.len() as u32;
    rank.tp_behind = behind.len() as u32;
    rank.rival_sol_ahead = sol;
    rank.info.tp_ahead = rank.tp_ahead;
    rank.info.tp_behind = rank.tp_behind;
    // 5. premium over the post-bundle price (our pre-trade state vs the reference)
    if let (Some(s), Some(t)) = (&slot0, trade.filter(|t| t.is_buy)) {
        rank.premium_bps = premium_bps(s.ref_t, t.t.saturating_add(t.tokens));
        rank.info.premium_bps = rank.premium_bps.unwrap_or(0);
    }
    if slot0.is_none() {
        return Err("creation slot not reconstructed (create transaction missing)".into());
    }
    Ok(())
}

/// Token amount of an SPL / Token-2022 account (base layout: amount at 64).
/// Slots after a position opened during which a *missing* token account is
/// "not visible yet", not "sold": the balance poll reads at `confirmed` and
/// the position opens on our own TradeEvent at `processed`, so a node a
/// block or two behind returns no account at all for a fill it has not
/// reached (2026-10-22: a 0.25 SOL position was closed as BalanceZero in
/// its opening slot, its tokens left in the wallet unsold).
pub const MISSING_ACCOUNT_GRACE_SLOTS: u64 = 32;
/// The same grace once a sell was sent (the position is exiting): the final
/// sell closes the token account, so a missing account is then the normal
/// end of the position and each re-sent sell would fail on chain for a fee.
pub const MISSING_ACCOUNT_GRACE_SLOTS_EXITING: u64 = 8;

/// Whether a balance report may size or close a position opened at
/// `opened_slot`. A non-zero balance is always applied. A zero balance is
/// trusted only from a node view past the opening slot, and, when the
/// account is missing rather than empty, only once
/// [`MISSING_ACCOUNT_GRACE_SLOTS`] have passed since the open.
pub fn balance_is_trustworthy(
    exists: bool,
    tokens: u64,
    report_slot: u64,
    opened_slot: u64,
    exiting: bool,
) -> bool {
    if tokens > 0 {
        return true;
    }
    if report_slot <= opened_slot {
        return false;
    }
    let grace = if exiting {
        MISSING_ACCOUNT_GRACE_SLOTS_EXITING
    } else {
        MISSING_ACCOUNT_GRACE_SLOTS
    };
    exists || report_slot >= opened_slot.saturating_add(grace)
}

pub fn token_amount(data: &[u8]) -> Option<u64> {
    data.get(64..72)
        .and_then(|b| b.try_into().ok())
        .map(u64::from_le_bytes)
}

// ------------------------------------------------------------------ the task

struct Attempt {
    p: Pending,
    vars: Vec<SentVar>,
    burn_started: bool,
    resolving: bool,
}

struct SellWatch {
    mint: Pubkey,
    sent_slot: u64,
}

/// Run the lane until `cmds` closes.
pub async fn run(
    cfg: LaneCfg,
    mut cmds: UnboundedReceiver<LaneCmd>,
    out: UnboundedSender<LaneEvent>,
    snipers: Arc<Mutex<HashSet<Pubkey>>>,
) {
    let mut rpc = match rugsnipe_feed::ws::RpcClient::new(&cfg.rpc_url) {
        Ok(r) => r,
        Err(e) => {
            let _ = out.send(LaneEvent::Error(format!("real lane rpc: {e}")));
            return;
        }
    };
    let mut attempts: HashMap<u64, Attempt> = HashMap::new();
    let mut sells: HashMap<[u8; 64], SellWatch> = HashMap::new();
    let mut held: HashMap<Pubkey, u64> = HashMap::new();
    let mut nonces: HashMap<u16, (Pubkey, [u8; 32])> = HashMap::new();
    let mut slot_now = 0u64;
    let mut iv = tokio::time::interval(Duration::from_millis(POLL_MS));
    let mut last_slow = 0u64;
    let mut calls = 0u32;
    loop {
        tokio::select! {
            c = cmds.recv() => {
                let Some(c) = c else { return };
                match c {
                    LaneCmd::Track(p) => {
                        nonces.insert(p.nonce_idx, (p.nonce_account, p.nonce_value));
                        attempts.insert(p.intent, Attempt { p, vars: vec![], burn_started: false, resolving: false });
                    }
                    LaneCmd::Sent { intent, var } => {
                        if let Some(a) = attempts.get_mut(&intent) {
                            if !a.vars.iter().any(|v| v.sig == var.sig) {
                                a.vars.push(var);
                            }
                        }
                    }
                    LaneCmd::SellSent { mint, sig, slot } => {
                        sells.insert(sig, SellWatch { mint, sent_slot: slot.max(slot_now) });
                    }
                    LaneCmd::Held { intent, mint } => { held.insert(mint, intent); }
                    LaneCmd::Released { mint } => { held.remove(&mint); }
                    LaneCmd::WatchNonce { idx, account, value } => { nonces.insert(idx, (account, value)); }
                    LaneCmd::Slot(s) => slot_now = slot_now.max(s),
                    LaneCmd::Burn { idx, account, value, signer } => {
                        for a in attempts.values_mut() {
                            if a.p.nonce_idx == idx {
                                a.burn_started = true;
                            }
                        }
                        nonces.insert(idx, (account, value));
                        let ev = burn(&mut rpc, &cfg, &mut calls, idx, &account, &value, &signer).await;
                        let _ = out.send(ev);
                    }
                }
            }
            _ = iv.tick() => {
                poll(&mut rpc, &cfg, &mut calls, &mut attempts, &mut sells, &mut nonces, slot_now, &out, &snipers).await;
                let now = crate::now_ms();
                if now.saturating_sub(last_slow) >= SLOW_MS {
                    last_slow = now;
                    slow(&mut rpc, &cfg, &mut calls, &held, &out).await;
                }
                if calls > 0 {
                    let _ = out.send(LaneEvent::Calls(calls));
                    calls = 0;
                }
            }
        }
    }
}

async fn burn(
    rpc: &mut rugsnipe_feed::ws::RpcClient,
    cfg: &LaneCfg,
    calls: &mut u32,
    idx: u16,
    account: &Pubkey,
    value: &[u8; 32],
    signer: &rugsnipe_tx::signer::HotSigner,
) -> LaneEvent {
    let (tx, sig) = match rugsnipe_tx::sysix::burn_tx(signer, account, value) {
        Ok(x) => x,
        Err(e) => {
            return LaneEvent::BurnSent {
                idx,
                sig: [0; 64],
                ok: false,
                err: format!("burn tx: {e}"),
            }
        }
    };
    if cfg.dry {
        return LaneEvent::BurnSent {
            idx,
            sig,
            ok: false,
            err: "dry run: not sent".into(),
        };
    }
    let b64 = base64::engine::general_purpose::STANDARD.encode(&tx);
    let r = call(
        rpc,
        &cfg.budget,
        calls,
        "sendTransaction",
        serde_json::json!([b64, {"encoding": "base64", "skipPreflight": true}]),
    )
    .await;
    LaneEvent::BurnSent {
        idx,
        sig,
        ok: r.is_ok(),
        err: r
            .err()
            .map(|e| e.chars().take(200).collect())
            .unwrap_or_default(),
    }
}

#[allow(clippy::too_many_arguments)]
async fn poll(
    rpc: &mut rugsnipe_feed::ws::RpcClient,
    cfg: &LaneCfg,
    calls: &mut u32,
    attempts: &mut HashMap<u64, Attempt>,
    sells: &mut HashMap<[u8; 64], SellWatch>,
    nonces: &mut HashMap<u16, (Pubkey, [u8; 32])>,
    slot_now: u64,
    out: &UnboundedSender<LaneEvent>,
    snipers: &Mutex<HashSet<Pubkey>>,
) {
    // 1. statuses of every buy and sell signature
    let mut list: Vec<([u8; 64], Option<u64>)> = vec![];
    for (intent, a) in attempts.iter() {
        if !a.resolving {
            list.extend(a.vars.iter().map(|v| (v.sig, Some(*intent))));
        }
    }
    list.extend(sells.keys().map(|s| (*s, None)));
    for chunk in list.chunks(256) {
        let keys: Vec<String> = chunk.iter().map(|(s, _)| b58(s)).collect();
        let r = call(
            rpc,
            &cfg.budget,
            calls,
            "getSignatureStatuses",
            serde_json::json!([keys, {"searchTransactionHistory": false}]),
        )
        .await;
        let st = match r {
            Ok(v) => parse_statuses(&v, chunk.len()),
            Err(e) => {
                let _ = out.send(LaneEvent::Error(format!("getSignatureStatuses: {e}")));
                return;
            }
        };
        for ((sig, intent), s) in chunk.iter().zip(st) {
            let Some((slot, err)) = s else { continue };
            match intent {
                Some(intent) => {
                    let Some(a) = attempts.get_mut(intent) else {
                        continue;
                    };
                    if a.resolving {
                        continue;
                    }
                    a.resolving = true;
                    let (p, var, late) = (
                        a.p,
                        a.vars.iter().find(|v| v.sig == *sig).copied(),
                        a.burn_started,
                    );
                    let l =
                        resolve_buy(rpc, cfg, calls, &p, sig, var, slot, err, late, snipers).await;
                    attempts.remove(intent);
                    let _ = out.send(LaneEvent::Landed(l));
                }
                None => {
                    let Some(w) = sells.remove(sig) else { continue };
                    let r =
                        call(rpc, &cfg.budget, calls, "getTransaction", get_tx(&b58(sig))).await;
                    match (r, err) {
                        (Ok(tx), None) => match own_trade(&tx, &w.mint, &cfg.owner, slot) {
                            Some(t) => {
                                let _ = out.send(LaneEvent::OwnTrade {
                                    sig: *sig,
                                    slot,
                                    mint: w.mint,
                                    trade: t,
                                });
                            }
                            None => {
                                let _ = out.send(LaneEvent::Error(format!(
                                    "sell {} landed without our TradeEvent",
                                    b58(sig)
                                )));
                            }
                        },
                        (Ok(tx), Some(e)) => {
                            let (reason, _) = fail_reason(&e);
                            let _ = out.send(LaneEvent::SellFailed {
                                sig: *sig,
                                fee: failed_tx_fee(&tx).unwrap_or(0),
                                reason,
                            });
                        }
                        (Err(e), _) => {
                            // retry on the next poll
                            sells.insert(*sig, w);
                            let _ = out.send(LaneEvent::Error(format!("sell getTransaction: {e}")));
                        }
                    }
                }
            }
        }
    }
    sells
        .retain(|_, w| slot_now == 0 || slot_now.saturating_sub(w.sent_slot) <= SELL_GIVE_UP_SLOTS);
    // 2. watched nonces: a new value ends the attempt on it
    if nonces.is_empty() {
        return;
    }
    let idxs: Vec<u16> = nonces.keys().copied().collect();
    let keys: Vec<String> = idxs.iter().map(|i| nonces[i].0.to_string()).collect();
    let r = call(
        rpc,
        &cfg.budget,
        calls,
        "getMultipleAccounts",
        serde_json::json!([keys, {"encoding": "base64", "commitment": "confirmed"}]),
    )
    .await;
    let (datas, ctx_slot) = match r {
        Ok(v) => (
            rugsnipe_state::chain::parse_multiple_accounts(&v, idxs.len()),
            v["context"]["slot"].as_u64().unwrap_or(slot_now),
        ),
        Err(e) => {
            let _ = out.send(LaneEvent::Error(format!("nonce accounts: {e}")));
            return;
        }
    };
    let Ok(datas) = datas else { return };
    for (idx, data) in idxs.into_iter().zip(datas) {
        let Some((auth, value, _)) = data
            .as_deref()
            .and_then(rugsnipe_tx::nonce::parse_nonce_account)
        else {
            continue;
        };
        let (_, old) = nonces[&idx];
        if auth != cfg.owner || value == old {
            continue;
        }
        // the nonce moved: an attempt on it either landed (status) or dropped
        let on: Vec<u64> = attempts
            .iter()
            .filter(|(_, a)| a.p.nonce_idx == idx && !a.resolving)
            .map(|(k, _)| *k)
            .collect();
        for intent in on {
            let Some(a) = attempts.get_mut(&intent) else {
                continue;
            };
            a.resolving = true;
            let keys: Vec<String> = a.vars.iter().map(|v| b58(&v.sig)).collect();
            let st = if keys.is_empty() {
                Ok(vec![])
            } else {
                call(
                    rpc,
                    &cfg.budget,
                    calls,
                    "getSignatureStatuses",
                    serde_json::json!([keys, {"searchTransactionHistory": true}]),
                )
                .await
                .map(|v| parse_statuses(&v, a.vars.len()))
            };
            let Ok(st) = st else {
                a.resolving = false;
                continue; // ask again next poll
            };
            let landed = a
                .vars
                .iter()
                .zip(&st)
                .find_map(|(v, s)| s.clone().map(|(slot, err)| (*v, slot, err)));
            let (p, burned) = (a.p, a.burn_started);
            let primary = a
                .vars
                .iter()
                .find(|v| v.variant == 0)
                .or(a.vars.first())
                .copied();
            attempts.remove(&intent);
            let ev = match landed {
                Some((v, slot, err)) => LaneEvent::Landed(
                    resolve_buy(
                        rpc,
                        cfg,
                        calls,
                        &p,
                        &v.sig,
                        Some(v),
                        slot,
                        err,
                        burned,
                        snipers,
                    )
                    .await,
                ),
                None => LaneEvent::Dropped {
                    intent,
                    primary,
                    burned,
                },
            };
            let _ = out.send(ev);
        }
        nonces.remove(&idx);
        let _ = out.send(LaneEvent::Nonce {
            idx,
            value,
            slot: ctx_slot,
        });
    }
}

#[allow(clippy::too_many_arguments)]
async fn resolve_buy(
    rpc: &mut rugsnipe_feed::ws::RpcClient,
    cfg: &LaneCfg,
    calls: &mut u32,
    p: &Pending,
    sig: &[u8; 64],
    var: Option<SentVar>,
    slot: u64,
    err: Option<Value>,
    late: bool,
    snipers: &Mutex<HashSet<Pubkey>>,
) -> Landed {
    let mut l = Landed {
        intent: p.intent,
        sig: *sig,
        var,
        slot,
        fee: 0,
        fail: err.as_ref().map(fail_reason),
        trade: None,
        rank: Rank::default(),
        late,
    };
    match call(rpc, &cfg.budget, calls, "getTransaction", get_tx(&b58(sig))).await {
        Ok(tx) => {
            l.fee = tx["meta"]["fee"].as_u64().unwrap_or(0);
            if let Some(f) = failed_tx_fee(&tx) {
                l.fee = f;
                if l.fail.is_none() {
                    l.fail = tx["meta"].get("err").map(fail_reason);
                }
            }
            if l.fail.is_none() {
                l.trade = own_trade(&tx, &p.mint, &cfg.owner, slot);
            }
        }
        Err(e) => l.rank.error = Some(format!("getTransaction: {e}")),
    }
    let rank = analyze_landing(
        rpc,
        &cfg.budget,
        calls,
        p,
        sig,
        slot,
        l.trade,
        &cfg.owner,
        snipers,
    )
    .await;
    if l.rank.error.is_some() && rank.error.is_none() {
        l.rank = Rank {
            error: l.rank.error.take(),
            ..rank
        };
    } else {
        l.rank = rank;
    }
    l
}

async fn slow(
    rpc: &mut rugsnipe_feed::ws::RpcClient,
    cfg: &LaneCfg,
    calls: &mut u32,
    held: &HashMap<Pubkey, u64>,
    out: &UnboundedSender<LaneEvent>,
) {
    match call(
        rpc,
        &cfg.budget,
        calls,
        "getBalance",
        serde_json::json!([cfg.owner.to_string(), {"commitment": "confirmed"}]),
    )
    .await
    {
        Ok(v) => {
            if let Some(l) = v["value"].as_u64() {
                let _ = out.send(LaneEvent::Wallet { lamports: l });
            }
        }
        Err(e) => {
            let _ = out.send(LaneEvent::Error(format!("getBalance: {e}")));
        }
    }
    match call(
        rpc,
        &cfg.budget,
        calls,
        "getLatestBlockhash",
        serde_json::json!([{"commitment": "confirmed"}]),
    )
    .await
    {
        Ok(v) => {
            let hash = v["value"]["blockhash"]
                .as_str()
                .and_then(|s| bs58::decode(s).into_vec().ok())
                .and_then(|b| <[u8; 32]>::try_from(b).ok());
            if let Some(hash) = hash {
                let _ = out.send(LaneEvent::Blockhash {
                    hash,
                    slot: v["context"]["slot"].as_u64().unwrap_or(0),
                });
            }
        }
        Err(e) => {
            let _ = out.send(LaneEvent::Error(format!("getLatestBlockhash: {e}")));
        }
    }
    if held.is_empty() {
        return;
    }
    let mints: Vec<Pubkey> = held.keys().copied().collect();
    let atas: Vec<String> = mints
        .iter()
        .map(|m| pda::ata_token_2022(&cfg.owner, m).to_string())
        .collect();
    let r = call(
        rpc,
        &cfg.budget,
        calls,
        "getMultipleAccounts",
        serde_json::json!([atas, {"encoding": "base64", "commitment": "confirmed"}]),
    )
    .await;
    match r {
        Ok(v) => {
            let slot = v["context"]["slot"].as_u64().unwrap_or(0);
            if let Ok(datas) = rugsnipe_state::chain::parse_multiple_accounts(&v, mints.len()) {
                for (mint, d) in mints.into_iter().zip(datas) {
                    // a missing account reads as zero tokens; the hot loop
                    // decides whether to trust it (balance_is_trustworthy)
                    let exists = d.is_some();
                    let tokens = d.as_deref().and_then(token_amount).unwrap_or(0);
                    let _ = out.send(LaneEvent::Balance {
                        mint,
                        tokens,
                        slot,
                        exists,
                    });
                }
            }
        }
        Err(e) => {
            let _ = out.send(LaneEvent::Error(format!("held balances: {e}")));
        }
    }
}

/// The exit engine's `Trade` of an own trade.
pub fn trade_of(t: &OwnTrade, mint: Pubkey, owner: Pubkey, slot: u64) -> rugsnipe_exit::Trade {
    rugsnipe_exit::Trade {
        venue: rugsnipe_exit::Venue::Curve,
        mint,
        user: owner,
        is_buy: t.is_buy,
        sol_amount: t.sol,
        token_amount: t.tokens,
        fee_lamports: t.fee,
        reserve_sol: t.v,
        reserve_token: t.t,
        slot,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::e2::TradeObs;
    use serde_json::json;

    #[test]
    fn a_zero_balance_closes_only_from_a_view_past_the_fill() {
        let g = MISSING_ACCOUNT_GRACE_SLOTS;
        // the live case: the first poll after the open, from a node still at
        // the opening slot, found no account
        assert!(!balance_is_trustworthy(false, 0, 100, 100, false));
        assert!(!balance_is_trustworthy(false, 0, 99, 100, false));
        // a missing account stays "not visible yet" through the grace window
        assert!(!balance_is_trustworthy(false, 0, 100 + g - 1, 100, false));
        assert!(balance_is_trustworthy(false, 0, 100 + g, 100, false));
        // exiting: the final sell closes the account, a shorter grace
        let e = MISSING_ACCOUNT_GRACE_SLOTS_EXITING;
        assert!(!balance_is_trustworthy(false, 0, 100 + e - 1, 100, true));
        assert!(balance_is_trustworthy(false, 0, 100 + e, 100, true));
        assert!(!balance_is_trustworthy(false, 0, 100, 100, true));
        // an existing, empty account after the open is a sold position
        assert!(balance_is_trustworthy(true, 0, 101, 100, false));
        assert!(!balance_is_trustworthy(true, 0, 100, 100, false));
        // tokens are always applied (sizing, orphan adoption)
        assert!(balance_is_trustworthy(false, 5, 1, 100, false));
        assert!(balance_is_trustworthy(true, 5, 100, 100, true));
    }

    #[test]
    fn statuses_errors_and_premium() {
        let r = json!({"context": {"slot": 10}, "value": [
            null,
            {"slot": 9, "confirmations": 0, "err": null, "confirmationStatus": "confirmed"},
            {"slot": 9, "confirmations": 0, "err": {"InstructionError": [3, {"Custom": 6042}]}, "confirmationStatus": "finalized"},
            {"slot": 9, "confirmations": 0, "err": null, "confirmationStatus": "processed"}
        ]});
        let s = parse_statuses(&r, 5);
        assert_eq!(s[0], None);
        assert_eq!(s[1], Some((9, None)));
        assert!(matches!(&s[2], Some((9, Some(_)))));
        assert_eq!(s[3], None, "processed is not landed yet");
        assert_eq!(s[4], None);
        let e = json!({"InstructionError": [3, {"Custom": 6002}]});
        assert_eq!(fail_reason(&e).0, "slippage");
        assert_eq!(
            fail_reason(&json!({"InstructionError": [3, {"Custom": 6004}]})).0,
            "custom_6004"
        );
        assert_eq!(
            fail_reason(&json!("InsufficientFundsForFee")).0,
            "insufficient_funds_for_fee"
        );
        assert_eq!(
            fail_reason(&json!({"InsufficientFundsForRent": {"account_index": 0}})).0,
            "insufficient_funds_for_rent"
        );
        // price ∝ 1/T^2: 1% fewer tokens in the curve = +2.01%
        assert_eq!(premium_bps(1_000_000, 990_000), Some(203));
        assert_eq!(premium_bps(1_000_000, 1_010_000), Some(0), "never negative");
        assert_eq!(premium_bps(0, 1), None);
        let mut acct = vec![0u8; 165];
        acct[64..72].copy_from_slice(&777u64.to_le_bytes());
        assert_eq!(token_amount(&acct), Some(777));
        assert_eq!(token_amount(&[0u8; 10]), None);
    }

    #[test]
    fn ranks_third_party_buyers_around_us() {
        let owner = Pubkey::new_unique();
        let (dev, bw, a, b) = (
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        );
        let tr = |user, sol| TradeObs {
            slot: 5,
            is_buy: true,
            sol,
            tok: 1,
            v: 1,
            t: 1,
            user,
        };
        let tx = |ix, user, sol| TxObs {
            sig: format!("s{ix}"),
            slot: 5,
            index: Some(ix),
            fee_payer: None,
            signers: vec![],
            trades: vec![tr(user, sol)],
        };
        let txs = vec![
            tx(1, dev, 5),
            tx(2, bw, 7),
            tx(3, a, 11),
            tx(4, a, 13),
            tx(6, owner, 1),
            tx(7, b, 17),
        ];
        let insiders: HashSet<Pubkey> = [dev, bw].into_iter().collect();
        let (ahead, behind, sol) = rank_in_slot(&txs, 5, &owner, &insiders);
        assert_eq!(ahead, vec![a], "distinct wallets");
        assert_eq!(behind, vec![b]);
        assert_eq!(sol, 24);
    }
}
