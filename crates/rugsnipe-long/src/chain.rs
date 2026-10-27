//! Chain reads over JSON-RPC (blocking, off the decision path where it can
//! be): accounts with a cache, the blockhash, signature statuses,
//! simulation and `sendTransaction`.

use crate::route::Chain;
use base64::Engine as _;
use pump_codec::constants as c;
use pump_codec::Pubkey;
use rugsnipe_tx::rpc::RpcClient;
use serde_json::{json, Value};
use std::collections::HashMap;

/// An account's owner, lamports and data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Acct {
    pub owner: Pubkey,
    pub lamports: u64,
    pub data: Vec<u8>,
}

pub struct Rpc {
    pub rpc: RpcClient,
    /// Accounts that do not change for our purposes (pools, mints).
    cache: HashMap<Pubkey, Option<Acct>>,
}

fn b64(s: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .unwrap_or_default()
}

impl Rpc {
    pub fn new(url: &str) -> Self {
        Self {
            rpc: RpcClient::new(url),
            cache: HashMap::new(),
        }
    }

    /// Accounts now (`None` = missing), at most 100 per call.
    pub fn accounts(&self, keys: &[Pubkey]) -> Result<Vec<Option<Acct>>, String> {
        let mut out = Vec::with_capacity(keys.len());
        for chunk in keys.chunks(100) {
            let ks: Vec<String> = chunk.iter().map(|k| k.to_string()).collect();
            let v = self.rpc.call(
                "getMultipleAccounts",
                json!([ks, {"encoding": "base64", "commitment": "processed"}]),
            )?;
            for a in v["value"]
                .as_array()
                .ok_or("getMultipleAccounts: no value")?
            {
                out.push(if a.is_null() {
                    None
                } else {
                    Some(Acct {
                        owner: a["owner"]
                            .as_str()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or_default(),
                        lamports: a["lamports"].as_u64().unwrap_or(0),
                        data: b64(a["data"][0].as_str().unwrap_or("")),
                    })
                });
            }
        }
        Ok(out)
    }

    /// One account through the cache.
    pub fn cached(&mut self, k: &Pubkey) -> Option<Acct> {
        if let Some(a) = self.cache.get(k) {
            return a.clone();
        }
        match self.accounts(&[*k]) {
            Ok(mut v) => {
                let a = v.pop().flatten();
                self.cache.insert(*k, a.clone());
                a
            }
            Err(_) => None,
        }
    }

    /// Fill the cache for several accounts in one call.
    pub fn prefetch(&mut self, keys: &[Pubkey]) {
        let want: Vec<Pubkey> = keys
            .iter()
            .filter(|k| !self.cache.contains_key(k))
            .copied()
            .collect();
        if want.is_empty() {
            return;
        }
        if let Ok(v) = self.accounts(&want) {
            for (k, a) in want.into_iter().zip(v) {
                self.cache.insert(k, a);
            }
        }
    }

    pub fn blockhash(&self) -> Result<([u8; 32], u64), String> {
        let v = self
            .rpc
            .call("getLatestBlockhash", json!([{"commitment": "confirmed"}]))?;
        let s = v["value"]["blockhash"].as_str().ok_or("no blockhash")?;
        let b = bs58::decode(s).into_vec().map_err(|e| e.to_string())?;
        let h: [u8; 32] = b.try_into().map_err(|_| "blockhash length")?;
        Ok((h, v["value"]["lastValidBlockHeight"].as_u64().unwrap_or(0)))
    }

    /// `getTransaction` (`encoding = "json"`).
    pub fn transaction(&self, sig: &str) -> Result<Value, String> {
        let v = self.rpc.call(
            "getTransaction",
            json!([sig, {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}]),
        )?;
        if v.is_null() {
            return Err(format!("{sig}: not found"));
        }
        Ok(v)
    }

