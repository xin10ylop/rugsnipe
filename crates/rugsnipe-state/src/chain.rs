//! Chain reads used by recovery and the guard. [`ChainRead`] is a trait so
//! tests (and a future gRPC source) can stand in for RPC; [`RpcChain`]
//! (feature `rpc`) uses public JSON-RPC inside an [`crate::RpcBudget`].

use base64::Engine as _;
use pump_codec::Pubkey;
use serde_json::Value;

/// A non-empty token account of the owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenBalance {
    pub mint: Pubkey,
    pub account: Pubkey,
    pub amount: u64,
}

/// What recovery and the guard read from the chain.
pub trait ChainRead {
    /// Current (confirmed) slot.
    fn slot(&self) -> Result<u64, String>;
    /// Token accounts of `owner` with a non-zero balance (SPL Token and Token-2022).
    fn token_balances(&self, owner: &Pubkey) -> Result<Vec<TokenBalance>, String>;
    /// Raw account data (`None` = account missing), in `keys` order.
    fn accounts(&self, keys: &[Pubkey]) -> Result<Vec<Option<Vec<u8>>>, String>;
    /// Lamports of `key` (the wallet balance the risk gate needs).
    fn balance(&self, _key: &Pubkey) -> Result<u64, String> {
        Err("balance: not supported by this chain source".into())
    }
    /// What a confirmed transaction did to `owner`'s lamports, fee
    /// included (`Some((delta, slot))`; `None` = not found or not yet
    /// confirmed). The guard books its own sale's proceeds with it.
    fn tx_sol_delta(&self, _sig: &[u8; 64], _owner: &Pubkey) -> Result<Option<(i64, u64)>, String> {
        Err("tx_sol_delta: not supported by this chain source".into())
    }
    /// The newest sale of `mint` by `owner` at `since_slot` or later: a
    /// confirmed transaction without an error whose account keys (static
    /// and loaded) name the mint and that raised the owner's lamports
    /// (`Some((sig, delta fee included, slot))`). The guard books a sale it
    /// did not send with it (2026-10-23: the hot box sent the sell and was
    /// restarted before its fill reached it). See [`find_sell_in`].
    fn find_owner_sell(
        &self,
        _owner: &Pubkey,
        _mint: &Pubkey,
        _since_slot: u64,
    ) -> Result<Option<([u8; 64], i64, u64)>, String> {
        Err("find_owner_sell: not supported by this chain source".into())
    }
    /// A recent blockhash (`getLatestBlockhash`, confirmed) and the slot it
    /// was read at: the sender thread's fallback when the real lane's
    /// cached blockhash is stale ([`parse_latest_blockhash`]).
    fn latest_blockhash(&self) -> Result<([u8; 32], u64), String> {
        Err("latest_blockhash: not supported by this chain source".into())
    }
}

/// Parse a `getLatestBlockhash` `result`: `(hash, context slot)`.
pub fn parse_latest_blockhash(result: &Value) -> Option<([u8; 32], u64)> {
    let hash = bs58::decode(result["value"]["blockhash"].as_str()?)
        .into_vec()
        .ok()?;
    Some((
        <[u8; 32]>::try_from(hash).ok()?,
        result["context"]["slot"].as_u64().unwrap_or(0),
    ))
}

/// [`ChainRead::find_owner_sell`]: signatures of the owner read (newest first).
pub const SELL_LOOKUP_SIGS: usize = 40;
/// [`ChainRead::find_owner_sell`]: transactions fetched at most.
pub const SELL_LOOKUP_TXS: usize = 8;

