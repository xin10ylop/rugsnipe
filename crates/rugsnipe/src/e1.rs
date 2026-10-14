//! Paper fill model **E1** (docs/04 sections 2 and 7): we land in the
//! creation slot right behind the creator's bundle and pay a premium over
//! the post-bundle price, or the attempt fails and pays its priority fee.
//!
//! The free feed sees a create about a second late, so the cold lane
//! reconstructs the creation slot and the next `replay_slots` slots from
//! confirmed history inside the RPC budget:
//!
//! 1. `getSignaturesForAddress(bonding curve)`: every transaction on the
//!    curve with its slot (failed ones count as lost races of others);
//! 2. no other successful slot-0 transaction: the bundle is the create's own
//!    in-tx buys (all by the creator set), so the launch is not
//!    multi-bundle (`class` under `entry.filter = "multi_bundle"`) and the
//!    lane stops after one call;
//! 3. `getBlock(slot, transactionDetails = "signatures")`: the transaction
//!    index of every slot-0 signature (the create's position in its block);
//! 4. `getTransaction` for the slot-0 transactions on the curve: TradeEvents
//!    (user, SOL, tokens, post-trade reserves), signers and fee payer;
//! 5. the bundle, exactly as the gap-3 tapes define it
//!    (`analysis/rugsnipe_analysis/strategy/tapes.py` `_raw_bundle`): the
//!    create transaction's buys, every slot-0 buy by the creator set
//!    (creator, create signer, fee payer, the create's signers), and the
//!    contiguous run of slot-0 buys at index <= create + 5 with index gaps
//!    <= 2. The post-bundle state is the curve after the last of them;
//! 6. a third-party buy interleaved in the run (a wallet this session has
//!    already seen buying another launch's slot 0 outside its bundle) is
//!    ahead of us: it leaves the bundle, and its price impact over the
//!    bundle-only state is the measured premium. The fill pays
//!    `max(entry_premium, measured)`; above `entry.max_premium` the attempt
//!    fails (`premium`);
//! 7. a create in the last `late_block_frac` of its block cannot be followed
//!    in slot 0 (gap-2): the attempt fails (`not_in_slot0`);
//! 8. for a fill, `getTransaction` for the transactions of slots
//!    `s+1 ..= s+replay_slots` (ordered inside a slot by the reserve chain):
//!    the tape the hot loop replays through the exit engine on a slot clock.
//!
//! The hot loop applies the operator's on-chain bound (`slippage`) and
//! everything after: the paper position, the exits, the sell fills one slot
//! after the trigger slot, all on the counterfactual curve that keeps our
//! tokens outstanding (the tape simulator's model).

use crate::e2::{parse_signatures, trades_from_tx, TradeObs};
use crate::now_ms;
use pump_codec::Pubkey;
use rugsnipe_core::config::EntryFilter;
use rugsnipe_core::rslog::ShadowOutcome;
use rugsnipe_state::RpcBudget;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 80 SOL of bundle buys or 79% of the supply bought in slot 0: the launch
/// graduates in its creation slot (gap-3 drops these tapes).
pub const SLOT0_GRADUATION_LAMPORTS: u64 = 80_000_000_000;
pub const SLOT0_GRADUATION_TOKENS: u64 = 790_000_000_000_000;
/// A dev-only bundle needs at least 1 SOL to be a gap-3 tape.
pub const DEV_ONLY_MIN_LAMPORTS: u64 = 1_000_000_000;
/// Token supply in raw units (1e9 tokens, 6 decimals): market cap = price x this.
pub const SUPPLY_RAW: u128 = 1_000_000_000_000_000;

/// One transaction on the curve from confirmed history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxObs {
    pub sig: String,
    pub slot: u64,
    /// Index in its block (slot-0 transactions only).
    pub index: Option<u32>,
    pub fee_payer: Option<Pubkey>,
    pub signers: Vec<Pubkey>,
    /// TradeEvents of the mint, in log order.
    pub trades: Vec<TradeObs>,
}

/// Signers and fee payer of a `getTransaction` result (`json` or
/// `jsonParsed` encoding).
pub fn keys_from_tx(result: &Value) -> (Option<Pubkey>, Vec<Pubkey>) {
    let msg = &result["transaction"]["message"];
    let Some(keys) = msg["accountKeys"].as_array() else {
        return (None, vec![]);
    };
    let n_sig = msg["header"]["numRequiredSignatures"].as_u64();
    let mut signers = vec![];
    for (i, k) in keys.iter().enumerate() {
        let (pk, is_signer) = match k {
            Value::String(s) => (
                s.parse::<Pubkey>().ok(),
                n_sig.is_some_and(|n| (i as u64) < n),
            ),
            Value::Object(o) => (
                o.get("pubkey")
                    .and_then(|p| p.as_str())
                    .and_then(|s| s.parse().ok()),
                o.get("signer").and_then(|s| s.as_bool()).unwrap_or(false),
            ),
            _ => (None, false),
        };
        if let (Some(pk), true) = (pk, is_signer) {
            signers.push(pk);
        }
    }
    (signers.first().copied(), signers)
}

