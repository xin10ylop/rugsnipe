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
            let r = self.call(
                "getTransaction",
                json!([bs58::encode(sig).into_string(), {"encoding": "json", "commitment": "confirmed",
                       "maxSupportedTransactionVersion": 1}]),
            )?;
            Ok(parse_tx_sol_delta(&r, owner))
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
}
