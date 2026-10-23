//! Watched keys: an ArcSwap snapshot built from the ArmSet plus dynamic
//! entries (funding watcher, successors), read lock-free on the hot path.
//!
//! Dynamic entry lifecycle (docs/06 section 9.6):
//!
//! * **TTL** (`expires_unix`): the entry matches creates until then;
//! * **remove-if-no-launch** (`remove_if_no_launch_unix`): if no launch was
//!   seen by then the entry is deleted (between TTL and this deadline it is
//!   kept, inactive, so transfers can still spawn a successor);
//! * **keep**: once the key launched, it stays active past both deadlines
//!   until the ArmSet or an operator removes it;
//! * **last transfer**: a watched wallet moving >= `last_transfer_min_frac`
//!   of its balance to another wallet creates a `Successor` that inherits the
//!   operator, rule and deadlines.
//!
//! Writers (cold thread) serialize on a mutex and publish a new map
//! (copy-on-write); readers never block.

use crate::armset::{ArmSet, FundingRule, WatchKind};
use arc_swap::ArcSwap;
use pump_codec::create_hit::CreateHit;
use pump_codec::Pubkey;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// A key the hot path can match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchEntry {
    pub key: Pubkey,
    pub kind: WatchKind,
    pub op_id: u32,
    pub rule_id: u32,
    pub dynamic: bool,
}

/// A dynamic entry with its deadlines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DynEntry {
    pub key: Pubkey,
    pub kind: WatchKind,
    pub op_id: u32,
    pub rule_id: u32,
    pub added_slot: u64,
    pub added_unix: u64,
    pub expires_unix: u64,
    pub remove_if_no_launch_unix: u64,
    pub keep: bool,
    pub parent: Option<Pubkey>,
    pub launched: bool,
}

impl DynEntry {
    /// Matches creates at `now`.
    pub fn is_active(&self, now_unix: u64) -> bool {
        now_unix < self.expires_unix || (self.launched && self.keep)
    }
    /// Should be deleted at `now`.
    pub fn is_dead(&self, now_unix: u64) -> bool {
        if self.launched {
            !self.keep && now_unix >= self.expires_unix
        } else {
            now_unix >= self.remove_if_no_launch_unix.max(self.expires_unix)
        }
    }
    fn entry(&self) -> WatchEntry {
        WatchEntry {
            key: self.key,
            kind: self.kind,
            op_id: self.op_id,
            rule_id: self.rule_id,
            dynamic: true,
        }
    }
}

#[derive(Debug, Default)]
struct Snapshot {
    keys: HashMap<Pubkey, WatchEntry>,
    shared_fee_payers: HashSet<Pubkey>,
    funding: HashMap<Pubkey, Vec<FundingRule>>,
    armset_ver: u64,
    /// ArmSet class arm (`match_all`): operator of every unmatched create.
    match_all: Option<u32>,
}

/// How a create matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    /// A signer or the fee payer is watched: eligible for a real buy.
    Live {
        entry: WatchEntry,
        via: Via,
    },
    /// Only the `creator` argument (or a creator-kind key) matched: shadow only.
    Shadow {
        entry: WatchEntry,
    },
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Signer,
    FeePayer,
    /// No key matched; the ArmSet's class arm (`match_all`) took the create.
    Class,
}

#[derive(Debug, Default)]
pub struct Watchlist {
    snap: ArcSwap<Snapshot>,
    dynamic: ArcSwap<HashMap<Pubkey, DynEntry>>,
    write: Mutex<()>,
}

impl Watchlist {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the ArmSet part (static keys, shared fee payers, funding rules).
    pub fn load_armset(&self, a: &ArmSet) {
        let _g = self.write.lock().unwrap_or_else(|e| e.into_inner());
        let mut s = Snapshot {
            armset_ver: a.ver,
            match_all: a.match_all,
            shared_fee_payers: a.shared_fee_payers.iter().copied().collect(),
            ..Default::default()
        };
        for w in &a.watch {
            s.keys.insert(
                w.key,
                WatchEntry {
                    key: w.key,
                    kind: w.kind,
                    op_id: w.op_id,
                    rule_id: w.rule_id,
                    dynamic: false,
                },
            );
        }
        for r in &a.funding_rules {
            s.funding.entry(r.funder).or_default().push(r.clone());
        }
        self.snap.store(Arc::new(s));
    }

    pub fn armset_ver(&self) -> u64 {
        self.snap.load().armset_ver
    }