/// A `TxObs` from a `getTransaction` result (`None` for a failed or missing
/// transaction).
pub fn tx_obs(result: &Value, mint: &Pubkey, sig: &str, slot: u64) -> Option<TxObs> {
    let meta = result.get("meta")?;
    if meta.get("err").is_some_and(|e| !e.is_null()) {
        return None;
    }
    let (fee_payer, signers) = keys_from_tx(result);
    Some(TxObs {
        sig: sig.to_string(),
        slot,
        index: None,
        fee_payer,
        signers,
        trades: trades_from_tx(result, mint, slot),
    })
}

/// Block signatures of a `getBlock(transactionDetails = "signatures")` result.
pub fn block_signatures(result: &Value) -> Vec<String> {
    result["signatures"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Pre-trade reserves of a trade.
fn pre(t: &TradeObs) -> (u64, u64) {
    if t.is_buy {
        (t.v.saturating_sub(t.sol), t.t.saturating_add(t.tok))
    } else {
        (t.v.saturating_add(t.sol), t.t.saturating_sub(t.tok))
    }
}

/// Order the trades of one slot by the reserve chain (each trade's
/// pre-state is the previous trade's post-state). `trades` come grouped by
/// transaction in log order; unchained trades keep their given order at the
/// end.
pub fn order_slot(trades: &[TradeObs]) -> Vec<TradeObs> {
    let n = trades.len();
    if n <= 1 {
        return trades.to_vec();
    }
    let post: Vec<(u64, u64)> = trades.iter().map(|t| (t.v, t.t)).collect();
    let mut used = vec![false; n];
    let mut out = Vec::with_capacity(n);
    // chain heads: a pre-state that is no other trade's post-state
    let heads: Vec<usize> = (0..n)
        .filter(|&i| {
            let p = pre(&trades[i]);
            !(0..n).any(|j| j != i && post[j] == p)
        })
        .collect();
    for h in heads {
        if used[h] {
            continue;
        }
        let mut cur = h;
        loop {
            used[cur] = true;
            out.push(trades[cur]);
            let next = (0..n).find(|&j| !used[j] && pre(&trades[j]) == post[cur]);
            match next {
                Some(j) => cur = j,
                None => break,
            }
        }
    }
    for i in 0..n {
        if !used[i] {
            out.push(trades[i]);
        }
    }
    out
}

/// The creation slot reconstructed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Slot0 {
    pub create_index: u32,
    pub block_txs: u32,
    /// Trades in the bundle (create tx buys, creator-set buys, the run).
    pub bundle_trades: u32,
    /// Bundle wallets outside the creator set.
    pub bundle_wallets: Vec<Pubkey>,
    pub creator_set: Vec<Pubkey>,
    /// Index of the last transaction we land behind; ours is the next.
    pub our_index: u32,
    /// Third-party slot-0 buyers ahead of / behind us (distinct wallets).
    pub tp_ahead: u32,
    pub tp_behind: u32,
    /// Third-party slot-0 buyers (for the session's sniper set).
    pub third_party: Vec<Pubkey>,
    /// Bundle-only post-bundle state (the premium's reference).
    pub ref_v: u64,
    pub ref_t: u64,
    /// Price impact of interleaved third-party buys over the reference (bps).
    pub measured_premium_bps: u32,
    /// SOL of the creator set's and the bundle wallets' bundle buys.
    pub dev_sol: u64,
    pub bundle_sol: u64,
    /// The launch graduates in its creation slot (no curve to trade).
    pub graduated: bool,
    /// Slot-0 trades after us, in block order.
    pub after: Vec<TradeObs>,
}

impl Slot0 {
    /// At least one bundle wallet outside the creator set (docs/04 section 2).
    pub fn multi(&self) -> bool {
        !self.bundle_wallets.is_empty()
    }
    /// A gap-3 tape at all: multi-wallet, or a dev-only bundle of >= 1 SOL.
    pub fn is_tape(&self) -> bool {
        !self.graduated && (self.multi() || self.dev_sol >= DEV_ONLY_MIN_LAMPORTS)
    }
    /// Post-bundle market cap (lamports).
    pub fn mcap(&self) -> u64 {
        if self.ref_t == 0 {
            return 0;
        }
        u64::try_from(self.ref_v as u128 * SUPPLY_RAW / self.ref_t as u128).unwrap_or(u64::MAX)
    }
}

/// Analyze the creation slot. `txs` are its successful transactions on the
/// curve with their block index; `creator_set` the creator, create signer,
/// fee payer and the create's signers; `snipers` the wallets seen buying
/// another launch's slot 0 outside its bundle. `None` if the create is
/// missing.
pub fn analyze_slot0(
    create_sig: &str,
    txs: &[TxObs],
    block_txs: u32,
    creator_set: &HashSet<Pubkey>,
    snipers: &HashSet<Pubkey>,
) -> Option<Slot0> {
    let mut txs: Vec<&TxObs> = txs.iter().filter(|t| t.index.is_some()).collect();
    txs.sort_by_key(|t| t.index);
    let create = txs.iter().find(|t| t.sig == create_sig)?;
    let ctx = create.index?;
    // flattened slot-0 trades: (index, is_create_tx, trade)
    let tr: Vec<(u32, bool, TradeObs)> = txs
        .iter()
        .flat_map(|t| {
            let ix = t.index.unwrap_or(0);
            let is_c = t.sig == create_sig;
            t.trades.iter().map(move |x| (ix, is_c, *x))
        })
        .collect();
    let mut in_bundle = vec![false; tr.len()];
    for (i, (_, is_c, t)) in tr.iter().enumerate() {
        if t.is_buy && (*is_c || creator_set.contains(&t.user)) {
            in_bundle[i] = true;
        }
    }
    // the contiguous run after the create
    let mut prev = ctx;
    let mut run_tp = vec![false; tr.len()];
    for (i, (ix, _, t)) in tr.iter().enumerate() {
        if *ix <= ctx {
            continue;
        }
        if ix - prev <= 2 && ix - ctx <= 5 && t.is_buy {
            if !creator_set.contains(&t.user) && snipers.contains(&t.user) {
                run_tp[i] = true; // a known sniper inside the run: ahead of us
            } else {
                in_bundle[i] = true;
            }
            prev = *ix;
        } else if ix - prev > 2 || ix - ctx > 5 {
            break;
        }
    }
    let pb = in_bundle.iter().rposition(|b| *b)?;
    // we land after the last bundle trade and after any sniper inside the run
    let e = run_tp.iter().rposition(|b| *b).map_or(pb, |k| k.max(pb));
    let mut bundle_wallets: Vec<Pubkey> = vec![];
    let (mut dev_sol, mut bundle_sol, mut n_bundle) = (0u64, 0u64, 0u32);
    for (i, (_, _, t)) in tr.iter().enumerate() {
        if !in_bundle[i] {
            continue;
        }
        n_bundle += 1;
        if creator_set.contains(&t.user) {
            dev_sol = dev_sol.saturating_add(t.sol);
        } else {
            bundle_sol = bundle_sol.saturating_add(t.sol);
            if !bundle_wallets.contains(&t.user) {
                bundle_wallets.push(t.user);
            }
        }
    }
    let s0_tokens: u64 = tr
        .iter()
        .filter(|(_, _, t)| t.is_buy)
        .map(|(_, _, t)| t.tok)
        .fold(0u64, |a, b| a.saturating_add(b));
    let graduated = dev_sol.saturating_add(bundle_sol) >= SLOT0_GRADUATION_LAMPORTS
        || s0_tokens >= SLOT0_GRADUATION_TOKENS;
    // third-party slot-0 buyers: not in the creator set, not bundle wallets
    let is_tp = |u: &Pubkey| !creator_set.contains(u) && !bundle_wallets.contains(u);
    let mut ahead: Vec<Pubkey> = vec![];
    let mut behind: Vec<Pubkey> = vec![];
    for (i, (_, _, t)) in tr.iter().enumerate() {
        if !t.is_buy || !is_tp(&t.user) {
            continue;
        }
        let list = if i <= e { &mut ahead } else { &mut behind };
        if !list.contains(&t.user) {
            list.push(t.user);
        }
    }
    let mut third_party = ahead.clone();
    for b in &behind {
        if !third_party.contains(b) {
            third_party.push(*b);
        }
    }
    // the state we buy at, and the bundle-only reference without the
    // interleaved snipers' tokens (constant product through the end state)
    let end = tr[e].2;
    let tp_tokens: u64 = tr
        .iter()
        .enumerate()
        .filter(|(i, _)| run_tp[*i])
        .map(|(_, (_, _, t))| t.tok)
        .fold(0u64, |a, b| a.saturating_add(b));
    let (ref_v, ref_t, prem) = if tp_tokens == 0 {
        (tr[pb].2.v, tr[pb].2.t, 0u32)
    } else {
        let t_ref = end.t.saturating_add(tp_tokens);
        let v_ref = (end.v as u128 * end.t as u128 / t_ref as u128) as u64;
        // price ratio = (t_ref / t_end)^2
        let r = (t_ref as f64 / end.t as f64).powi(2) - 1.0;
        (v_ref, t_ref, (r * 10_000.0).round().max(0.0) as u32)
    };
    let mut creator_list: Vec<Pubkey> = creator_set.iter().copied().collect();
    creator_list.sort();
    Some(Slot0 {
        create_index: ctx,
        block_txs,
        bundle_trades: n_bundle,
        bundle_wallets,
        creator_set: creator_list,
        our_index: tr[e].0 + 1,
        tp_ahead: ahead.len() as u32,
        tp_behind: behind.len() as u32,
        third_party,
        ref_v,
        ref_t,
        measured_premium_bps: prem,
        dev_sol,
        bundle_sol,
        graduated,
        after: tr[e + 1..].iter().map(|x| x.2).collect(),
    })
}

/// What the lane needs from the decision.
#[derive(Debug, Clone, Copy)]
pub struct E1Req {
    pub intent: u64,
    pub mint: Pubkey,
    pub bc: Pubkey,
    pub create_sig: [u8; 64],
    pub slot: u64,
    pub creator: Pubkey,
    pub user: Pubkey,
    pub decided_unix_ms: u64,
}

/// Lane settings (`[paper]`, `[entry]`).
#[derive(Debug, Clone)]
pub struct E1Cfg {
    pub rpc_url: String,
    /// Total requests per second of the lane (every method shares one bucket).
    pub budget: Arc<RpcBudget>,
    pub delay_ms: u64,
    pub max_tx: usize,
    pub replay_slots: u64,
    pub late_block_frac: f64,
    pub entry_premium_bps: u32,
    pub max_premium_bps: u32,
    pub filter: EntryFilter,
    /// Wait before retrying a create that is not in confirmed history yet
    /// (transient RPC errors back off from `min(retry_ms, 250)`).
    pub retry_ms: u64,
    /// A fill needs the tape complete through `s + min_replay_slots` (the
    /// exit window); a shorter tape is `unresolved`, never replayed in part.
    pub min_replay_slots: u64,
}

/// The lane's verdict on one intent.
#[derive(Debug, Clone)]
pub struct E1Res {
    pub req: E1Req,
    /// `Filled` (the hot loop may still turn it into `Slippage`), `Premium`,
    /// `NotInSlot0`, `Class` or `Unresolved`.
    pub outcome: ShadowOutcome,
    /// `None` when the slot could not be analyzed (class without a block
    /// read, or unresolved).
    pub slot0: Option<Slot0>,
    /// Premium the fill pays: `max(entry_premium, measured)` (bps).
    pub premium_bps: u32,
    pub s0_failed: u32,
    pub intx_trades: u32,
    /// Trades after us: the rest of slot 0, then slots `s+1 ..= horizon`.
    pub tape: Vec<TradeObs>,
    /// Last slot the tape covers completely.
    pub horizon: u64,
    pub calls: u32,
    pub error: Option<String>,
}

/// The verdict once the creation slot is known (pure; also used by tests).
pub fn verdict(cfg: &E1Cfg, s0: &Slot0) -> (ShadowOutcome, u32) {
    let premium = cfg.entry_premium_bps.max(s0.measured_premium_bps);
    let class = match cfg.filter {
        EntryFilter::MultiBundle => !s0.multi() || s0.graduated,
        EntryFilter::Any => s0.graduated,
    };
    let late = s0.block_txs > 0
        && (s0.create_index as f64) >= (1.0 - cfg.late_block_frac) * s0.block_txs as f64;
    let outcome = if class {
        ShadowOutcome::Class
    } else if late {
        ShadowOutcome::NotInSlot0
    } else if premium > cfg.max_premium_bps {
        ShadowOutcome::Premium
    } else {
        ShadowOutcome::Filled
    };
    (outcome, premium)
}

async fn budgeted(budget: &RpcBudget) {
    loop {
        if budget.try_acquire("all") {
            return;
        }
        tokio::time::sleep(budget.wait_time("all").max(Duration::from_millis(5))).await;
    }
}

/// A public-RPC error worth retrying: rate limit (429), data allowance
/// (413), timeouts and dropped connections.
pub fn transient(e: &str) -> bool {
    let e = e.to_ascii_lowercase();
    [
        "429",
        "413",
        "timed out",
        "timeout",
        "eof",
        "reset",
        "broken pipe",
        "connection",
    ]
    .iter()
    .any(|k| e.contains(k))
}

/// One JSON-RPC call inside the budget; transient errors are retried up to
/// four times with exponential backoff from `backoff_ms`.
async fn call(
    rpc: &mut rugsnipe_feed::ws::RpcClient,
    budget: &RpcBudget,
    calls: &mut u32,
    body: &str,
    backoff_ms: u64,
) -> Result<Value, String> {
    let mut attempt = 0u32;
    loop {
        budgeted(budget).await;
        *calls += 1;
        let r = async {
            let text = rpc.call(body).await.map_err(|e| e.to_string())?;
            let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            if let Some(e) = v.get("error") {
                return Err(e.to_string());
            }
            Ok(v.get("result").cloned().unwrap_or(Value::Null))
        }
        .await;
        match r {
            Err(e) if attempt < 4 && transient(&e) => {
                tokio::time::sleep(Duration::from_millis(backoff_ms << attempt)).await;
                attempt += 1;
            }
            r => return r,
        }
    }
}

fn sigs_body(bc: &Pubkey, before: Option<&str>) -> String {
    match before {
        Some(b) => format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getSignaturesForAddress","params":["{bc}",{{"limit":1000,"before":"{b}","commitment":"confirmed"}}]}}"#
        ),
        None => format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getSignaturesForAddress","params":["{bc}",{{"limit":1000,"commitment":"confirmed"}}]}}"#
        ),
    }
}

