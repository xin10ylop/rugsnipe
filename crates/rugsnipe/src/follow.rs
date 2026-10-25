//! The operators the copied wallet follows, by their bundle wallets
//! (2026-10-25, docs/16). An operator launches from a fresh deployer wallet
//! each time but buys its bundle with the same few wallets; the copied wallet
//! attempts almost every launch of the operators it follows (one set of three
//! bundle wallets: 231 of 236 launches) and few of the rest. The cold thread
//! counts, per bundle wallet, the bundled launches it was in (`runs`, every
//! run the feed shows) and how many of those the copied wallet attempted
//! (`attempts`: its transactions on the mint, landed or failed, from the
//! gRPC `copied` filter). A wallet with at least `min_attempts` attempts at
//! a rate of at least `min_rate` is followed; the hot loop reads the set at
//! the fire (`entry.follow`). The counts persist (`follow.json` in the state
//! directory), so a restart keeps what the day taught.

use arc_swap::ArcSwap;
use pump_codec::Pubkey;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

/// The published set (hot loop reads, cold thread writes).
pub type FollowSnapshot = Arc<ArcSwap<HashSet<Pubkey>>>;

pub fn snapshot_empty() -> FollowSnapshot {
    Arc::new(ArcSwap::from_pointee(HashSet::new()))
}

/// Runs older than this many slots no longer take an attempt (the copied
/// wallet buys at create + 0 or + 1; the feed names mints of the last 4).
pub const OPEN_SLOTS: u64 = 16;
/// Open runs and pending attempts held at most (oldest dropped).
pub const MAX_OPEN: usize = 4_096;
/// A wallet not seen in a bundle for this many slots (about 3 days) is
/// forgotten.
pub const FORGET_SLOTS: u64 = 650_000;

/// One bundle wallet's record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WalletStat {
    pub runs: u32,
    pub attempts: u32,
    pub last_slot: u64,
}

#[derive(Debug, Clone)]
struct Open {
    slot: u64,
    wallets: Vec<Pubkey>,
    attempted: bool,
}

/// The cold thread's tracker.
#[derive(Debug)]
pub struct FollowTracker {
    min_attempts: u32,
    /// `min_rate` in basis points.
    min_rate_bps: u32,
    exclude: Vec<Pubkey>,
    open: HashMap<Pubkey, Open>,
    /// Attempts on a mint whose run has not come yet (mint -> slot).
    pending: HashMap<Pubkey, u64>,
    stats: HashMap<Pubkey, WalletStat>,
    set: HashSet<Pubkey>,
    snapshot: FollowSnapshot,
    dirty: bool,
    pub runs: u64,
    pub attempts: u64,
    /// Attempts whose run never came (an unbundled launch, or one the feed
    /// did not assemble).
    pub unmatched: u64,
}