    /// Simulate (signature not verified, blockhash replaced); `addrs`
    /// come back as base64 accounts after the simulation.
    pub fn simulate(&self, tx: &[u8], addrs: &[Pubkey]) -> Result<Value, String> {
        let t = base64::engine::general_purpose::STANDARD.encode(tx);
        let a: Vec<String> = addrs.iter().map(|k| k.to_string()).collect();
        let v = self.rpc.call(
            "simulateTransaction",
            json!([t, {"encoding": "base64", "sigVerify": false, "replaceRecentBlockhash": true, "commitment": "processed",
                       "accounts": {"encoding": "base64", "addresses": a}}]),
        )?;
        Ok(v["value"].clone())
    }

    /// `sendTransaction` (skip preflight: our own checks ran).
    pub fn send(&self, tx: &[u8]) -> Result<String, String> {
        let t = base64::engine::general_purpose::STANDARD.encode(tx);
        let v = self.rpc.call(
            "sendTransaction",
            json!([t, {"encoding": "base64", "skipPreflight": true, "maxRetries": 0}]),
        )?;
        v.as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| format!("sendTransaction: {v}"))
    }

    /// Signature statuses: `Some(Ok(slot))` landed, `Some(Err(e))` landed
    /// and failed, `None` not seen.
    pub fn statuses(&self, sigs: &[String]) -> Result<Vec<Option<Result<u64, String>>>, String> {
        let v = self.rpc.call(
            "getSignatureStatuses",
            json!([sigs, {"searchTransactionHistory": false}]),
        )?;
        Ok(v["value"]
            .as_array()
            .ok_or("statuses")?
            .iter()
            .map(|s| {
                if s.is_null() {
                    None
                } else if s["err"].is_null() {
                    Some(Ok(s["slot"].as_u64().unwrap_or(0)))
                } else {
                    Some(Err(s["err"].to_string()))
                }
            })
            .collect())
    }

    /// The raw amount in `owner`'s ATA of `mint` (0 when missing).
    pub fn token_balance(
        &self,
        owner: &Pubkey,
        mint: &Pubkey,
        program: &Pubkey,
    ) -> Result<u64, String> {
        let ata = pump_codec::pda::associated_token_address(owner, mint, program);
        Ok(self
            .accounts(&[ata])?
            .pop()
            .flatten()
            .map(|a| token_amount(&a.data))
            .unwrap_or(0))
    }

    pub fn balance(&self, k: &Pubkey) -> Result<u64, String> {
        let v = self.rpc.call(
            "getBalance",
            json!([k.to_string(), {"commitment": "processed"}]),
        )?;
        v["value"].as_u64().ok_or_else(|| "getBalance".into())
    }

    /// A mint's decimals (cached).
    pub fn decimals(&mut self, mint: &Pubkey) -> Option<u8> {
        if *mint == c::WSOL_MINT {
            return Some(9);
        }
        self.cached(mint).and_then(|a| a.data.get(44).copied())
    }
}

/// The amount of a token account (SPL or Token-2022 base layout).
pub fn token_amount(data: &[u8]) -> u64 {
    data.get(64..72)
        .map(|b| u64::from_le_bytes(b.try_into().expect("8 bytes")))
        .unwrap_or(0)
}

impl Chain for Rpc {
    fn pool_exists(&mut self, pool: &Pubkey) -> bool {
        self.cached(pool)
            .is_some_and(|a| a.owner == c::PUMP_SWAP_PROGRAM_ID && !a.data.is_empty())
    }
    fn mint_program(&mut self, mint: &Pubkey) -> Option<Pubkey> {
        if *mint == c::WSOL_MINT {
            return Some(c::SPL_TOKEN_PROGRAM_ID);
        }
        self.cached(mint)
            .map(|a| a.owner)
            .filter(|o| *o == c::SPL_TOKEN_PROGRAM_ID || *o == c::TOKEN_2022_PROGRAM_ID)
    }
}
