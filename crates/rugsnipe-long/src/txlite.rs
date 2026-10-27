//! One landed transaction, reduced to what the long copy reads: its keys,
//! instructions, pump trade events, balance changes and token programs.
//! Built from a Yellowstone update (live) or from a `getTransaction`
//! (`encoding = "json"`) result (the simulator, the tests).

use pump_codec::constants as c;
use pump_codec::events::{decode_cpi_event, EventSource, PumpEvent};
use pump_codec::multihop::{self, ParsedMultiHop};
use pump_codec::Pubkey;
use serde_json::Value;

/// One compiled instruction (indices into [`TxLite::keys`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ix {
    pub program: u8,
    pub accounts: Vec<u8>,
    pub data: Vec<u8>,
}

/// One token account's balance around the transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokBal {
    pub index: u32,
    pub mint: Pubkey,
    pub owner: Pubkey,
    pub program: Pubkey,
    pub pre: u64,
    pub post: u64,
}

/// A pump curve trade event, owned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trade {
    pub mint: Pubkey,
    pub user: Pubkey,
    pub is_buy: bool,
    pub token_amount: u64,
    /// Quote moved by the curve (lamports on SOL coins).
    pub quote_amount: u64,
    /// Post-trade virtual reserves.
    pub vt: u64,
    pub vq: u64,
    pub real_token: Option<u64>,
    /// `None` or a SOL-like mint on SOL coins.
    pub quote_mint: Option<Pubkey>,
    /// Top-level instruction that produced it.
    pub outer: u8,
}

impl Trade {
    pub fn sol_quoted(&self) -> bool {
        self.quote_mint
            .is_none_or(|q| c::is_sol_like_quote_mint(&q))
    }
    /// The curve sold its last token (the coin graduates).
    pub fn complete(&self) -> bool {
        self.real_token == Some(0)
    }
}

/// See the module docs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TxLite {
    pub sig: Vec<u8>,
    pub slot: u64,
    pub err: bool,
    pub fee: u64,
    /// Static keys, then ALT-loaded writable, then loaded readonly.
    pub keys: Vec<Pubkey>,
    pub outer: Vec<Ix>,
    /// (top-level index, instruction).
    pub inner: Vec<(u8, Ix)>,
    pub pre_lamports: Vec<u64>,
    pub post_lamports: Vec<u64>,
    pub tokens: Vec<TokBal>,
}

fn pk(s: &str) -> Option<Pubkey> {
    s.parse().ok()
}

impl TxLite {
    pub fn signer(&self) -> Option<Pubkey> {
        self.keys.first().copied()
    }

    pub fn sig_b58(&self) -> String {
        bs58::encode(&self.sig).into_string()
    }

    /// Every pump curve trade event (self-CPI), in order. An event that does
    /// not decode is skipped, not fatal.
    pub fn trades(&self) -> Vec<Trade> {
        let mut out = vec![];
        for (o, ix) in &self.inner {
            let Some(program) = self.keys.get(ix.program as usize) else {
                continue;
            };
            if EventSource::from_program(program) != Some(EventSource::Pump) {
                continue;
            }
            let first = ix.accounts.first().and_then(|i| self.keys.get(*i as usize));
            if first != Some(&c::EVENT_AUTHORITY) {
                continue;
            }
            if let Ok(Some(PumpEvent::Trade(t))) = decode_cpi_event(program, &ix.data) {
                out.push(Trade {
                    mint: t.mint,
                    user: t.user,
                    is_buy: t.is_buy,
                    token_amount: t.token_amount,
                    quote_amount: t.quote_amount.unwrap_or(t.sol_amount),
                    vt: t.virtual_token_reserves,
                    vq: t
                        .virtual_quote_reserves
                        .filter(|v| *v > 0)
                        .unwrap_or(t.virtual_sol_reserves),
                    real_token: t.real_token_reserves,
                    quote_mint: t.quote_mint,
                    outer: *o,
                });
            }
        }
        out
    }

    /// The top-level `multi_hop_swap` instructions.
    pub fn multihops(&self) -> Vec<ParsedMultiHop> {
        self.outer
            .iter()
            .filter(|ix| self.keys.get(ix.program as usize) == Some(&c::PUMP_SWAP_PROGRAM_ID))
            .filter_map(|ix| {
                let accts: Vec<Option<Pubkey>> = ix
                    .accounts
                    .iter()
                    .map(|i| self.keys.get(*i as usize).copied())
                    .collect();
                multihop::parse_multi_hop_swap(&accts[..], &ix.data)
            })
            .collect()
    }