fn block_body(slot: u64) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"getBlock","params":[{slot},{{"transactionDetails":"signatures","rewards":false,"maxSupportedTransactionVersion":0,"commitment":"confirmed"}}]}}"#
    )
}

/// Resolve one intent (see the module docs). `snipers` is the session's
/// set of third-party slot-0 buyers; this launch's are added to it.
pub async fn resolve(
    cfg: &E1Cfg,
    req: E1Req,
    intx_trades: u32,
    snipers: Arc<Mutex<HashSet<Pubkey>>>,
) -> E1Res {
    let mut res = E1Res {
        req,
        outcome: ShadowOutcome::Unresolved,
        slot0: None,
        premium_bps: cfg.entry_premium_bps,
        s0_failed: 0,
        intx_trades,
        tape: vec![],
        horizon: req.slot,
        calls: 0,
        error: None,
    };
    let due = req.decided_unix_ms + cfg.delay_ms;
    let now = now_ms();
    if due > now {
        tokio::time::sleep(Duration::from_millis(due - now)).await;
    }
    // confirmed history can lag the decision: retry a missing create twice
    for attempt in 0..3 {
        match resolve_inner(cfg, &mut res, &snipers).await {
            Ok(()) => break,
            Err(e) if e.starts_with(NOT_YET) && attempt < 2 => {
                tokio::time::sleep(Duration::from_millis(cfg.retry_ms)).await;
            }
            Err(e) => {
                res.outcome = ShadowOutcome::Unresolved;
                res.error = Some(e);
                break;
            }
        }
    }
    res
}