    /// Lock-free lookup of one key.
    pub fn get(&self, key: &Pubkey, now_unix: u64) -> Option<WatchEntry> {
        if let Some(e) = self.snap.load().keys.get(key) {
            return Some(*e);
        }
        self.dynamic
            .load()
            .get(key)
            .filter(|d| d.is_active(now_unix))
            .map(|d| d.entry())
    }

    /// Match a create (docs/06 section 7.1 step 5): signers and fee payer
    /// minus shared fee payers are live keys; a `creator_arg`-only match (or
    /// a creator-kind key) is shadow.
    pub fn match_hit(&self, hit: &CreateHit, now_unix: u64) -> Match {
        let snap = self.snap.load();
        let mut shadow = None;
        let mut check = |k: &Pubkey, via: Via| -> Option<Match> {
            if snap.shared_fee_payers.contains(k) {
                return None;
            }
            let e = self.get(k, now_unix)?;
            if e.kind == WatchKind::Creator {
                shadow.get_or_insert(e);
                return None;
            }
            Some(Match::Live { entry: e, via })
        };
        for s in hit.signers.as_slice() {
            if *s == hit.mint {
                continue; // the mint keypair signs every create
            }
            if let Some(m) = check(s, Via::Signer) {
                return m;
            }
        }
        if let Some(fp) = hit.fee_payer {
            if let Some(m) = check(&fp, Via::FeePayer) {
                return m;
            }
        }
        if let Some(op_id) = snap.match_all {
            // class arm: every create no key matched (docs/04 section 2)
            return Match::Live {
                entry: WatchEntry {
                    key: hit.user,
                    kind: WatchKind::Signer,
                    op_id,
                    rule_id: 0,
                    dynamic: false,
                },
                via: Via::Class,
            };
        }
        if let Some(e) = shadow {
            return Match::Shadow { entry: e };
        }
        match self.get(&hit.creator_arg, now_unix) {
            Some(e) => Match::Shadow { entry: e },
            None => Match::None,
        }
    }

    fn mutate(&self, f: impl FnOnce(&mut HashMap<Pubkey, DynEntry>)) {
        let _g = self.write.lock().unwrap_or_else(|e| e.into_inner());
        let mut m = (**self.dynamic.load()).clone();
        f(&mut m);
        self.dynamic.store(Arc::new(m));
    }

    /// Insert or replace a dynamic entry.
    pub fn upsert_dynamic(&self, e: DynEntry) {
        self.mutate(|m| {
            m.insert(e.key, e);
        });
    }

    pub fn remove_dynamic(&self, key: &Pubkey) -> bool {
        let mut removed = false;
        self.mutate(|m| removed = m.remove(key).is_some());
        removed
    }

    /// Record that `key` launched a coin (enables `keep`).
    pub fn mark_launched(&self, key: &Pubkey) {
        self.mutate(|m| {
            if let Some(e) = m.get_mut(key) {
                e.launched = true;
            }
        });
    }

    /// Drop dead entries; returns the removed keys.
    pub fn expire(&self, now_unix: u64) -> Vec<Pubkey> {
        let mut gone = vec![];
        self.mutate(|m| {
            m.retain(|k, e| {
                let dead = e.is_dead(now_unix);
                if dead {
                    gone.push(*k);
                }
                !dead
            })
        });
        gone
    }

    pub fn dynamic_entries(&self) -> Vec<DynEntry> {
        self.dynamic.load().values().copied().collect()
    }

    /// Funding watcher: a funder from the ArmSet sends `amount` to
    /// `recipient` (pre-balance `recipient_pre_lamports`). Upserts a `Funded`
    /// entry when a rule's band matches (and the recipient is fresh if the
    /// rule requires it).
    pub fn on_funding(
        &self,
        funder: &Pubkey,
        recipient: &Pubkey,
        amount: u64,
        recipient_pre_lamports: u64,
        slot: u64,
        now_unix: u64,
    ) -> Option<DynEntry> {
        let snap = self.snap.load();
        let rules = snap.funding.get(funder)?;
        let r = rules.iter().find(|r| {
            (!r.fresh_only || recipient_pre_lamports == 0)
                && r.bands.iter().any(|[lo, hi]| (*lo..=*hi).contains(&amount))
        })?;
        let e = DynEntry {
            key: *recipient,
            kind: WatchKind::Funded,
            op_id: r.op_id,
            rule_id: r.rule_id,
            added_slot: slot,
            added_unix: now_unix,
            expires_unix: now_unix + r.ttl_s,
            remove_if_no_launch_unix: now_unix + r.remove_if_no_launch_s,
            keep: true,
            parent: Some(*funder),
            launched: false,
        };
        drop(snap);
        self.upsert_dynamic(e);
        Some(e)
    }

