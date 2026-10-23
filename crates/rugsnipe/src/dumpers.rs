//! Operators who dumped (2026-10-23). A launch whose insiders (the create's
//! signer and the bundle wallets, as the feed's bundle run shows them) sold
//! at least [`DUMP_BPS`] of the tokens they bought within [`DUMP_SLOTS`] of
//! the create cannot be exited at a profit by anyone behind them; the one
//! defence is not entering their next launch. The cold thread tracks every
//! bundled launch the feed streams (the run, then the trades of its first
//! slots), marks the wallets, persists them (`dumpers`) and publishes the
//! set; the hot loop reads it at the create (`entry.dumper_skip`).

use arc_swap::ArcSwap;
use pump_codec::Pubkey;
use rugsnipe_exit::Trade;
use rugsnipe_feed::bundle::BundleRun;
use rugsnipe_state::rows::DumperRow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Slots after the create within which the insider sells count.
pub const DUMP_SLOTS: u64 = 3;
/// Share of the insiders' tokens sold that marks a dump (basis points).
pub const DUMP_BPS: u64 = 5_000;
/// Open launches tracked at most (oldest dropped).
pub const MAX_OPEN: usize = 4_096;

/// The published set (hot loop reads, cold thread writes).
pub type DumperSnapshot = Arc<ArcSwap<HashSet<Pubkey>>>;

pub fn snapshot_from(seed: Vec<Pubkey>) -> DumperSnapshot {
    Arc::new(ArcSwap::from_pointee(seed.into_iter().collect()))
}

#[derive(Debug, Clone)]
struct Track {
    slot: u64,
    /// Insider wallet -> tokens bought in the run.
    insiders: HashMap<Pubkey, u64>,
    total: u64,
    sold: u64,
    marked: bool,
}

/// The cold thread's tracker.
#[derive(Debug)]
pub struct DumpTracker {
    open: HashMap<Pubkey, Track>,
    set: HashSet<Pubkey>,
    snapshot: DumperSnapshot,
    pub launches: u64,
    pub dumps: u64,
}