    /// `true` when a top-level instruction calls `program`.
    pub fn calls(&self, program: &Pubkey) -> bool {
        self.outer
            .iter()
            .any(|ix| self.keys.get(ix.program as usize) == Some(program))
    }

    /// Lamport change of `k` (0 when not in the transaction).
    pub fn lamport_delta(&self, k: &Pubkey) -> i128 {
        match self.keys.iter().position(|x| x == k) {
            Some(i) => {
                self.post_lamports.get(i).copied().unwrap_or(0) as i128
                    - self.pre_lamports.get(i).copied().unwrap_or(0) as i128
            }
            None => 0,
        }
    }

    /// Token change of `owner`'s accounts of `mint`.
    pub fn token_delta(&self, owner: &Pubkey, mint: &Pubkey) -> i128 {
        self.tokens
            .iter()
            .filter(|b| b.owner == *owner && b.mint == *mint)
            .map(|b| b.post as i128 - b.pre as i128)
            .sum()
    }

    /// The mints whose `owner` balance fell (amount), largest first.
    pub fn spent_by(&self, owner: &Pubkey) -> Vec<(Pubkey, u64)> {
        let mut by: Vec<(Pubkey, i128)> = vec![];
        for b in self.tokens.iter().filter(|b| b.owner == *owner) {
            let d = b.post as i128 - b.pre as i128;
            match by.iter_mut().find(|(m, _)| *m == b.mint) {
                Some(e) => e.1 += d,
                None => by.push((b.mint, d)),
            }
        }
        let mut v: Vec<(Pubkey, u64)> = by
            .into_iter()
            .filter(|(_, d)| *d < 0)
            .map(|(m, d)| (m, (-d) as u64))
            .collect();
        v.sort_by_key(|x| std::cmp::Reverse(x.1));
        v
    }

    /// The token program of `mint`, read from the token balances.
    pub fn mint_program(&self, mint: &Pubkey) -> Option<Pubkey> {
        self.tokens
            .iter()
            .find(|b| b.mint == *mint)
            .map(|b| b.program)
    }

    /// The SOL `owner` spent: native lamports plus wrapped SOL (positive =
    /// spent), the network fee included when `owner` paid it.
    pub fn sol_spent(&self, owner: &Pubkey) -> i128 {
        -(self.lamport_delta(owner) + self.token_delta(owner, &c::WSOL_MINT))
    }

    /// From a `getTransaction` result (`encoding = "json"`).
    pub fn from_rpc_json(v: &Value) -> Result<Self, String> {
        let tx = &v["transaction"];
        let meta = &v["meta"];
        let msg = &tx["message"];
        let mut keys: Vec<Pubkey> = vec![];
        for k in msg["accountKeys"].as_array().ok_or("no accountKeys")? {
            keys.push(pk(k.as_str().ok_or("key")?).ok_or("bad key")?);
        }
        for part in ["writable", "readonly"] {
            if let Some(a) = meta["loadedAddresses"][part].as_array() {
                for k in a {
                    keys.push(pk(k.as_str().ok_or("key")?).ok_or("bad key")?);
                }
            }
        }
        let ix_of = |i: &Value| -> Result<Ix, String> {
            Ok(Ix {
                program: i["programIdIndex"].as_u64().ok_or("programIdIndex")? as u8,
                accounts: i["accounts"]
                    .as_array()
                    .ok_or("accounts")?
                    .iter()
                    .map(|a| a.as_u64().unwrap_or(0) as u8)
                    .collect(),
                data: bs58::decode(i["data"].as_str().unwrap_or(""))
                    .into_vec()
                    .map_err(|e| e.to_string())?,
            })
        };
        let mut outer = vec![];
        for i in msg["instructions"].as_array().ok_or("no instructions")? {
            outer.push(ix_of(i)?);
        }
        let mut inner = vec![];
        if let Some(groups) = meta["innerInstructions"].as_array() {
            for g in groups {
                let o = g["index"].as_u64().unwrap_or(0) as u8;
                for i in g["instructions"].as_array().into_iter().flatten() {
                    inner.push((o, ix_of(i)?));
                }
            }
        }
        let u64s = |a: &Value| {
            a.as_array()
                .map(|a| a.iter().map(|x| x.as_u64().unwrap_or(0)).collect())
                .unwrap_or_default()
        };
        let mut tokens: Vec<TokBal> = vec![];
        let amt = |b: &Value| {
            b["uiTokenAmount"]["amount"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0)
        };
        for (part, is_post) in [("preTokenBalances", false), ("postTokenBalances", true)] {
            for b in meta[part].as_array().into_iter().flatten() {
                let index = b["accountIndex"].as_u64().unwrap_or(0) as u32;
                let e = match tokens.iter_mut().position(|t| t.index == index) {
                    Some(p) => &mut tokens[p],
                    None => {
                        tokens.push(TokBal {
                            index,
                            mint: pk(b["mint"].as_str().unwrap_or("")).unwrap_or_default(),
                            owner: pk(b["owner"].as_str().unwrap_or("")).unwrap_or_default(),
                            program: pk(b["programId"].as_str().unwrap_or("")).unwrap_or_default(),
                            pre: 0,
                            post: 0,
                        });
                        tokens.last_mut().expect("pushed")
                    }
                };
                if is_post {
                    e.post = amt(b);
                } else {
                    e.pre = amt(b);
                }
            }
        }
        let sig = tx["signatures"][0]
            .as_str()
            .map(|s| bs58::decode(s).into_vec().unwrap_or_default())
            .unwrap_or_default();
        Ok(Self {
            sig,
            slot: v["slot"].as_u64().unwrap_or(0),
            err: !meta["err"].is_null(),
            fee: meta["fee"].as_u64().unwrap_or(0),
            keys,
            outer,
            inner,
            pre_lamports: u64s(&meta["preBalances"]),
            post_lamports: u64s(&meta["postBalances"]),
            tokens,
        })
    }

