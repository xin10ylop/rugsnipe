//! The operator table (docs/09-operator-wave.md): repeat create signers and
//! the hold each one's launches are exited at, decoded from the round trips
//! of the wallet being copied (`analysis/scripts/strategy/operators_from_rt.py`
//! writes `data/operators/*.json`).
//!
//! The file is a JSON object keyed by the create transaction's signer (the
//! fee payer, base58):
//!
//! ```json
//! { "36X7abKn6GbhhTYfU61sGC3whuB2hjQ2evhsZTYxDhJW":
//!     {"hold_slots": 10, "n": 48, "wins": 23, "pnl_sol": 0.93, "dev_sol": 4.03, "bundle_wallets": 3} }
//! ```
//!
//! Only `hold_slots` (>= 1) is required; the rest is the evidence the rule
//! came from and is carried for the journal and reports. The table is
//! loaded once at startup (`entry.operators_file`) and read by the hot loop
//! with a plain hash lookup: no allocation, no lock.

use crate::hot::Parked;
use pump_codec::create_hit::CreateHit;
use pump_codec::Pubkey;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

/// One operator's rule and its evidence.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct OpRule {
    /// Sell everything at `create slot + hold_slots` (the copied wallet's
    /// modal sell decision offset for this signer).
    pub hold_slots: u16,
    /// Round trips of the copied wallet on this signer's launches.
    #[serde(default)]
    pub n: u32,
    /// Of which with a positive P&L.
    #[serde(default)]
    pub wins: u32,
    /// Their summed P&L, SOL.
    #[serde(default)]
    pub pnl_sol: f64,
    /// Mean bundle SOL (the create's dev buy plus the bundle run), SOL.
    #[serde(default)]
    pub dev_sol: f64,
    /// Modal distinct bundle wallets outside the creator set.
    #[serde(default)]
    pub bundle_wallets: u32,
}

/// Signer -> rule. Built once; shared behind an `Arc`.
#[derive(Debug, Clone, Default)]
pub struct OperatorTable {
    map: HashMap<Pubkey, OpRule>,
}

/// What the hot loop holds (`None` in [`crate::hot::HotCfg`]: no table).
pub type Operators = Arc<OperatorTable>;

impl OperatorTable {
    /// Parse the JSON form; every key must be a pubkey and every
    /// `hold_slots` at least 1 (an error names the first bad entry).
    pub fn from_json(s: &str) -> Result<Self, String> {
        let raw: BTreeMap<String, OpRule> =
            serde_json::from_str(s).map_err(|e| format!("operator table: {e}"))?;
        let mut map = HashMap::with_capacity(raw.len());
        for (k, r) in raw {
            let key: Pubkey = k
                .parse()
                .map_err(|e| format!("operator table: key {k:?} is not a pubkey: {e}"))?;
            if r.hold_slots == 0 {
                return Err(format!("operator table: {k}: hold_slots must be >= 1"));
            }
            map.insert(key, r);
        }
        Ok(Self { map })
    }

    /// Read and parse `path`.
    pub fn load(path: &Path) -> Result<Self, String> {
        let s = std::fs::read_to_string(path)
            .map_err(|e| format!("entry.operators_file {}: {e}", path.display()))?;
        Self::from_json(&s).map_err(|e| format!("entry.operators_file {}: {e}", path.display()))
    }

    /// The rule of a create signer (allocation-free).
    #[inline]
    pub fn get(&self, signer: &Pubkey) -> Option<&OpRule> {
        self.map.get(signer)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// The key the operator table is looked up by: the create transaction's fee
/// payer (its first signer, rt.json's `csig_signer`); the logs path, which
/// knows no fee payer, falls back to the create's `user` signer.
#[inline]
pub fn create_signer(hit: &CreateHit) -> Pubkey {
    hit.fee_payer.unwrap_or(hit.user)
}

/// The creator set of a create (rt.json's `cset`): the signer, the
/// `creator` argument, the fee payer, every signer and every buyer of the
/// create transaction.
#[inline]
pub fn in_creator_set(hit: &CreateHit, k: &Pubkey) -> bool {
    *k == hit.user
        || *k == hit.creator_arg
        || hit.fee_payer == Some(*k)
        || hit.signers.as_slice().contains(k)
        || hit.intx_buyers.as_slice().contains(k)
}

/// Distinct bundle wallets of a parked create, counted like rt.json's
/// `n_bw`: the bundle run's buyers outside the creator set. The park holds
/// at most `RUN_MAX_AFTER` (5) distinct buyers and only what the stream
/// delivered before the fire, so the count saturates at 5 and can be short
/// of the confirmed block's.
pub fn bundle_wallet_count(p: &Parked) -> u32 {
    p.wallets[..p.n_wallets as usize]
        .iter()
        .filter(|w| !in_creator_set(&p.hit, w))
        .count() as u32
}

/// `entry.bundle_wallets` admits `n` (an empty list admits every count).
#[inline]
pub fn bundle_shape_ok(allowed: &[u32], n: u32) -> bool {
    allowed.is_empty() || allowed.contains(&n)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "36X7abKn6GbhhTYfU61sGC3whuB2hjQ2evhsZTYxDhJW";

    #[test]
    fn the_table_loads_and_looks_up_by_signer() {
        let b = Pubkey::new_unique();
        let js = format!(
            r#"{{"{A}": {{"hold_slots": 10, "n": 48, "wins": 23, "pnl_sol": 0.93, "dev_sol": 4.03, "bundle_wallets": 3}},
                "{b}": {{"hold_slots": 6}}}}"#
        );
        let t = OperatorTable::from_json(&js).unwrap();
        assert_eq!(t.len(), 2);
        let a: Pubkey = A.parse().unwrap();
        let r = t.get(&a).copied().unwrap();
        assert_eq!(
            r,
            OpRule {
                hold_slots: 10,
                n: 48,
                wins: 23,
                pnl_sol: 0.93,
                dev_sol: 4.03,
                bundle_wallets: 3
            }
        );
        assert_eq!(t.get(&b).map(|r| (r.hold_slots, r.n)), Some((6, 0)));
        assert!(t.get(&Pubkey::new_unique()).is_none());
        // from a file, and the refusals
        let dir = std::env::temp_dir().join(format!("ops-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("ops.json");
        std::fs::write(&p, &js).unwrap();
        assert_eq!(OperatorTable::load(&p).unwrap().len(), 2);
        assert!(OperatorTable::load(&dir.join("missing.json")).is_err());
        assert!(OperatorTable::from_json(r#"{"not-a-key": {"hold_slots": 6}}"#).is_err());
        assert!(OperatorTable::from_json(&format!(r#"{{"{A}": {{"hold_slots": 0}}}}"#)).is_err());
        assert!(OperatorTable::from_json(&format!(r#"{{"{A}": {{"n": 4}}}}"#)).is_err());
        assert!(OperatorTable::from_json("{}").unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_checked_in_table_loads() {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/operators/ahw-2026-10-23.json"
        );
        let t = OperatorTable::load(Path::new(p)).unwrap();
        assert!(t.len() >= 33, "{}", t.len());
        let a: Pubkey = A.parse().unwrap();
        assert!(t
            .get(&a)
            .is_some_and(|r| [1, 3, 6, 10].contains(&r.hold_slots)));
    }
}
