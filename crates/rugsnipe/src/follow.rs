//! The operators the copied wallet follows, by their bundle wallets
//! (2026-10-25, docs/16). An operator launches from a fresh deployer wallet
//! each time but buys its bundle with the same few wallets; the copied wallet
//! attempts almost every launch of the operators it follows (one set of three
//! bundle wallets: 231 of 236 launches) and few of the rest. It follows an
//! operator per fee routing: the launches where the dev keeps the creator
//! fees and the ones where a third wallet gets them are separate decisions
//! (docs/16 section 8), so a record is a bundle wallet and a routing. The
//! cold thread counts, per record, the bundled launches it was in (`runs`,
//! every run the feed shows) and how many of those the copied wallet
//! attempted (`attempts`: its transactions on the mint, landed or failed,
//! from the gRPC `copied` filter). A record with at least `min_attempts`
//! attempts at a rate of at least `min_rate` is followed; the hot loop reads
//! the set at the fire (`entry.follow`). Each record also keeps its last
//! [`HOT_COINS`] coins' value at create + 10 (the last price there against
//! the price after the bundle, from the feed's [`RunPrice`]): a record whose
//! coins averaged at least `hot_min_value` is hot, and the copied wallet
//! stakes a hot operator's coin about 1.5x (docs/16 section 10). The records
//! persist (`follow.json` in the state directory), so a restart keeps what
//! the day taught.
//!
//! [`RunPrice`]: rugsnipe_feed::bundle::RunPrice

use arc_swap::ArcSwap;
use pump_codec::Pubkey;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

/// Who gets a launch's creator fees
/// ([`pump_codec::CreateHit::dev_fees`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Fees {
    /// A third wallet (the `creator` argument is neither the create's user
    /// nor the fee payer).
    Third,
    /// The dev keeps them.
    Dev,
    /// The feed did not decode the create.
    Unknown,
}

impl Fees {
    pub fn of(dev_fees: Option<bool>) -> Self {
        match dev_fees {
            Some(true) => Self::Dev,
            Some(false) => Self::Third,
            None => Self::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Third => "third",
            Self::Dev => "dev",
            Self::Unknown => "unknown",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "third" => Some(Self::Third),
            "dev" => Some(Self::Dev),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

/// A record: a bundle wallet and a fee routing.
pub type FollowKey = (Pubkey, Fees);

/// The published sets (hot loop reads, cold thread writes).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FollowSet {
    /// The followed records.
    pub followed: HashSet<FollowKey>,
    /// The hot records (followed or not): their last coins' value at
    /// create + 10 averaged at least `hot_min_value`.
    pub hot: HashSet<FollowKey>,
}

impl FollowSet {
    /// A set with these followed records and no hot one.
    pub fn followed<I: IntoIterator<Item = FollowKey>>(keys: I) -> Self {
        Self {
            followed: keys.into_iter().collect(),
            hot: HashSet::new(),
        }
    }

    pub fn contains(&self, k: &FollowKey) -> bool {
        self.followed.contains(k)
    }

    pub fn is_hot(&self, k: &FollowKey) -> bool {
        self.hot.contains(k)
    }

    /// Followed records.
    pub fn len(&self) -> usize {
        self.followed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.followed.is_empty()
    }
}

pub type FollowSnapshot = Arc<ArcSwap<FollowSet>>;

pub fn snapshot_empty() -> FollowSnapshot {
    Arc::new(ArcSwap::from_pointee(FollowSet::default()))
}

/// Runs held this many slots, so an attempt (the copied wallet buys at
/// create + 0 or + 1; the feed names mints of the last 4) or the price at
/// create + 10 (handed over when the stream passes it) still finds its run.
pub const OPEN_SLOTS: u64 = 40;
/// The coins a record's value is averaged over.
pub const HOT_COINS: usize = 3;
/// Open runs and pending attempts held at most (oldest dropped).
pub const MAX_OPEN: usize = 4_096;
/// A record not seen in a bundle for this many slots (about 3 days at
/// 0.21 s a slot) is forgotten.
pub const FORGET_SLOTS: u64 = 1_250_000;

/// One record's counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WalletStat {
    pub runs: u32,
    pub attempts: u32,
    pub last_slot: u64,
    /// The last coins' value at create + 10 in basis points, oldest first
    /// (`n_vals` of them).
    pub vals: [i32; HOT_COINS],
    pub n_vals: u8,
}

impl WalletStat {
    fn push_val(&mut self, bps: i32) {
        let n = self.n_vals as usize;
        if n < HOT_COINS {
            self.vals[n] = bps;
            self.n_vals += 1;
        } else {
            self.vals.rotate_left(1);
            self.vals[HOT_COINS - 1] = bps;
        }
    }