    /// From a Yellowstone transaction update (`None` without a message).
    pub fn from_grpc(
        u: &yellowstone_grpc_proto::prelude::SubscribeUpdateTransaction,
    ) -> Option<Self> {
        let info = u.transaction.as_ref()?;
        let tx = info.transaction.as_ref()?;
        let msg = tx.message.as_ref()?;
        let meta = info.meta.as_ref()?;
        let key = |b: &Vec<u8>| -> Pubkey {
            <[u8; 32]>::try_from(b.as_slice())
                .map(Pubkey::new_from_array)
                .unwrap_or_default()
        };
        let mut keys: Vec<Pubkey> = msg.account_keys.iter().map(key).collect();
        keys.extend(meta.loaded_writable_addresses.iter().map(key));
        keys.extend(meta.loaded_readonly_addresses.iter().map(key));
        let outer = msg
            .instructions
            .iter()
            .map(|i| Ix {
                program: i.program_id_index.min(255) as u8,
                accounts: i.accounts.clone(),
                data: i.data.clone(),
            })
            .collect();
        let inner = meta
            .inner_instructions
            .iter()
            .flat_map(|g| {
                g.instructions.iter().map(move |i| {
                    (
                        g.index.min(255) as u8,
                        Ix {
                            program: i.program_id_index.min(255) as u8,
                            accounts: i.accounts.clone(),
                            data: i.data.clone(),
                        },
                    )
                })
            })
            .collect();
        let mut tokens: Vec<TokBal> = vec![];
        let amt = |b: &yellowstone_grpc_proto::prelude::TokenBalance| {
            b.ui_token_amount
                .as_ref()
                .and_then(|a| a.amount.parse::<u64>().ok())
                .unwrap_or(0)
        };
        for (list, is_post) in [
            (&meta.pre_token_balances, false),
            (&meta.post_token_balances, true),
        ] {
            for b in list {
                let e = match tokens.iter_mut().position(|t| t.index == b.account_index) {
                    Some(p) => &mut tokens[p],
                    None => {
                        tokens.push(TokBal {
                            index: b.account_index,
                            mint: pk(&b.mint).unwrap_or_default(),
                            owner: pk(&b.owner).unwrap_or_default(),
                            program: pk(&b.program_id).unwrap_or_default(),
                            pre: 0,
                            post: 0,
                        });
                        tokens.last_mut().expect("pushed")
                    }
                };
                if is_post {
                    e.post = amt(b);
                } else {
                    e.pre = amt(b);
                }
            }
        }
        Some(Self {
            sig: info.signature.clone(),
            slot: u.slot,
            err: meta.err.is_some(),
            fee: meta.fee,
            keys,
            outer,
            inner,
            pre_lamports: meta.pre_balances.clone(),
            post_lamports: meta.post_balances.clone(),
            tokens,
        })
    }
}