/// Parse a `getSignaturesForAddress` `result`: the signatures of
/// transactions without an error at `since_slot` or later, newest first.
pub fn parse_ok_signatures(result: &Value, since_slot: u64) -> Vec<([u8; 64], u64)> {
    let Some(list) = result.as_array() else {
        return vec![];
    };
    let mut out: Vec<([u8; 64], u64)> = list
        .iter()
        .filter_map(|e| {
            if !e.get("err").is_none_or(Value::is_null) {
                return None;
            }
            let slot = e.get("slot")?.as_u64()?;
            let sig = bs58::decode(e.get("signature")?.as_str()?)
                .into_vec()
                .ok()?;
            (slot >= since_slot).then_some((<[u8; 64]>::try_from(sig).ok()?, slot))
        })
        .collect();
    // the node lists newest first; a stable sort keeps its order within a slot
    out.sort_by_key(|e| std::cmp::Reverse(e.1));
    out
}

/// Parse a `getTransaction` (json encoding) `result` as a sale of `mint`
/// by `owner`: no error, the mint among the account keys (static or loaded
/// through a lookup table) and a positive lamport delta of the owner
/// (`Some((delta, slot))`; a buy or a failed sell costs lamports).
pub fn parse_owner_sell(result: &Value, owner: &Pubkey, mint: &Pubkey) -> Option<(i64, u64)> {
    let meta = result.get("meta")?;
    if !meta.get("err").is_none_or(Value::is_null) {
        return None;
    }
    let want = mint.to_string();
    let names = |v: Option<&Value>| {
        v.and_then(Value::as_array)
            .is_some_and(|a| a.iter().any(|k| k.as_str() == Some(want.as_str())))
    };
    let loaded = meta.get("loadedAddresses");
    if !(names(result.pointer("/transaction/message/accountKeys"))
        || names(loaded.and_then(|l| l.get("writable")))
        || names(loaded.and_then(|l| l.get("readonly"))))
    {
        return None;
    }
    let (delta, slot) = parse_tx_sol_delta(result, owner)?;
    (delta > 0).then_some((delta, slot))
}

/// [`ChainRead::find_owner_sell`] over a `getSignaturesForAddress` result
/// of the owner: the first of its error-free signatures at `since_slot` or
/// later (newest first, at most [`SELL_LOOKUP_TXS`]) whose transaction
/// (`get_tx`: a `getTransaction` json result) is a sale of `mint`.
pub fn find_sell_in(
    sigs: &Value,
    owner: &Pubkey,
    mint: &Pubkey,
    since_slot: u64,
    mut get_tx: impl FnMut(&[u8; 64]) -> Result<Value, String>,
) -> Result<Option<([u8; 64], i64, u64)>, String> {
    for (sig, _) in parse_ok_signatures(sigs, since_slot)
        .into_iter()
        .take(SELL_LOOKUP_TXS)
    {
        if let Some((delta, slot)) = parse_owner_sell(&get_tx(&sig)?, owner, mint) {
            return Ok(Some((sig, delta, slot)));
        }
    }
    Ok(None)
}

/// Parse a `getTransaction` (json encoding) `result` into `owner`'s
/// lamport delta and the slot; `None` when the transaction is not there.
pub fn parse_tx_sol_delta(result: &Value, owner: &Pubkey) -> Option<(i64, u64)> {
    if result.is_null() {
        return None;
    }
    let keys = result
        .get("transaction")?
        .get("message")?
        .get("accountKeys")?
        .as_array()?;
    let want = owner.to_string();
    let i = keys
        .iter()
        .position(|k| k.as_str() == Some(want.as_str()))?;
    let meta = result.get("meta")?;
    let pre = meta.get("preBalances")?.as_array()?.get(i)?.as_i64()?;
    let post = meta.get("postBalances")?.as_array()?.get(i)?.as_i64()?;
    let slot = result.get("slot")?.as_u64()?;
    Some((post - pre, slot))
}