impl DumpTracker {
    pub fn new(snapshot: DumperSnapshot) -> Self {
        let set: HashSet<Pubkey> = snapshot.load().iter().copied().collect();
        Self {
            open: HashMap::new(),
            set,
            snapshot,
            launches: 0,
            dumps: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.set.len()
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    pub fn contains(&self, k: &Pubkey) -> bool {
        self.set.contains(k)
    }

    /// A creation slot's bundle run: the insiders and what they bought.
    pub fn on_run(&mut self, run: &BundleRun) {
        let mut insiders: HashMap<Pubkey, u64> = HashMap::new();
        for t in run.trades.iter().filter(|t| t.is_buy && t.tok > 0) {
            *insiders.entry(t.user).or_default() += t.tok;
        }
        if insiders.is_empty() {
            return;
        }
        let total: u64 = insiders.values().sum();
        if self.open.len() >= MAX_OPEN {
            if let Some(oldest) = self
                .open
                .iter()
                .min_by_key(|(_, t)| t.slot)
                .map(|(m, _)| *m)
            {
                self.open.remove(&oldest);
            }
        }
        self.launches += 1;
        self.open.insert(
            run.mint,
            Track {
                slot: run.slot,
                insiders,
                total,
                sold: 0,
                marked: false,
            },
        );
    }

    /// A trade of a tracked mint: an insider sell within the window counts;
    /// the rows of the wallets newly marked come back (to persist).
    pub fn on_trade(&mut self, t: &Trade, now_ms: u64) -> Vec<DumperRow> {
        let mut out = vec![];
        let Some(tr) = self.open.get_mut(&t.mint) else {
            return out;
        };
        if t.is_buy
            || tr.marked
            || t.slot > tr.slot + DUMP_SLOTS
            || !tr.insiders.contains_key(&t.user)
        {
            return out;
        }
        tr.sold = tr.sold.saturating_add(t.token_amount);
        if tr.sold.saturating_mul(10_000) < tr.total.saturating_mul(DUMP_BPS) {
            return out;
        }
        tr.marked = true;
        self.dumps += 1;
        let sold_bps = (tr.sold.saturating_mul(10_000) / tr.total.max(1)).min(10_000) as u32;
        let slot = tr.slot;
        let wallets: Vec<Pubkey> = tr.insiders.keys().copied().collect();
        for w in wallets {
            if self.set.insert(w) {
                out.push(DumperRow {
                    wallet: w,
                    mint: t.mint,
                    slot,
                    sold_bps,
                    written_ms: now_ms,
                });
            }
        }
        if !out.is_empty() {
            self.snapshot.store(Arc::new(self.set.clone()));
        }
        out
    }

    /// Forget launches whose window is long over.
    pub fn prune(&mut self, now_slot: u64) {
        let floor = now_slot.saturating_sub(DUMP_SLOTS + 16);
        self.open.retain(|_, t| t.slot >= floor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rugsnipe_feed::bundle::{CloseReason, RunTrade};
    use rugsnipe_feed::event::Sig;

    fn k(b: u8) -> Pubkey {
        Pubkey::new_from_array([b; 32])
    }
    fn run(mint: Pubkey, slot: u64) -> BundleRun {
        BundleRun {
            mint,
            create_sig: Sig::default(),
            slot,
            create_index: Some(10),
            trades: vec![
                RunTrade {
                    index: 10,
                    is_create: true,
                    user: k(1),
                    is_buy: true,
                    sol: 1_000,
                    tok: 100,
                    v: 0,
                    t: 0,
                },
                RunTrade {
                    index: 11,
                    is_create: false,
                    user: k(2),
                    is_buy: true,
                    sol: 2_000,
                    tok: 200,
                    v: 0,
                    t: 0,
                },
                RunTrade {
                    index: 12,
                    is_create: false,
                    user: k(3),
                    is_buy: true,
                    sol: 3_000,
                    tok: 300,
                    v: 0,
                    t: 0,
                },
            ],
            closed: CloseReason::SlotEnded,
            create_ns: 0,
            run_closed_ns: 0,
            closed_ns: 0,
            out_of_order: 0,
        }
    }
    fn sell(mint: Pubkey, user: Pubkey, tok: u64, slot: u64) -> Trade {
        Trade {
            venue: rugsnipe_exit::Venue::Curve,
            mint,
            user,
            is_buy: false,
            sol_amount: 0,
            token_amount: tok,
            fee_lamports: 0,
            reserve_sol: 0,
            reserve_token: 0,
            slot,
        }
    }

    #[test]
    fn insiders_selling_half_within_three_slots_mark_every_insider_wallet() {
        let snap = snapshot_from(vec![]);
        let mut d = DumpTracker::new(snap.clone());
        let mint = k(9);
        d.on_run(&run(mint, 100));
        // an outsider's sell and a late insider sell count for nothing
        assert!(d.on_trade(&sell(mint, k(7), 600, 101), 1).is_empty());
        assert!(d.on_trade(&sell(mint, k(3), 300, 104), 1).is_empty());
        // 200 of 600 sold: not yet; 300 more: a dump
        assert!(d.on_trade(&sell(mint, k(2), 200, 102), 1).is_empty());
        let rows = d.on_trade(&sell(mint, k(1), 100, 103), 1);
        assert_eq!(rows.len(), 3, "{rows:?}");
        assert!(rows
            .iter()
            .all(|r| r.mint == mint && r.slot == 100 && r.sold_bps == 5_000));
        assert!(
            snap.load().contains(&k(1))
                && snap.load().contains(&k(2))
                && snap.load().contains(&k(3))
        );
        assert!(!snap.load().contains(&k(7)));
        // marked once: a later sell adds nothing
        assert!(d.on_trade(&sell(mint, k(3), 300, 103), 1).is_empty());
        assert_eq!((d.launches, d.dumps), (1, 1));
    }
}
