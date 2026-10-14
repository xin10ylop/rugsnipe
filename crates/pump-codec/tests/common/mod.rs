//! Offline fixture helpers shared by the integration tests.
#![allow(dead_code)]

use base64::Engine as _;
use pump_codec::events::InnerInstruction;
use pump_codec::tx::TxView;
use pump_codec::Pubkey;
use serde_json::Value;
use std::path::PathBuf;

pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

pub fn read_json(rel: &str) -> Value {
    let p = fixtures_dir().join(rel);
    let s = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    serde_json::from_str(&s).unwrap()
}

/// Every transaction fixture name (from `tx/`).
pub fn tx_names() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(fixtures_dir().join("tx"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".json"))
        .map(|n| n.trim_end_matches(".json").to_string())
        .collect();
    v.sort();
    v
}

pub fn pk(s: &str) -> Pubkey {
    s.parse().unwrap()
}

/// A transaction fixture: raw wire bytes, decoded inner instruction data and
/// the RPC result (`meta` etc.).
pub struct TxFixture {
    pub name: String,
    pub signature: String,
    pub raw: Vec<u8>,
    pub result: Value,
    /// `(outer_index, program_id_index, accounts, data)` with base58 decoded.
    pub inner: Vec<(u8, u8, Vec<u8>, Vec<u8>)>,
    /// `meta.loadedAddresses.writable` then `.readonly`.
    pub loaded: Vec<Pubkey>,
    pub logs: Vec<String>,
}

impl TxFixture {
    pub fn load(name: &str) -> Self {
        let v = read_json(&format!("tx/{name}.json"));
        let result = v["result"].clone();
        let raw = base64::engine::general_purpose::STANDARD
            .decode(result["transaction"][0].as_str().unwrap())
            .unwrap();
        assert_eq!(result["transaction"][1], "base64");
        let meta = &result["meta"];
        let mut inner = Vec::new();
        if let Some(groups) = meta["innerInstructions"].as_array() {
            for g in groups {
                let outer = g["index"].as_u64().unwrap() as u8;
                for ix in g["instructions"].as_array().unwrap() {
                    let accts: Vec<u8> = ix["accounts"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|a| a.as_u64().unwrap() as u8)
                        .collect();
                    let data = bs58::decode(ix["data"].as_str().unwrap())
                        .into_vec()
                        .unwrap();
                    inner.push((
                        outer,
                        ix["programIdIndex"].as_u64().unwrap() as u8,
                        accts,
                        data,
                    ));
                }
            }
        }
        let mut loaded = Vec::new();
        if let Some(la) = meta.get("loadedAddresses").filter(|x| !x.is_null()) {
            for k in ["writable", "readonly"] {
                for a in la[k].as_array().unwrap() {
                    loaded.push(pk(a.as_str().unwrap()));
                }
            }
        }
        let logs = meta["logMessages"]
            .as_array()
            .map(|a| a.iter().map(|l| l.as_str().unwrap().to_string()).collect())
            .unwrap_or_default();
        Self {
            name: name.to_string(),
            signature: v["signature"].as_str().unwrap().to_string(),
            raw,
            result,
            inner,
            loaded,
            logs,
        }
    }

    pub fn view(&self) -> TxView<'_> {
        TxView::parse(&self.raw).unwrap_or_else(|e| panic!("{}: {e}", self.name))
    }

    /// Static keys followed by the ALT-loaded keys.
    pub fn full_keys(&self) -> Vec<Pubkey> {
        let mut v = self.view().static_account_keys().to_vec();
        v.extend_from_slice(&self.loaded);
        v
    }

    pub fn inner_instructions(&self) -> Vec<InnerInstruction<'_>> {
        self.inner
            .iter()
            .map(|(o, p, a, d)| InnerInstruction {
                outer_index: *o,
                program_id_index: *p,
                accounts: a,
                data: d,
            })
            .collect()
    }

    pub fn json(&self) -> Value {
        read_json(&format!("tx_json/{}.json", self.name))["result"].clone()
    }
}