/// Parse a `getTokenAccountsByOwner` (jsonParsed) `result`.
pub fn parse_token_accounts(result: &Value) -> Vec<TokenBalance> {
    let Some(list) = result.get("value").and_then(|v| v.as_array()) else {
        return vec![];
    };
    list.iter()
        .filter_map(|e| {
            let account: Pubkey = e.get("pubkey")?.as_str()?.parse().ok()?;
            let info = e.pointer("/account/data/parsed/info")?;
            let mint: Pubkey = info.get("mint")?.as_str()?.parse().ok()?;
            let amount: u64 = info
                .pointer("/tokenAmount/amount")?
                .as_str()?
                .parse()
                .ok()?;
            (amount > 0).then_some(TokenBalance {
                mint,
                account,
                amount,
            })
        })
        .collect()
}

/// Parse a `getMultipleAccounts` (base64) `result`.
pub fn parse_multiple_accounts(result: &Value, n: usize) -> Result<Vec<Option<Vec<u8>>>, String> {
    let list = result
        .get("value")
        .and_then(|v| v.as_array())
        .ok_or("getMultipleAccounts: no value array")?;
    if list.len() != n {
        return Err(format!(
            "getMultipleAccounts: {} entries for {n} keys",
            list.len()
        ));
    }
    list.iter()
        .map(|e| {
            if e.is_null() {
                return Ok(None);
            }
            let b64 = e
                .pointer("/data/0")
                .and_then(|v| v.as_str())
                .ok_or("account without base64 data")?;
            base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map(Some)
                .map_err(|e| e.to_string())
        })
        .collect()
}

#[cfg(feature = "rpc")]
pub use rpc_chain::RpcChain;

#[cfg(feature = "rpc")]
mod rpc_chain {
    use super::*;
    use crate::budget::RpcBudget;
    use pump_codec::constants::{SPL_TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID};
    use rugsnipe_tx::rpc::RpcClient;
    use serde_json::json;
    use std::sync::Arc;

    /// Public JSON-RPC reads, each call gated by the role's [`RpcBudget`].
    #[derive(Debug)]
    pub struct RpcChain {
        client: RpcClient,
        budget: Arc<RpcBudget>,
    }

    impl RpcChain {
        pub fn new(url: &str, budget: Arc<RpcBudget>) -> Self {
            Self {
                client: RpcClient::new(url),
                budget,
            }
        }

        fn call(&self, method: &str, params: Value) -> Result<Value, String> {
            self.budget.acquire_blocking(method);
            self.client.call(method, params)
        }

        /// `getTransaction` (json, confirmed, versioned) of one signature.
        fn get_tx(&self, sig: &[u8; 64]) -> Result<Value, String> {
            self.call(
                "getTransaction",
                json!([bs58::encode(sig).into_string(), {"encoding": "json", "commitment": "confirmed",
                       "maxSupportedTransactionVersion": 1}]),
            )
        }
    }

    impl ChainRead for RpcChain {
        fn slot(&self) -> Result<u64, String> {
            self.call("getSlot", json!([{"commitment": "confirmed"}]))?
                .as_u64()
                .ok_or_else(|| "getSlot: not a number".into())
        }

        fn token_balances(&self, owner: &Pubkey) -> Result<Vec<TokenBalance>, String> {
            let mut out = vec![];
            for prog in [TOKEN_2022_PROGRAM_ID, SPL_TOKEN_PROGRAM_ID] {
                let r = self.call(
                    "getTokenAccountsByOwner",
                    json!([owner.to_string(), {"programId": prog.to_string()},
                           {"encoding": "jsonParsed", "commitment": "confirmed"}]),
                )?;
                out.extend(parse_token_accounts(&r));
            }
            Ok(out)
        }

        fn balance(&self, key: &Pubkey) -> Result<u64, String> {
            self.call(
                "getBalance",
                json!([key.to_string(), {"commitment": "confirmed"}]),
            )?
            .get("value")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| "getBalance: no value".into())
        }
        fn tx_sol_delta(
            &self,
            sig: &[u8; 64],
            owner: &Pubkey,
        ) -> Result<Option<(i64, u64)>, String> {
            Ok(parse_tx_sol_delta(&self.get_tx(sig)?, owner))
        }

