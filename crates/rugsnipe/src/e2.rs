//! Paper fill model **E2** (docs/06 section 9.3): the paper buy fills at the
//! curve state at the end of slot `create_slot + 1`. The free feed sees a
//! create ~1 s late and subscribes to the mint even later, so the cold lane
//! reconstructs that state from confirmed history: `getSignaturesForAddress`
//! on the bonding curve, then `getTransaction` for the transactions of slots
//! `s` and `s+1` (inside the role's RPC budget), and the TradeEvents in their
//! logs. The end-of-slot state is the trade whose post-trade reserves are not
//! the pre-trade reserves of another trade in that slot (a chain); if the
//! chain is ambiguous (or truncated by the fetch cap) the highest virtual SOL
//! is used, which is pessimistic for a buyer.

use crate::now_ms;
use pump_codec::events::{events_from_log_scan, scan_program_data_logs, PumpEvent};
use pump_codec::Pubkey;
use rugsnipe_state::RpcBudget;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

/// One TradeEvent of the mint seen in a fetched transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TradeObs {
    pub slot: u64,
    pub is_buy: bool,
    pub sol: u64,
    pub tok: u64,
    /// Post-trade virtual reserves.
    pub v: u64,
    pub t: u64,
    pub user: Pubkey,
}

impl TradeObs {
    fn pre(&self) -> (u64, u64) {
        if self.is_buy {
            (
                self.v.saturating_sub(self.sol),
                self.t.saturating_add(self.tok),
            )
        } else {
            (
                self.v.saturating_add(self.sol),
                self.t.saturating_sub(self.tok),
            )
        }
    }
}

/// End-of-slot reserves of a set of trades from one slot.
pub fn end_state(trades: &[TradeObs]) -> Option<(u64, u64)> {
    if trades.is_empty() {
        return None;
    }
    let pres: Vec<(u64, u64)> = trades.iter().map(|t| t.pre()).collect();
    let ends: Vec<&TradeObs> = trades
        .iter()
        .enumerate()
        .filter(|(i, t)| {
            !pres
                .iter()
                .enumerate()
                .any(|(j, p)| j != *i && *p == (t.v, t.t))
        })
        .map(|(_, t)| t)
        .collect();
    let pick = if ends.len() == 1 {
        ends[0]
    } else {
        trades.iter().max_by_key(|t| t.v)?
    };
    Some((pick.v, pick.t))
}