const NOT_YET: &str = "create signature not in the curve's history";

async fn resolve_inner(
    cfg: &E1Cfg,
    res: &mut E1Res,
    snipers: &Arc<Mutex<HashSet<Pubkey>>>,
) -> Result<(), String> {
    let req = res.req;
    let s0 = req.slot;
    let horizon = s0 + cfg.replay_slots;
    let mut rpc = rugsnipe_feed::ws::RpcClient::new(&cfg.rpc_url).map_err(|e| e.to_string())?;
    let mut calls = res.calls;
    let backoff = cfg.retry_ms.min(250);
    // 1. every signature on the curve back to the creation slot
    let mut sigs: Vec<(String, u64, bool)> = vec![];
    let mut before: Option<String> = None;
    for _ in 0..3 {
        let page = parse_signatures(
            &call(
                &mut rpc,
                &cfg.budget,
                &mut calls,
                &sigs_body(&req.bc, before.as_deref()),
                backoff,
            )
            .await?,
        );
        let n = page.len();
        let oldest = page.last().map(|p| p.1).unwrap_or(0);
        before = page.last().map(|p| p.0.clone());
        sigs.extend(page);
        if n < 1000 || oldest <= s0 {
            break;
        }
    }
    res.calls = calls;
    let create = bs58::encode(req.create_sig).into_string();
    if !sigs.iter().any(|(s, _, _)| *s == create) {
        return Err(format!("{NOT_YET} (yet)"));
    }
    res.s0_failed = sigs.iter().filter(|(_, s, ok)| *s == s0 && !ok).count() as u32;
    let s0_ok: Vec<&String> = sigs
        .iter()
        .filter(|(_, s, ok)| *s == s0 && *ok)
        .map(|(sig, _, _)| sig)
        .collect();
    if !s0_ok.contains(&&create) {
        return Err(format!("create not in slot {s0} (feed slot mismatch)"));
    }
    // 2. nothing but the create in slot 0: every bundle buy is the creator set's
    if s0_ok.len() == 1 && cfg.filter == EntryFilter::MultiBundle {
        res.outcome = ShadowOutcome::Class;
        return Ok(());
    }
    // 3. block order
    let block = call(&mut rpc, &cfg.budget, &mut calls, &block_body(s0), backoff).await?;
    res.calls = calls;
    let order = block_signatures(&block);
    if order.is_empty() {
        return Err(format!("getBlock({s0}) returned no signatures"));
    }
    let index: HashMap<&str, u32> = order
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i as u32))
        .collect();
    // 4. the slot-0 transactions (the create first, then by index)
    let mut s0_sigs: Vec<(&String, u32)> = s0_ok
        .iter()
        .filter_map(|s| index.get(s.as_str()).map(|i| (*s, *i)))
        .collect();
    s0_sigs.sort_by_key(|(s, i)| (**s != create, *i));
    let mut txs = vec![];
    let mut fetched = 0usize;
    for (sig, ix) in &s0_sigs {
        if fetched >= cfg.max_tx {
            break;
        }
        fetched += 1;
        let r = call(
            &mut rpc,
            &cfg.budget,
            &mut calls,
            &rugsnipe_feed::ws::rpc::get_transaction_body(2, sig),
            backoff,
        )
        .await;
        if let Ok(r) = r {
            if let Some(mut t) = tx_obs(&r, &req.mint, sig, s0) {
                t.index = Some(*ix);
                txs.push(t);
            }
        }
    }
    res.calls = calls;
    let Some(create_tx) = txs.iter().find(|t| t.sig == create) else {
        return Err("create transaction not fetched".into());
    };
    let mut cset: HashSet<Pubkey> = create_tx.signers.iter().copied().collect();
    cset.insert(req.creator);
    cset.insert(req.user);
    if let Some(fp) = create_tx.fee_payer {
        cset.insert(fp);
    }
    cset.remove(&req.mint); // the mint keypair signs every create
    let known: HashSet<Pubkey> = snipers.lock().map(|s| s.clone()).unwrap_or_default();
    let slot0 = analyze_slot0(&create, &txs, order.len() as u32, &cset, &known)
        .ok_or("no bundle trade in the creation slot")?;
    if let Ok(mut s) = snipers.lock() {
        s.extend(slot0.third_party.iter().copied());
    }
    let (outcome, premium) = verdict(cfg, &slot0);
    res.outcome = outcome;
    res.premium_bps = premium;
    res.tape = slot0.after.clone();
    res.slot0 = Some(slot0);
    if outcome != ShadowOutcome::Filled {
        return Ok(());
    }
    // 8. the replay tape: slots s+1 ..= s+replay_slots, oldest first
    let mut later: Vec<(&String, u64)> = sigs
        .iter()
        .filter(|(_, s, ok)| *ok && *s > s0 && *s <= horizon)
        .map(|(sig, s, _)| (sig, *s))
        .collect();
    // getSignaturesForAddress lists newest first: reverse for the chain order
    later.reverse();
    later.sort_by_key(|(_, s)| *s);
    let mut covered = horizon;
    let mut by_slot: Vec<(u64, Vec<TradeObs>)> = vec![];
    for (sig, slot) in later {
        if fetched >= cfg.max_tx {
            // the cap cut this slot: the tape is complete up to the slot before
            covered = covered.min(slot.saturating_sub(1));
            break;
        }
        fetched += 1;
        let r = call(
            &mut rpc,
            &cfg.budget,
            &mut calls,
            &rugsnipe_feed::ws::rpc::get_transaction_body(2, sig),
            backoff,
        )
        .await;
        let trades = match r {
            Ok(r) => tx_obs(&r, &req.mint, sig, slot)
                .map(|t| t.trades)
                .unwrap_or_default(),
            Err(_) => {
                covered = covered.min(slot.saturating_sub(1));
                break;
            }
        };
        match by_slot.last_mut() {
            Some((s, v)) if *s == slot => v.extend(trades),
            _ => by_slot.push((slot, trades)),
        }
    }
    res.calls = calls;
    for (slot, trades) in by_slot {
        if slot > covered {
            break;
        }
        res.tape.extend(order_slot(&trades));
    }
    res.horizon = covered.max(s0);
    if res.horizon < s0 + cfg.min_replay_slots.min(cfg.replay_slots) {
        res.outcome = ShadowOutcome::Unresolved;
        res.error = Some(format!(
            "replay tape cut at slot s+{} (needs s+{})",
            res.horizon - s0,
            cfg.min_replay_slots
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pump_codec::curve::CurveState;

    struct B {
        c: CurveState,
        txs: Vec<TxObs>,
        slot: u64,
    }

    impl B {
        fn new() -> Self {
            Self {
                c: CurveState::fresh(Pubkey::default()),
                txs: vec![],
                slot: 500,
            }
        }
        /// A transaction at `index` with buys (`sol > 0`) / sells (`sol < 0`, tokens) by `user`.
        fn tx(&mut self, sig: &str, index: u32, user: Pubkey, trades: &[i64]) {
            let mut out = vec![];
            for &x in trades {
                let (is_buy, sol, tok) = if x > 0 {
                    let tok = self.c.tokens_for_sol(x as u64);
                    self.c.apply_buy(tok, x as u64);
                    (true, x as u64, tok)
                } else {
                    let tok = (-x) as u64;
                    let sol = self.c.sell_sol_out(tok);
                    self.c.apply_sell(tok, sol);
                    (false, sol, tok)
                };
                out.push(TradeObs {
                    slot: self.slot,
                    is_buy,
                    sol,
                    tok,
                    v: self.c.virtual_sol,
                    t: self.c.virtual_token,
                    user,
                });
            }
            self.txs.push(TxObs {
                sig: sig.into(),
                slot: self.slot,
                index: Some(index),
                fee_payer: Some(user),
                signers: vec![user],
                trades: out,
            });
        }
    }

    fn cfg(filter: EntryFilter) -> E1Cfg {
        E1Cfg {
            rpc_url: String::new(),
            budget: Arc::new(RpcBudget::new(1.0)),
            delay_ms: 0,
            max_tx: 60,
            replay_slots: 12,
            late_block_frac: 0.10,
            entry_premium_bps: 110,
            max_premium_bps: 200,
            filter,
            retry_ms: 0,
            min_replay_slots: 7,
        }
    }

    const SOL: i64 = 1_000_000_000;

    #[test]
    fn bundle_run_matches_the_gap3_definition() {
        let (dev, b1, b2, late, sniper) = (
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        );
        let mut b = B::new();
        b.tx("create", 100, dev, &[2 * SOL]);
        b.tx("b1", 101, b1, &[SOL]);
        b.tx("b2", 103, b2, &[SOL / 2]); // gap 2: still the run
        let pb = (b.c.virtual_sol, b.c.virtual_token);
        b.tx("s1", 107, sniper, &[SOL / 4]); // gap 4: run over, a sniper behind us
        b.tx("late", 108, late, &[SOL / 10]);
        let cset: HashSet<Pubkey> = [dev].into_iter().collect();
        let s = analyze_slot0("create", &b.txs, 1_000, &cset, &HashSet::new()).unwrap();
        assert_eq!(s.create_index, 100);
        assert_eq!(s.bundle_trades, 3);
        assert_eq!(s.bundle_wallets, vec![b1, b2]);
        assert!(s.multi() && s.is_tape() && !s.graduated);
        assert_eq!((s.ref_v, s.ref_t), pb);
        assert_eq!(s.our_index, 104);
        assert_eq!((s.tp_ahead, s.tp_behind), (0, 2));
        assert_eq!(s.measured_premium_bps, 0);
        assert_eq!(s.after.len(), 2);
        assert_eq!(s.dev_sol, 2 * SOL as u64);
        assert_eq!(s.bundle_sol, 3 * SOL as u64 / 2);
        // mcap = price x 1e9 tokens: ~ 34.5 SOL post-bundle here
        assert!(
            (30_000_000_000..40_000_000_000).contains(&s.mcap()),
            "{}",
            s.mcap()
        );
        // with the 1.1% default premium it fills
        assert_eq!(
            verdict(&cfg(EntryFilter::MultiBundle), &s),
            (ShadowOutcome::Filled, 110)
        );
    }

    #[test]
    fn index_beyond_create_plus_five_or_a_sell_ends_nothing_but_the_window() {
        let (dev, a, c) = (
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        );
        let mut b = B::new();
        b.tx("create", 10, dev, &[SOL]);
        b.tx("x", 12, a, &[SOL]);
        b.tx("sell", 13, c, &[-1_000_000_000]); // a sell in the window: skipped, no break
        b.tx("y", 15, c, &[SOL]); // 15 - 12 = 3 > 2: the run breaks here
        let cset: HashSet<Pubkey> = [dev].into_iter().collect();
        let s = analyze_slot0("create", &b.txs, 400, &cset, &HashSet::new()).unwrap();
        assert_eq!(s.bundle_wallets, vec![a]);
        assert_eq!(s.our_index, 13);
        assert_eq!(s.tp_behind, 1);
        // a creator-set buy late in slot 0 moves the post-bundle state (gap-3 rule)
        b.tx("dev2", 30, dev, &[SOL / 10]);
        let s = analyze_slot0("create", &b.txs, 400, &cset, &HashSet::new()).unwrap();
        assert_eq!(s.our_index, 31);
        assert_eq!(s.tp_ahead, 1);
        assert_eq!(s.bundle_trades, 3);
    }

    #[test]
    fn dev_only_is_class_and_a_known_sniper_in_the_run_is_premium() {
        let (dev, sn) = (Pubkey::new_unique(), Pubkey::new_unique());
        let mut b = B::new();
        b.tx("create", 50, dev, &[SOL]);
        b.tx("dev2", 51, dev, &[SOL]);
        let cset: HashSet<Pubkey> = [dev].into_iter().collect();
        let s = analyze_slot0("create", &b.txs, 900, &cset, &HashSet::new()).unwrap();
        assert!(!s.multi() && s.is_tape());
        assert_eq!(
            verdict(&cfg(EntryFilter::MultiBundle), &s).0,
            ShadowOutcome::Class
        );
        assert_eq!(verdict(&cfg(EntryFilter::Any), &s).0, ShadowOutcome::Filled);
        // the same wallet inside the run: unknown = bundle, known sniper = ahead of us
        let mut b2 = B::new();
        b2.tx("create", 50, dev, &[SOL]);
        b2.tx("sn", 52, sn, &[SOL]);
        let ref_unknown = (b2.c.virtual_sol, b2.c.virtual_token);
        let s = analyze_slot0("create", &b2.txs, 900, &cset, &HashSet::new()).unwrap();
        assert_eq!(s.bundle_wallets, vec![sn]);
        assert_eq!((s.ref_v, s.ref_t), ref_unknown);
        let snipers: HashSet<Pubkey> = [sn].into_iter().collect();
        let s = analyze_slot0("create", &b2.txs, 900, &cset, &snipers).unwrap();
        assert!(s.bundle_wallets.is_empty());
        assert_eq!((s.tp_ahead, s.tp_behind, s.our_index), (1, 0, 53));
        // 1 SOL on ~31 SOL of virtual reserves: ~+6.6% price impact
        assert!(
            (600..700).contains(&s.measured_premium_bps),
            "{}",
            s.measured_premium_bps
        );
        let after_create = b2.txs[0].trades[0];
        assert!(
            s.ref_v.abs_diff(after_create.v) < 1_000,
            "reference = before the sniper"
        );
        let (o, p) = verdict(&cfg(EntryFilter::Any), &s);
        assert_eq!((o, p), (ShadowOutcome::Premium, s.measured_premium_bps));
    }

    #[test]
    fn late_creates_and_slot0_graduations() {
        let (dev, a) = (Pubkey::new_unique(), Pubkey::new_unique());
        let mut b = B::new();
        b.tx("create", 950, dev, &[SOL]);
        b.tx("a", 951, a, &[SOL]);
        let cset: HashSet<Pubkey> = [dev].into_iter().collect();
        let s = analyze_slot0("create", &b.txs, 1_000, &cset, &HashSet::new()).unwrap();
        assert_eq!(
            verdict(&cfg(EntryFilter::MultiBundle), &s).0,
            ShadowOutcome::NotInSlot0
        );
        let s = analyze_slot0("create", &b.txs, 1_100, &cset, &HashSet::new()).unwrap();
        assert_eq!(
            verdict(&cfg(EntryFilter::MultiBundle), &s).0,
            ShadowOutcome::Filled
        );
        let mut g = B::new();
        g.tx("create", 5, dev, &[40 * SOL]);
        g.tx("a", 6, a, &[41 * SOL]);
        let s = analyze_slot0("create", &g.txs, 1_000, &cset, &HashSet::new()).unwrap();
        assert!(s.graduated && !s.is_tape());
        assert_eq!(verdict(&cfg(EntryFilter::Any), &s).0, ShadowOutcome::Class);
        assert!(analyze_slot0("missing", &g.txs, 1_000, &cset, &HashSet::new()).is_none());
    }

    #[test]
    fn transient_rpc_errors() {
        assert!(transient(r#"http 429:  {"error":{"code": 429}}"#));
        assert!(transient("http 413: You have used your data allowance"));
        assert!(transient("rpc timeout"));
        assert!(!transient(r#"{"code":-32602,"message":"Invalid param"}"#));
    }

    #[test]
    fn slot_order_follows_the_reserve_chain() {
        let mut b = B::new();
        let u = Pubkey::new_unique();
        b.tx("a", 0, u, &[SOL, SOL / 2]);
        b.tx("b", 1, u, &[-1_000_000_000_000]);
        b.tx("c", 2, u, &[SOL / 3]);
        let all: Vec<TradeObs> = b.txs.iter().flat_map(|t| t.trades.clone()).collect();
        let shuffled = vec![all[3], all[2], all[0], all[1]];
        assert_eq!(order_slot(&shuffled), all);
        assert_eq!(order_slot(&all[..1]), all[..1].to_vec());
    }

    #[test]
    fn parses_keys_blocks_and_failed_transactions() {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../pump-codec/fixtures/tx_json/v1_create_v2_buy.json"
        );
        let v: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let (fp, signers) = keys_from_tx(&v["result"]);
        assert_eq!(signers.len(), 3, "the 3-signer create (docs/06)");
        assert_eq!(fp, Some(signers[0]));
        let parsed = serde_json::json!({"transaction": {"message": {"accountKeys": [
            {"pubkey": signers[1].to_string(), "signer": true},
            {"pubkey": signers[0].to_string(), "signer": false}
        ]}}});
        assert_eq!(keys_from_tx(&parsed), (Some(signers[1]), vec![signers[1]]));
        let failed = serde_json::json!({"meta": {"err": {"x": 1}}, "transaction": {}});
        assert!(tx_obs(&failed, &Pubkey::default(), "s", 1).is_none());
        let blk = serde_json::json!({"signatures": ["a", "b", 3]});
        assert_eq!(block_signatures(&blk), vec!["a", "b"]);
    }
}