        fn find_owner_sell(
            &self,
            owner: &Pubkey,
            mint: &Pubkey,
            since_slot: u64,
        ) -> Result<Option<([u8; 64], i64, u64)>, String> {
            let sigs = self.call(
                "getSignaturesForAddress",
                json!([owner.to_string(), {"limit": SELL_LOOKUP_SIGS, "commitment": "confirmed"}]),
            )?;
            find_sell_in(&sigs, owner, mint, since_slot, |sig| self.get_tx(sig))
        }

        fn latest_blockhash(&self) -> Result<([u8; 32], u64), String> {
            let r = self.call("getLatestBlockhash", json!([{"commitment": "confirmed"}]))?;
            parse_latest_blockhash(&r).ok_or_else(|| "getLatestBlockhash: no blockhash".into())
        }

        fn accounts(&self, keys: &[Pubkey]) -> Result<Vec<Option<Vec<u8>>>, String> {
            let mut out = Vec::with_capacity(keys.len());
            for chunk in keys.chunks(100) {
                let list: Vec<String> = chunk.iter().map(|k| k.to_string()).collect();
                let r = self.call(
                    "getMultipleAccounts",
                    json!([list, {"encoding": "base64", "commitment": "confirmed"}]),
                )?;
                out.extend(parse_multiple_accounts(&r, chunk.len())?);
            }
            Ok(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_rpc_shapes() {
        let mint = Pubkey::new_unique();
        let acct = Pubkey::new_unique();
        let r = json!({"context": {"slot": 1}, "value": [
            {"pubkey": acct.to_string(), "account": {"data": {"parsed": {"info": {
                "mint": mint.to_string(), "tokenAmount": {"amount": "12345", "decimals": 6}}}}}},
            {"pubkey": Pubkey::new_unique().to_string(), "account": {"data": {"parsed": {"info": {
                "mint": mint.to_string(), "tokenAmount": {"amount": "0", "decimals": 6}}}}}},
            {"bad": true}
        ]});
        assert_eq!(
            parse_token_accounts(&r),
            vec![TokenBalance {
                mint,
                account: acct,
                amount: 12_345
            }]
        );
        let r = json!({"value": [null, {"data": ["AQID", "base64"]}]});
        assert_eq!(
            parse_multiple_accounts(&r, 2).unwrap(),
            vec![None, Some(vec![1, 2, 3])]
        );
        assert!(parse_multiple_accounts(&r, 3).is_err());
    }

    #[test]
    fn a_sale_is_booked_as_the_owners_lamport_delta_fee_included() {
        let owner = Pubkey::new_from_array([7; 32]);
        let r = serde_json::json!({
            "slot": 460485864u64,
            "transaction": {"message": {"accountKeys": ["11111111111111111111111111111111", owner.to_string()]}},
            "meta": {"fee": 5000, "preBalances": [1, 2_500_000_000u64], "postBalances": [1, 2_708_000_000u64]}
        });
        assert_eq!(
            parse_tx_sol_delta(&r, &owner),
            Some((208_000_000, 460485864))
        );
        assert_eq!(parse_tx_sol_delta(&serde_json::Value::Null, &owner), None);
        let other = Pubkey::new_from_array([9; 32]);
        assert_eq!(parse_tx_sol_delta(&r, &other), None);
    }

    /// A sale the guard did not send (the hot box's, booked nowhere): the
    /// newest error-free owner transaction since the position opened that
    /// names the mint (here through a lookup table) and raised the owner's
    /// lamports; the buy (lamports down), a failed sell, another mint's
    /// sale and anything before `since_slot` are passed over.
    #[test]
    fn the_owners_sale_of_a_mint_is_found_newest_first() {
        let owner = Pubkey::new_from_array([7; 32]);
        let mint = Pubkey::new_from_array([5; 32]);
        let other = Pubkey::new_from_array([6; 32]);
        let sig = |b: u8| [b; 64];
        let b58 = |s: [u8; 64]| bs58::encode(s).into_string();
        let sigs = json!([
            {"signature": b58(sig(1)), "slot": 460485870u64, "err": {"InstructionError": [2, {"Custom": 6003}]}},
            {"signature": b58(sig(2)), "slot": 460485870u64, "err": null},
            {"signature": b58(sig(3)), "slot": 460485869u64, "err": null},
            {"signature": b58(sig(4)), "slot": 460485864u64, "err": null},
            {"signature": b58(sig(5)), "slot": 460485800u64, "err": null}
        ]);
        let tx = |slot: u64, keys: Vec<String>, loaded: Vec<String>, pre: u64, post: u64| {
            json!({
                "slot": slot,
                "transaction": {"message": {"accountKeys": keys}},
                "meta": {"err": null, "fee": 5000, "preBalances": [pre, 1], "postBalances": [post, 1],
                         "loadedAddresses": {"writable": [], "readonly": loaded}}
            })
        };
        let o = owner.to_string();
        let mut fetched = vec![];
        let got = find_sell_in(&sigs, &owner, &mint, 460485864, |s| {
            fetched.push(s[0]);
            Ok(match s[0] {
                // another mint's sale
                2 => tx(
                    460485870,
                    vec![o.clone(), other.to_string()],
                    vec![],
                    1_000,
                    9_000,
                ),
                // ours: the mint comes from the lookup table; +0.2416 SOL
                3 => tx(
                    460485869,
                    vec![o.clone()],
                    vec![mint.to_string()],
                    2_000_000_000,
                    2_241_600_000,
                ),
                // the buy (never reached)
                _ => tx(
                    460485864,
                    vec![o.clone(), mint.to_string()],
                    vec![],
                    2_250_000_000,
                    2_000_000_000,
                ),
            })
        })
        .unwrap();
        assert_eq!(got, Some((sig(3), 241_600_000, 460485869)));
        assert_eq!(fetched, vec![2, 3], "the failed one is never fetched");
        // the buy alone is no sale; nothing before since_slot is looked at
        let mut fetched = vec![];
        let got = find_sell_in(&sigs, &owner, &mint, 460485864, |s| {
            fetched.push(s[0]);
            Ok(tx(
                460485864,
                vec![o.clone(), mint.to_string()],
                vec![],
                2_250_000_000,
                2_000_000_000,
            ))
        })
        .unwrap();
        assert_eq!(got, None);
        assert_eq!(fetched, vec![2, 3, 4]);
        // a transaction that failed on chain is no sale either, nor a missing one
        let mut failed = tx(1, vec![o.clone(), mint.to_string()], vec![], 1_000, 9_000);
        failed["meta"]["err"] = json!({"InstructionError": [2, {"Custom": 6003}]});
        assert_eq!(parse_owner_sell(&failed, &owner, &mint), None);
        assert_eq!(parse_owner_sell(&Value::Null, &owner, &mint), None);
        // an RPC error ends the lookup (the caller retries)
        assert!(find_sell_in(&sigs, &owner, &mint, 0, |_| Err("timeout".into())).is_err());
        // at most SELL_LOOKUP_TXS transactions are fetched
        let many: Vec<Value> = (0..20u8)
            .map(|i| json!({"signature": b58(sig(i + 10)), "slot": 100 - i as u64, "err": null}))
            .collect();
        let mut n = 0;
        let got = find_sell_in(&json!(many), &owner, &mint, 0, |_| {
            n += 1;
            Ok(Value::Null)
        })
        .unwrap();
        assert_eq!((got, n), (None, SELL_LOOKUP_TXS));
        assert_eq!(parse_ok_signatures(&json!({"bad": 1}), 0), vec![]);
    }
}