    /// The mean value of the last coins (bps), with at least 2 of them.
    pub fn mean_val(&self) -> Option<i32> {
        let n = self.n_vals as usize;
        (n >= 2)
            .then(|| (self.vals[..n].iter().map(|&v| i64::from(v)).sum::<i64>() / n as i64) as i32)
    }
}

#[derive(Debug, Clone)]
struct Open {
    slot: u64,
    keys: Vec<FollowKey>,
    attempted: bool,
    /// Reserves after the run's last bundle buy (the price the value is
    /// measured against).
    post: Option<(u64, u64)>,
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
    stats: HashMap<FollowKey, WalletStat>,
    set: HashSet<FollowKey>,
    hot: HashSet<FollowKey>,
    /// `hot_min_value` in basis points.
    hot_bps: i32,
    snapshot: FollowSnapshot,
    dirty: bool,
    pub runs: u64,
    pub attempts: u64,
    /// Attempts whose run never came (an unbundled launch, or one the feed
    /// did not assemble).
    pub unmatched: u64,
    /// Prices at create + 10 scored against a run.
    pub priced: u64,
}

impl FollowTracker {
    /// `exclude`: wallets that are never an operator's (the copied wallet,
    /// ours). `hot_min_value`: the mean value of a record's last coins at
    /// create + 10 that makes it hot (0.10 = 10% above the price after the
    /// bundle).
    pub fn new(
        snapshot: FollowSnapshot,
        min_attempts: u32,
        min_rate: f64,
        hot_min_value: f64,
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
            hot: HashSet::new(),
            hot_bps: (hot_min_value.clamp(-1.0, 100.0) * 10_000.0).round() as i32,
            snapshot,
            dirty: false,
            runs: 0,
            attempts: 0,
            unmatched: 0,
            priced: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.set.len()
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    /// Records held (followed or not).
    pub fn records(&self) -> usize {
        self.stats.len()
    }

    pub fn stat(&self, w: &Pubkey, fees: Fees) -> Option<WalletStat> {
        self.stats.get(&(*w, fees)).copied()
    }

    pub fn followed(&self, w: &Pubkey, fees: Fees) -> bool {
        self.set.contains(&(*w, fees))
    }

    pub fn is_hot(&self, w: &Pubkey, fees: Fees) -> bool {
        self.hot.contains(&(*w, fees))
    }

    /// Hot records.
    pub fn hot_len(&self) -> usize {
        self.hot.len()
    }

    fn passes(&self, s: &WalletStat) -> bool {
        s.attempts >= self.min_attempts
            && u64::from(s.attempts) * 10_000 >= u64::from(s.runs) * u64::from(self.min_rate_bps)
    }

    fn refresh(&mut self, k: FollowKey) {
        let on = self.stats.get(&k).is_some_and(|s| self.passes(s));
        let changed = if on {
            self.set.insert(k)
        } else {
            self.set.remove(&k)
        };
        self.dirty |= changed;
        let hot = self
            .stats
            .get(&k)
            .and_then(|s| s.mean_val())
            .is_some_and(|v| v >= self.hot_bps);
        let changed = if hot {
            self.hot.insert(k)
        } else {
            self.hot.remove(&k)
        };
        self.dirty |= changed;
    }

    /// A bundled launch: its fee routing, its bundle wallets (outside the
    /// creator set, as `filter::bundle_features` finds them) and the
    /// reserves after its last bundle buy.
    pub fn on_run(
        &mut self,
        mint: Pubkey,
        slot: u64,
        fees: Fees,
        wallets: &[Pubkey],
        post: Option<(u64, u64)>,
    ) {
        let mut ks: Vec<FollowKey> = vec![];
        for w in wallets {
            if !self.exclude.contains(w) && !ks.iter().any(|(k, _)| k == w) {
                ks.push((*w, fees));
            }
        }
        if ks.is_empty() || self.open.contains_key(&mint) {
            return;
        }
        self.runs += 1;
        for k in &ks {
            let s = self.stats.entry(*k).or_default();
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
                keys: ks.clone(),
                attempted: false,
                post,
            },
        );
        if self.pending.remove(&mint).is_some() {
            self.attempt_open(&mint);
        }
        for k in ks {
            self.refresh(k);
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
        let ks = o.keys.clone();
        for k in ks {
            let s = self.stats.entry(k).or_default();
            s.attempts = s.attempts.saturating_add(1);
            self.refresh(k);
        }
    }

    /// A create's last price at create + 10 (`v`, `t`: virtual reserves):
    /// scored against its run's price after the bundle, into each of its
    /// records' last values. A mint with no open run (unbundled, or not
    /// assembled) is ignored.
    pub fn on_price(&mut self, mint: &Pubkey, v: u64, t: u64) {
        let Some(o) = self.open.get(mint) else {
            return;
        };
        let Some((v0, t0)) = o.post else {
            return;
        };
        if t == 0 || t0 == 0 || v0 == 0 {
            return;
        }
        let x = (v as f64 / t as f64) / (v0 as f64 / t0 as f64) - 1.0;
        let bps = (x * 10_000.0).round().clamp(-10_000.0, 1_000_000.0) as i32;
        self.priced += 1;
        let ks = o.keys.clone();
        for k in ks {
            if let Some(s) = self.stats.get_mut(&k) {
                s.push_val(bps);
            }
            self.refresh(k);
        }
    }

    /// Publish the sets to the hot loop when they changed.
    pub fn publish(&mut self) {
        if self.dirty {
            self.snapshot.store(Arc::new(FollowSet {
                followed: self.set.clone(),
                hot: self.hot.clone(),
            }));
            self.dirty = false;
        }
    }

    /// Forget runs past the attempt window, attempts whose run never came
    /// and records not seen for [`FORGET_SLOTS`].
    pub fn prune(&mut self, now_slot: u64) {
        let floor = now_slot.saturating_sub(OPEN_SLOTS);
        self.open.retain(|_, o| o.slot >= floor);
        let before = self.pending.len();
        self.pending.retain(|_, s| *s >= floor);
        self.unmatched += (before - self.pending.len()) as u64;
        let old = now_slot.saturating_sub(FORGET_SLOTS);
        let gone: Vec<FollowKey> = self
            .stats
            .iter()
            .filter(|(_, s)| s.last_slot < old)
            .map(|(k, _)| *k)
            .collect();
        for k in gone {
            self.stats.remove(&k);
            self.dirty |= self.set.remove(&k);
            self.dirty |= self.hot.remove(&k);
        }
    }

    /// The records as JSON (`{"wallet:routing": [runs, attempts,
    /// last_slot, value...]}`, routing `dev`, `third` or `unknown`, then the
    /// last coins' values at create + 10 in basis points, oldest first).
    pub fn to_json(&self) -> String {
        let m: std::collections::BTreeMap<String, Vec<i64>> = self
            .stats
            .iter()
            .map(|((w, f), s)| {
                let mut r = vec![i64::from(s.runs), i64::from(s.attempts), s.last_slot as i64];
                r.extend(s.vals[..s.n_vals as usize].iter().map(|&v| i64::from(v)));
                (format!("{w}:{}", f.as_str()), r)
            })
            .collect();
        serde_json::to_string(&m).unwrap_or_else(|_| "{}".into())
    }

    /// Load the records saved by [`FollowTracker::save`] (a missing file is
    /// an empty start); the sets are rebuilt and published. A record of
    /// three numbers (no values yet) loads too. Returns the records loaded
    /// and the keys skipped (a wallet without a routing: the first format,
    /// counted per wallet alone).
    pub fn load(&mut self, path: &Path) -> Result<(usize, usize), String> {
        let b = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((0, 0)),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let m: std::collections::BTreeMap<String, Vec<i64>> =
            serde_json::from_slice(&b).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut skipped = 0;
        for (k, v) in m {
            let key = k
                .split_once(':')
                .and_then(|(w, f)| Some((w.parse::<Pubkey>().ok()?, Fees::parse(f)?)));
            let (Some(key), true) = (key, v.len() >= 3) else {
                skipped += 1;
                continue;
            };
            let n = |x: i64| x.clamp(0, i64::from(u32::MAX)) as u32;
            let mut s = WalletStat {
                runs: n(v[0]),
                attempts: n(v[1]),
                last_slot: v[2].max(0) as u64,
                ..Default::default()
            };
            let vals = &v[3..];
            for &x in &vals[vals.len().saturating_sub(HOT_COINS)..] {
                s.push_val(x.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32);
            }
            self.stats.insert(key, s);
        }
        let ks: Vec<FollowKey> = self.stats.keys().copied().collect();
        for k in ks {
            self.refresh(k);
        }
        self.publish();
        Ok((self.stats.len(), skipped))
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
        (FollowTracker::new(s.clone(), 2, 0.5, 0.10, vec![k(99)]), s)
    }

    #[test]
    fn a_wallet_is_followed_after_two_attempts_at_half_its_runs() {
        let (mut t, snap) = tracker();
        let ws = [k(1), k(2)];
        t.on_run(k(10), 100, Fees::Dev, &ws, None);
        t.on_attempt(k(10), 100);
        t.publish();
        assert!(!t.followed(&k(1), Fees::Dev), "one attempt is not enough");
        t.on_run(k(11), 200, Fees::Dev, &ws, None);
        t.on_attempt(k(11), 201);
        t.publish();
        assert!(t.followed(&k(1), Fees::Dev) && t.followed(&k(2), Fees::Dev));
        assert!(
            snap.load().contains(&(k(1), Fees::Dev)),
            "published to the hot loop"
        );
        assert!(
            !t.followed(&k(1), Fees::Third),
            "the same wallet's third-wallet launches are a separate record"
        );
        // two launches it skipped: 2 of 4, still at the rate
        t.on_run(k(12), 300, Fees::Dev, &ws, None);
        t.on_run(k(13), 400, Fees::Dev, &ws, None);
        assert!(t.followed(&k(1), Fees::Dev));
        // a third-wallet launch it skipped: the dev record is unchanged
        t.on_run(k(15), 450, Fees::Third, &ws, None);
        assert!(t.followed(&k(1), Fees::Dev));
        assert_eq!(
            t.stat(&k(1), Fees::Third).map(|s| (s.runs, s.attempts)),
            Some((1, 0))
        );
        // a fifth it skipped: 2 of 5, below half
        t.on_run(k(14), 500, Fees::Dev, &ws, None);
        t.publish();
        assert!(!t.followed(&k(1), Fees::Dev));
        assert!(!snap.load().contains(&(k(1), Fees::Dev)));
        assert_eq!(
            t.stat(&k(1), Fees::Dev).map(|s| (s.runs, s.attempts)),
            Some((5, 2))
        );
    }

    #[test]
    fn an_attempt_before_its_run_counts_once_and_the_copied_wallet_is_never_an_operator() {
        let (mut t, _) = tracker();
        // the copied wallet's buy arrives before the run closes
        t.on_attempt(k(10), 100);
        t.on_attempt(k(10), 100);
        t.on_run(k(10), 100, Fees::Third, &[k(1), k(99)], None);
        t.on_attempt(k(10), 101);
        assert_eq!(
            t.stat(&k(1), Fees::Third).map(|s| (s.runs, s.attempts)),
            Some((1, 1))
        );
        assert_eq!(
            t.stat(&k(99), Fees::Third),
            None,
            "excluded wallets are not counted"
        );
        assert_eq!(t.attempts, 1);
    }

    #[test]
    fn attempts_whose_run_never_came_are_dropped_and_old_wallets_forgotten() {
        let (mut t, _) = tracker();
        t.on_attempt(k(10), 100);
        t.prune(100 + OPEN_SLOTS + 1);
        assert_eq!(t.unmatched, 1);
        // a late run of that mint takes no attempt any more
        t.on_run(k(10), 100, Fees::Dev, &[k(1)], None);
        assert_eq!(t.stat(&k(1), Fees::Dev).map(|s| s.attempts), Some(0));
        t.prune(100 + FORGET_SLOTS + 1);
        assert_eq!(t.records(), 0);
    }

    #[test]
    fn a_record_is_hot_when_its_last_coins_made_ten_percent_at_create_plus_ten() {
        let (mut t, snap) = tracker();
        let ws = [k(1)];
        // price after the bundle 40 SOL / 800 tokens; at create + 10 +12%, +9%
        t.on_run(k(10), 100, Fees::Dev, &ws, Some((40, 800)));
        t.on_price(&k(10), 448, 8_000);
        assert!(!t.is_hot(&k(1), Fees::Dev), "one coin is not enough");
        t.on_run(k(11), 200, Fees::Dev, &ws, Some((40, 800)));
        t.on_price(&k(11), 436, 8_000);
        t.publish();
        assert!(t.is_hot(&k(1), Fees::Dev), "mean +10.5%");
        assert!(snap.load().is_hot(&(k(1), Fees::Dev)));
        assert!(
            !snap.load().contains(&(k(1), Fees::Dev)),
            "hot, not followed"
        );
        // a third coin at -30%: the mean of the last 3 falls below +10%
        t.on_run(k(12), 300, Fees::Dev, &ws, Some((40, 800)));
        t.on_price(&k(12), 28, 800);
        t.publish();
        assert!(!t.is_hot(&k(1), Fees::Dev));
        assert_eq!(t.priced, 3);
        // a price for a mint with no run, or a run without its post-bundle
        // price, scores nothing
        t.on_price(&k(50), 40, 800);
        t.on_run(k(13), 400, Fees::Dev, &ws, None);
        t.on_price(&k(13), 80, 800);
        assert_eq!(t.priced, 3);
        // the values survive a restart, and a record of three numbers loads
        let dir = std::env::temp_dir().join(format!("follow-hot-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("follow.json");
        t.save(&p).unwrap();
        let (mut u, _) = tracker();
        u.load(&p).unwrap();
        assert_eq!(u.stat(&k(1), Fees::Dev).map(|s| s.n_vals), Some(3));
        let old = dir.join("three.json");
        std::fs::write(&old, format!("{{\"{}:dev\": [3, 3, 100]}}", k(2))).unwrap();
        let (mut v, _) = tracker();
        assert_eq!(v.load(&old).unwrap(), (1, 0));
        assert!(v.followed(&k(2), Fees::Dev));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_records_survive_a_restart() {
        let (mut t, _) = tracker();
        for (i, m) in [10u8, 11, 12].into_iter().enumerate() {
            t.on_run(k(m), 100 + i as u64, Fees::Third, &[k(1)], None);
            t.on_attempt(k(m), 100 + i as u64);
        }
        let dir = std::env::temp_dir().join(format!("follow-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("follow.json");
        t.save(&p).unwrap();
        let (mut u, snap) = tracker();
        assert_eq!(u.load(&p).unwrap(), (1, 0));
        assert!(u.followed(&k(1), Fees::Third) && snap.load().contains(&(k(1), Fees::Third)));
        assert_eq!(
            u.load(&dir.join("missing.json")).unwrap(),
            (0, 0),
            "missing file: nothing added"
        );
        // the first format (a wallet alone) is skipped
        let old = dir.join("old.json");
        std::fs::write(&old, format!("{{\"{}\": [3, 3, 100]}}", k(2))).unwrap();
        let (mut v, _) = tracker();
        assert_eq!(v.load(&old).unwrap(), (0, 1));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