/// `(signature, slot, ok)` from a `getSignaturesForAddress` result.
pub fn parse_signatures(result: &Value) -> Vec<(String, u64, bool)> {
    result
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| {
                    Some((
                        e.get("signature")?.as_str()?.to_string(),
                        e.get("slot")?.as_u64()?,
                        e.get("err").is_none_or(|x| x.is_null()),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// TradeEvents of `mint` in a `getTransaction` result (failed txs: none).
pub fn trades_from_tx(result: &Value, mint: &Pubkey, slot: u64) -> Vec<TradeObs> {
    let Some(meta) = result.get("meta") else {
        return vec![];
    };
    if meta.get("err").is_some_and(|e| !e.is_null()) {
        return vec![];
    }
    let logs: Vec<&str> = meta
        .get("logMessages")
        .and_then(|l| l.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
        .unwrap_or_default();
    let Ok(scan) = scan_program_data_logs(&logs) else {
        return vec![];
    };
    let Ok(evs) = events_from_log_scan(&scan) else {
        return vec![];
    };
    evs.iter()
        .filter_map(|(_, e)| match e {
            PumpEvent::Trade(t) if t.mint == *mint => Some(TradeObs {
                slot,
                is_buy: t.is_buy,
                sol: t.sol_amount,
                tok: t.token_amount,
                v: t.virtual_sol_reserves,
                t: t.virtual_token_reserves,
                user: t.user,
            }),
            _ => None,
        })
        .collect()
}

/// The E2 state and what was ahead of us.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct E2 {
    pub v: u64,
    pub t: u64,
    /// Buys by others in slots `s..=s+1` (outside the create transaction).
    pub buyers_ahead: u32,
    pub rival_sol_ahead: u64,
    pub fetched: u32,
}

/// Combine fetched trades into the E2 state (`create_sig_trades` are the
/// create transaction's own in-tx buys, never "ahead" of us).
pub fn e2_from(slot: u64, trades: &[TradeObs], v0: u64, t0: u64) -> E2 {
    let s1: Vec<TradeObs> = trades
        .iter()
        .copied()
        .filter(|t| t.slot == slot + 1)
        .collect();
    let s0: Vec<TradeObs> = trades.iter().copied().filter(|t| t.slot == slot).collect();
    let (v, t) = end_state(&s1)
        .or_else(|| end_state(&s0))
        .unwrap_or((v0, t0));
    let ahead: Vec<&TradeObs> = trades.iter().filter(|t| t.is_buy).collect();
    E2 {
        v,
        t,
        buyers_ahead: ahead.len() as u32,
        rival_sol_ahead: ahead.iter().map(|t| t.sol).sum(),
        fetched: 0,
    }
}

async fn budgeted(budget: &RpcBudget, method: &str) {
    loop {
        if budget.try_acquire(method) {
            return;
        }
        tokio::time::sleep(budget.wait_time(method).max(Duration::from_millis(5))).await;
    }
}

async fn call(rpc: &mut rugsnipe_feed::ws::RpcClient, body: &str) -> Result<Value, String> {
    let text = rpc.call(body).await.map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if let Some(e) = v.get("error") {
        return Err(e.to_string());
    }
    Ok(v.get("result").cloned().unwrap_or(Value::Null))
}

/// Resolve one paper fill. Waits until `decided_unix_ms + delay_ms` so the
/// slots are confirmed, fetches at most `max_tx` transactions.
pub async fn resolve(
    rpc_url: &str,
    budget: Arc<RpcBudget>,
    mint: Pubkey,
    bc: Pubkey,
    create_sig: [u8; 64],
    slot: u64,
    v0: u64,
    t0: u64,
    decided_unix_ms: u64,
    delay_ms: u64,
    max_tx: usize,
) -> Result<E2, String> {
    let due = decided_unix_ms + delay_ms;
    let now = now_ms();
    if due > now {
        tokio::time::sleep(Duration::from_millis(due - now)).await;
    }
    let mut rpc = rugsnipe_feed::ws::RpcClient::new(rpc_url).map_err(|e| e.to_string())?;
    budgeted(&budget, "getSignaturesForAddress").await;
    let body = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"getSignaturesForAddress","params":["{bc}",{{"limit":1000,"commitment":"confirmed"}}]}}"#
    );
    let sigs = parse_signatures(&call(&mut rpc, &body).await?);
    if sigs.is_empty() {
        return Err("no signatures for the bonding curve yet".into());
    }
    let create = bs58::encode(create_sig).into_string();
    // slot s+1 first (it decides the state), then slot s (rivals ahead)
    let mut want: Vec<(String, u64)> = sigs
        .iter()
        .filter(|(sig, s, ok)| *ok && *s == slot + 1 && *sig != create)
        .map(|(sig, s, _)| (sig.clone(), *s))
        .collect();
    want.extend(
        sigs.iter()
            .filter(|(sig, s, ok)| *ok && *s == slot && *sig != create)
            .map(|(sig, s, _)| (sig.clone(), *s)),
    );
    want.truncate(max_tx);
    let mut trades = vec![];
    let mut fetched = 0u32;
    for (i, (sig, s)) in want.iter().enumerate() {
        budgeted(&budget, "getTransaction").await;
        let body = rugsnipe_feed::ws::rpc::get_transaction_body(i as u64 + 2, sig);
        match call(&mut rpc, &body).await {
            Ok(r) => {
                fetched += 1;
                trades.extend(trades_from_tx(&r, &mint, *s));
            }
            Err(_) => continue,
        }
    }
    let mut e = e2_from(slot, &trades, v0, t0);
    e.fetched = fetched;
    Ok(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pump_codec::curve::CurveState;

    fn chain(n: usize) -> Vec<TradeObs> {
        let mut c = CurveState::fresh(Pubkey::default());
        let mut out = vec![];
        for i in 0..n {
            let sol = 100_000_000 * (i as u64 + 1);
            let tok = c.tokens_for_sol(sol);
            c.apply_buy(tok, sol);
            out.push(TradeObs {
                slot: 11,
                is_buy: true,
                sol,
                tok,
                v: c.virtual_sol,
                t: c.virtual_token,
                user: Pubkey::new_unique(),
            });
        }
        out
    }

    #[test]
    fn end_of_slot_follows_the_chain_in_any_order() {
        let c = chain(5);
        let last = (c[4].v, c[4].t);
        let mut shuffled = vec![c[2], c[4], c[0], c[3], c[1]];
        assert_eq!(end_state(&shuffled), Some(last));
        shuffled.reverse();
        assert_eq!(end_state(&shuffled), Some(last));
        // a sell after the buys
        let s = c[4];
        let sell_tok = s.tok / 2;
        let mut st = CurveState {
            virtual_sol: s.v,
            virtual_token: s.t,
            real_sol: 0,
            real_token: u64::MAX,
            complete: false,
            creator: Pubkey::default(),
        };
        let out = st.sell_sol_out(sell_tok);
        st.apply_sell(sell_tok, out);
        let sell = TradeObs {
            is_buy: false,
            sol: out,
            tok: sell_tok,
            v: st.virtual_sol,
            t: st.virtual_token,
            ..s
        };
        let mut with_sell = c.clone();
        with_sell.insert(1, sell);
        assert_eq!(end_state(&with_sell), Some((sell.v, sell.t)));
        // gap (missing trade): pessimistic max V
        let gap = vec![c[0], c[1], c[3]];
        assert_eq!(end_state(&gap), Some((c[3].v, c[3].t)));
        assert_eq!(end_state(&[]), None);
    }

    #[test]
    fn e2_prefers_slot_plus_one_then_slot_then_v0() {
        let mut c = chain(3);
        c[2].slot = 12; // s+1
        let e = e2_from(11, &c, 1, 2);
        assert_eq!((e.v, e.t), (c[2].v, c[2].t));
        assert_eq!(e.buyers_ahead, 3);
        let e = e2_from(11, &c[..2], 1, 2);
        assert_eq!((e.v, e.t), (c[1].v, c[1].t));
        let e = e2_from(11, &[], 30, 40);
        assert_eq!((e.v, e.t, e.buyers_ahead), (30, 40, 0));
    }

    #[test]
    fn parses_rpc_results() {
        let r: Value = serde_json::json!([
            {"signature": "a", "slot": 12, "err": null},
            {"signature": "b", "slot": 11, "err": {"InstructionError": [0, "x"]}},
            {"signature": "c", "slot": 11}
        ]);
        assert_eq!(
            parse_signatures(&r),
            vec![
                ("a".into(), 12, true),
                ("b".into(), 11, false),
                ("c".into(), 11, true)
            ]
        );
        let failed = serde_json::json!({"meta": {"err": {"x": 1}, "logMessages": []}});
        assert!(trades_from_tx(&failed, &Pubkey::default(), 1).is_empty());
    }
}