impl FollowTracker {
    /// `exclude`: wallets that are never an operator's (the copied wallet,
    /// ours).
    pub fn new(
        snapshot: FollowSnapshot,
        min_attempts: u32,
        min_rate: f64,
        exclude: Vec<Pubkey>,
    ) -> Self {
        Self {
            min_attempts: min_attempts.max(1),
            min_rate_bps: (min_rate.clamp(0.0, 1.0) * 10_000.0).round() as u32,
            exclude,
            open: HashMap::new(),
            pending: HashMap::new(),
            stats: HashMap::new(),
            set: HashSet::new(),
            snapshot,
            dirty: false,
            runs: 0,
            attempts: 0,
            unmatched: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.set.len()
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    pub fn wallets(&self) -> usize {
        self.stats.len()
    }

    pub fn stat(&self, w: &Pubkey) -> Option<WalletStat> {
        self.stats.get(w).copied()
    }

    pub fn followed(&self, w: &Pubkey) -> bool {
        self.set.contains(w)
    }

    fn passes(&self, s: &WalletStat) -> bool {
        s.attempts >= self.min_attempts
            && u64::from(s.attempts) * 10_000 >= u64::from(s.runs) * u64::from(self.min_rate_bps)
    }

    fn refresh(&mut self, w: Pubkey) {
        let on = self.stats.get(&w).is_some_and(|s| self.passes(s));
        let changed = if on {
            self.set.insert(w)
        } else {
            self.set.remove(&w)
        };
        self.dirty |= changed;
    }

    /// A bundled launch: its bundle wallets (outside the creator set, as
    /// `filter::bundle_features` finds them).
    pub fn on_run(&mut self, mint: Pubkey, slot: u64, wallets: &[Pubkey]) {
        let mut ws: Vec<Pubkey> = vec![];
        for w in wallets {
            if !self.exclude.contains(w) && !ws.contains(w) {
                ws.push(*w);
            }
        }
        if ws.is_empty() || self.open.contains_key(&mint) {
            return;
        }
        self.runs += 1;
        for w in &ws {
            let s = self.stats.entry(*w).or_default();
            s.runs = s.runs.saturating_add(1);
            s.last_slot = s.last_slot.max(slot);
        }
        if self.open.len() >= MAX_OPEN {
            if let Some(oldest) = self
                .open
                .iter()
                .min_by_key(|(_, o)| o.slot)
                .map(|(m, _)| *m)
            {
                self.open.remove(&oldest);
            }
        }
        self.open.insert(
            mint,
            Open {
                slot,
                wallets: ws.clone(),
                attempted: false,
            },
        );
        if self.pending.remove(&mint).is_some() {
            self.attempt_open(&mint);
        }
        for w in ws {
            self.refresh(w);
        }
    }

    /// The copied wallet's transaction on a fresh mint. Counted once per
    /// mint; before its run came it waits (`pending`).
    pub fn on_attempt(&mut self, mint: Pubkey, slot: u64) {
        if self.open.contains_key(&mint) {
            self.attempt_open(&mint);
            return;
        }
        if self.pending.len() >= MAX_OPEN {
            if let Some(oldest) = self
                .pending
                .iter()
                .min_by_key(|(_, s)| **s)
                .map(|(m, _)| *m)
            {
                self.pending.remove(&oldest);
                self.unmatched += 1;
            }
        }
        self.pending.entry(mint).or_insert(slot);
    }

    fn attempt_open(&mut self, mint: &Pubkey) {
        let Some(o) = self.open.get_mut(mint) else {
            return;
        };
        if o.attempted {
            return;
        }
        o.attempted = true;
        self.attempts += 1;
        let ws = o.wallets.clone();
        for w in ws {
            let s = self.stats.entry(w).or_default();
            s.attempts = s.attempts.saturating_add(1);
            self.refresh(w);
        }
    }

    /// Publish the set to the hot loop when it changed.
    pub fn publish(&mut self) {
        if self.dirty {
            self.snapshot.store(Arc::new(self.set.clone()));
            self.dirty = false;
        }
    }

    /// Forget runs past the attempt window, attempts whose run never came
    /// and wallets not seen for [`FORGET_SLOTS`].
    pub fn prune(&mut self, now_slot: u64) {
        let floor = now_slot.saturating_sub(OPEN_SLOTS);
        self.open.retain(|_, o| o.slot >= floor);
        let before = self.pending.len();
        self.pending.retain(|_, s| *s >= floor);
        self.unmatched += (before - self.pending.len()) as u64;
        let old = now_slot.saturating_sub(FORGET_SLOTS);
        let gone: Vec<Pubkey> = self
            .stats
            .iter()
            .filter(|(_, s)| s.last_slot < old)
            .map(|(w, _)| *w)
            .collect();
        for w in gone {
            self.stats.remove(&w);
            self.dirty |= self.set.remove(&w);
        }
    }

    /// The records as JSON (`{wallet: [runs, attempts, last_slot]}`).
    pub fn to_json(&self) -> String {
        let m: std::collections::BTreeMap<String, [u64; 3]> = self
            .stats
            .iter()
            .map(|(w, s)| {
                (
                    w.to_string(),
                    [u64::from(s.runs), u64::from(s.attempts), s.last_slot],
                )
            })
            .collect();
        serde_json::to_string(&m).unwrap_or_else(|_| "{}".into())
    }

    /// Load the records saved by [`FollowTracker::save`] (a missing file is
    /// an empty start); the set is rebuilt and published.
    pub fn load(&mut self, path: &Path) -> Result<usize, String> {
        let b = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let m: std::collections::BTreeMap<String, [u64; 3]> =
            serde_json::from_slice(&b).map_err(|e| format!("{}: {e}", path.display()))?;
        for (k, v) in m {
            let Ok(w) = k.parse::<Pubkey>() else {
                continue;
            };
            self.stats.insert(
                w,
                WalletStat {
                    runs: v[0].min(u64::from(u32::MAX)) as u32,
                    attempts: v[1].min(u64::from(u32::MAX)) as u32,
                    last_slot: v[2],
                },
            );
        }
        let ws: Vec<Pubkey> = self.stats.keys().copied().collect();
        for w in ws {
            self.refresh(w);
        }
        self.publish();
        Ok(self.stats.len())
    }

    /// Write the records (temporary file, then rename).
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, self.to_json()).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(b: u8) -> Pubkey {
        Pubkey::new_from_array([b; 32])
    }

    fn tracker() -> (FollowTracker, FollowSnapshot) {
        let s = snapshot_empty();
        (FollowTracker::new(s.clone(), 2, 0.5, vec![k(99)]), s)
    }

    #[test]
    fn a_wallet_is_followed_after_two_attempts_at_half_its_runs() {
        let (mut t, snap) = tracker();
        let ws = [k(1), k(2)];
        t.on_run(k(10), 100, &ws);
        t.on_attempt(k(10), 100);
        t.publish();
        assert!(!t.followed(&k(1)), "one attempt is not enough");
        t.on_run(k(11), 200, &ws);
        t.on_attempt(k(11), 201);
        t.publish();
        assert!(t.followed(&k(1)) && t.followed(&k(2)));
        assert!(snap.load().contains(&k(1)), "published to the hot loop");
        // two launches it skipped: 2 of 4, still at the rate
        t.on_run(k(12), 300, &ws);
        t.on_run(k(13), 400, &ws);
        assert!(t.followed(&k(1)));
        // a fifth it skipped: 2 of 5, below half
        t.on_run(k(14), 500, &ws);
        t.publish();
        assert!(!t.followed(&k(1)));
        assert!(!snap.load().contains(&k(1)));
        assert_eq!(t.stat(&k(1)).map(|s| (s.runs, s.attempts)), Some((5, 2)));
    }

    #[test]
    fn an_attempt_before_its_run_counts_once_and_the_copied_wallet_is_never_an_operator() {
        let (mut t, _) = tracker();
        // the copied wallet's buy arrives before the run closes
        t.on_attempt(k(10), 100);
        t.on_attempt(k(10), 100);
        t.on_run(k(10), 100, &[k(1), k(99)]);
        t.on_attempt(k(10), 101);
        assert_eq!(t.stat(&k(1)).map(|s| (s.runs, s.attempts)), Some((1, 1)));
        assert_eq!(t.stat(&k(99)), None, "excluded wallets are not counted");
        assert_eq!(t.attempts, 1);
    }

    #[test]
    fn attempts_whose_run_never_came_are_dropped_and_old_wallets_forgotten() {
        let (mut t, _) = tracker();
        t.on_attempt(k(10), 100);
        t.prune(100 + OPEN_SLOTS + 1);
        assert_eq!(t.unmatched, 1);
        // a late run of that mint takes no attempt any more
        t.on_run(k(10), 100, &[k(1)]);
        assert_eq!(t.stat(&k(1)).map(|s| s.attempts), Some(0));
        t.prune(100 + FORGET_SLOTS + 1);
        assert_eq!(t.wallets(), 0);
    }

    #[test]
    fn the_records_survive_a_restart() {
        let (mut t, _) = tracker();
        for (i, m) in [10u8, 11, 12].into_iter().enumerate() {
            t.on_run(k(m), 100 + i as u64, &[k(1)]);
            t.on_attempt(k(m), 100 + i as u64);
        }
        let dir = std::env::temp_dir().join(format!("follow-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("follow.json");
        t.save(&p).unwrap();
        let (mut u, snap) = tracker();
        assert_eq!(u.load(&p).unwrap(), 1);
        assert!(u.followed(&k(1)) && snap.load().contains(&k(1)));
        assert_eq!(
            u.load(&dir.join("missing.json")).unwrap(),
            0,
            "missing file: nothing added"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