    /// Last-transfer successor: a watched wallet `from` sends `amount` of its
    /// `from_balance_before` to `to`. At or above `min_frac` the recipient
    /// inherits the entry (kind `Successor`, same op/rule/deadlines).
    pub fn on_transfer(
        &self,
        from: &Pubkey,
        to: &Pubkey,
        amount: u64,
        from_balance_before: u64,
        min_frac: f64,
        slot: u64,
        now_unix: u64,
    ) -> Option<DynEntry> {
        if from_balance_before == 0 || from == to {
            return None;
        }
        if (amount as f64) < min_frac * from_balance_before as f64 {
            return None;
        }
        let parent = self.get(from, now_unix)?;
        let base = self.dynamic.load().get(from).copied();
        let e = DynEntry {
            key: *to,
            kind: WatchKind::Successor,
            op_id: parent.op_id,
            rule_id: parent.rule_id,
            added_slot: slot,
            added_unix: now_unix,
            expires_unix: base.map(|b| b.expires_unix).unwrap_or(now_unix + 1_800),
            remove_if_no_launch_unix: base
                .map(|b| b.remove_if_no_launch_unix)
                .unwrap_or(now_unix + 3_600),
            keep: base.map(|b| b.keep).unwrap_or(true),
            parent: Some(*from),
            launched: false,
        };
        self.upsert_dynamic(e);
        Some(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::armset::WatchJson;

    fn armset(watch: Vec<WatchJson>, shared: Vec<Pubkey>, funding: Vec<FundingRule>) -> ArmSet {
        ArmSet {
            ver: 3,
            run_id: String::new(),
            code_sha: String::new(),
            prereg_sha: String::new(),
            created_at: 0,
            valid_until: None,
            regime: "slot200".into(),
            min_hot_version: String::new(),
            gate_passed: false,
            probe_allowed: false,
            probe_stage: None,
            shared_fee_payers: shared,
            operators: vec![],
            watch,
            funding_rules: funding,
            match_all: None,
        }
    }

    fn hit(signers: &[Pubkey], fee_payer: Pubkey, creator: Pubkey) -> CreateHit {
        let mut h = CreateHit {
            source: Default::default(),
            slot: None,
            sig: [0; 64],
            version: None,
            kind: None,
            mint: Pubkey::new_unique(),
            bc: Pubkey::default(),
            abc: Pubkey::default(),
            aqbc_hint: None,
            user: signers[0],
            fee_payer: Some(fee_payer),
            creator_arg: creator,
            holder_reward: false,
            signers: Default::default(),
            quote: None,
            token_prog: Pubkey::default(),
            mayhem: false,
            cashback: false,
            intx_sol_in: 0,
            intx_tokens_out: 0,
            intx_buys: 0,
            intx_buyers: Default::default(),
            intx_exact: true,
            curve_after: None,
            launcher: Default::default(),
            logs_truncated: false,
            tx_shape: Default::default(),
        };
        for s in signers {
            h.signers.push(*s);
        }
        h
    }

    fn wj(key: Pubkey, kind: WatchKind, op: u32) -> WatchJson {
        WatchJson {
            key,
            kind,
            op_id: op,
            rule_id: 0,
            expires_slot: None,
            keep: false,
        }
    }

    #[test]
    fn signer_live_creator_shadow_shared_fee_payer_ignored() {
        let (dev, launcher_fp, creator, other) = (
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        );
        let w = Watchlist::new();
        w.load_armset(&armset(
            vec![
                wj(dev, WatchKind::Signer, 1),
                wj(launcher_fp, WatchKind::FeePayer, 2),
                wj(creator, WatchKind::Creator, 3),
            ],
            vec![launcher_fp],
            vec![],
        ));
        assert_eq!(w.armset_ver(), 3);
        // signer
        match w.match_hit(&hit(&[dev], other, other), 0) {
            Match::Live { entry, via } => assert_eq!((entry.op_id, via), (1, Via::Signer)),
            m => panic!("{m:?}"),
        }
        // shared fee payer never matches
        assert_eq!(
            w.match_hit(&hit(&[other], launcher_fp, other), 0),
            Match::None
        );
        // creator_arg only -> shadow
        match w.match_hit(&hit(&[other], other, creator), 0) {
            Match::Shadow { entry } => assert_eq!(entry.op_id, 3),
            m => panic!("{m:?}"),
        }
        // creator-kind key signing is still shadow
        assert!(matches!(
            w.match_hit(&hit(&[creator], other, other), 0),
            Match::Shadow { .. }
        ));
    }

    #[test]
    fn dynamic_ttl_no_launch_keep_and_successor() {
        let w = Watchlist::new();
        let (funder, fresh, next) = (
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        );
        w.load_armset(&armset(
            vec![],
            vec![],
            vec![FundingRule {
                funder,
                bands: vec![[899_500_000, 900_500_000]],
                fresh_only: true,
                ttl_s: 1_800,
                remove_if_no_launch_s: 3_600,
                last_transfer: true,
                op_id: 9,
                rule_id: 4,
            }],
        ));
        // wrong band, not fresh: nothing
        assert!(w
            .on_funding(&funder, &fresh, 1_000_000_000, 0, 1, 100)
            .is_none());
        assert!(w
            .on_funding(&funder, &fresh, 900_000_000, 5, 1, 100)
            .is_none());
        let e = w
            .on_funding(&funder, &fresh, 900_000_000, 0, 1, 100)
            .unwrap();
        assert_eq!((e.kind, e.op_id, e.rule_id), (WatchKind::Funded, 9, 4));
        assert!(w.get(&fresh, 100).is_some());
        assert!(w.get(&fresh, 100 + 1_800).is_none(), "TTL ends matching");
        assert!(
            w.expire(100 + 1_800).is_empty(),
            "kept until remove-if-no-launch"
        );
        // successor before removal (moves >= 90%) while still active
        assert!(w.on_transfer(&fresh, &next, 80, 100, 0.9, 2, 200).is_none());
        let s = w.on_transfer(&fresh, &next, 95, 100, 0.9, 2, 200).unwrap();
        assert_eq!(
            (s.kind, s.op_id, s.parent),
            (WatchKind::Successor, 9, Some(fresh))
        );
        assert_eq!(s.expires_unix, e.expires_unix);
        let mut gone = w.expire(100 + 3_600);
        gone.sort();
        let mut want = vec![fresh, next];
        want.sort();
        assert_eq!(gone, want, "no launch by the deadline: both removed");
        // launched + keep survives past both deadlines
        let w2 = Watchlist::new();
        let k = Pubkey::new_unique();
        w2.upsert_dynamic(DynEntry {
            key: k,
            kind: WatchKind::Funded,
            op_id: 1,
            rule_id: 1,
            added_slot: 0,
            added_unix: 0,
            expires_unix: 10,
            remove_if_no_launch_unix: 20,
            keep: true,
            parent: None,
            launched: false,
        });
        w2.mark_launched(&k);
        assert!(w2.expire(1_000).is_empty());
        assert!(w2.get(&k, 1_000).is_some());
        // without keep a launched entry dies at TTL
        w2.upsert_dynamic(DynEntry {
            keep: false,
            launched: true,
            ..w2.dynamic_entries()[0]
        });
        assert_eq!(w2.expire(10), vec![k]);
    }

    #[test]
    fn class_arm_matches_every_unmatched_create() {
        let dev = Pubkey::new_unique();
        let watched = Pubkey::new_unique();
        let w = Watchlist::new();
        let mut a = armset(
            vec![WatchJson {
                key: watched,
                kind: WatchKind::Signer,
                op_id: 1,
                rule_id: 0,
                expires_slot: None,
                keep: false,
            }],
            vec![],
            vec![],
        );
        w.load_armset(&a);
        let fp = Pubkey::new_unique();
        let mut h = hit(&[dev], fp, dev);
        assert_eq!(w.match_hit(&h, 0), Match::None);
        a.match_all = Some(7);
        w.load_armset(&a);
        match w.match_hit(&h, 0) {
            Match::Live { entry, via } => {
                assert_eq!((entry.op_id, via, entry.key), (7, Via::Class, dev))
            }
            m => panic!("{m:?}"),
        }
        // a watched key still wins over the class arm
        h.signers.push(watched);
        match w.match_hit(&h, 0) {
            Match::Live { entry, via } => assert_eq!((entry.op_id, via), (1, Via::Signer)),
            m => panic!("{m:?}"),
        }
    }
}
