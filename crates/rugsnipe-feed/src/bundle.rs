//! The slot-0 bundle run assembled from a post-execution stream
//! (docs/04 section 2, `rugsnipe::filter::bundle_features`).
//!
//! The entry filter's 17 bundle features need the creation slot's trades of
//! the new mint in block order: the create transaction's own buys, then the
//! contiguous run of buys at index <= create + 5 with gaps <= 2 (the gap-3
//! bundle definition), then any later third-party slot-0 buys. In paper the
//! E1 lane rebuilds that slot from confirmed history; with the gRPC feed the
//! adapter thread collects it live: [`BundleAssembler`] opens a run per
//! create (keyed by mint), adds every trade of that mint in the same slot
//! with its transaction index, and closes the run when the stream moves to a
//! later slot (every slot-0 trade is then known) or after a timeout. Trades
//! may arrive out of index order; a closed run is sorted by index.
//! `run_closed_ns` is when the bundle run itself was complete (a trade of the
//! slot at index > create + 5 seen, or the slot ended): the earliest moment
//! the bundle features are final.

use crate::event::Sig;
use pump_codec::Pubkey;

/// One trade of the creation slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunTrade {
    /// Transaction index in the block (`u32::MAX` if the stream gave none).
    pub index: u32,
    /// In the create transaction.
    pub is_create: bool,
    pub user: Pubkey,
    pub is_buy: bool,
    pub sol: u64,
    pub tok: u64,
    /// Post-trade virtual reserves.
    pub v: u64,
    pub t: u64,
}

/// Why a run was handed over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    /// The stream reached a later slot.
    SlotEnded,
    /// No later slot within the timeout.
    Timeout,
    /// The assembler holds too many open runs (oldest evicted).
    Evicted,
}

impl CloseReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SlotEnded => "slot_ended",
            Self::Timeout => "timeout",
            Self::Evicted => "evicted",
        }
    }
}

/// A creation slot as the stream showed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleRun {
    pub mint: Pubkey,
    pub create_sig: Sig,
    pub slot: u64,
    pub create_index: Option<u32>,
    /// The slot's trades of the mint, sorted by index (create first).
    pub trades: Vec<RunTrade>,
    pub closed: CloseReason,
    /// Monotonic times: the create seen, the bundle run complete, handed over.
    pub create_ns: u64,
    pub run_closed_ns: u64,
    pub closed_ns: u64,
    /// Trades that arrived with a lower index than one already held.
    pub out_of_order: u32,
}

struct Open {
    run: BundleRun,
    last_index: u32,
    run_done: bool,
}

/// Collects creation-slot trades per mint (adapter thread; allocates).
pub struct BundleAssembler {
    open: Vec<Open>,
    closed: Vec<BundleRun>,
    max_open: usize,
    timeout_ns: u64,
}

impl std::fmt::Debug for BundleAssembler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BundleAssembler")
            .field("open", &self.open.len())
            .field("closed", &self.closed.len())
            .finish()
    }
}

/// Index distance after the create that ends the gap-3 bundle run.
pub const RUN_MAX_AFTER: u32 = 5;

impl BundleAssembler {
    pub fn new(max_open: usize, timeout_ms: u64) -> Self {
        Self {
            open: Vec::new(),
            closed: Vec::new(),
            max_open: max_open.max(1),
            timeout_ns: timeout_ms.max(1) * 1_000_000,
        }
    }

    /// Runs open now.
    pub fn open_runs(&self) -> usize {
        self.open.len()
    }

    /// A create: open a run for `mint` with the create transaction's own
    /// trades of it.
    pub fn on_create(
        &mut self,
        mint: Pubkey,
        sig: Sig,
        slot: u64,
        index: Option<u32>,
        now_ns: u64,
        create_trades: &[RunTrade],
    ) {
        if self.open.iter().any(|o| o.run.mint == mint) {
            return;
        }
        if self.open.len() >= self.max_open {
            let o = self.open.remove(0);
            self.close(o, CloseReason::Evicted, now_ns);
        }
        let idx = index.unwrap_or(u32::MAX);
        let trades = create_trades
            .iter()
            .map(|t| RunTrade {
                index: idx,
                is_create: true,
                ..*t
            })
            .collect();
        self.open.push(Open {
            run: BundleRun {
                mint,
                create_sig: sig,
                slot,
                create_index: index,
                trades,
                closed: CloseReason::SlotEnded,
                create_ns: now_ns,
                run_closed_ns: 0,
                closed_ns: 0,
                out_of_order: 0,
            },
            last_index: idx,
            run_done: false,
        });
    }

    /// Trades of one transaction (`(mint, trade)` pairs) at `slot`/`index`.
    /// Only mints with an open run in that slot are kept.
    pub fn on_trades(
        &mut self,
        slot: u64,
        index: Option<u32>,
        now_ns: u64,
        trades: &[(Pubkey, RunTrade)],
    ) {
        let idx = index.unwrap_or(u32::MAX);
        for o in self.open.iter_mut().filter(|o| o.run.slot == slot) {
            if let (Some(ci), false) = (o.run.create_index, o.run_done) {
                if idx != u32::MAX && idx > ci.saturating_add(RUN_MAX_AFTER) {
                    o.run_done = true;
                    o.run.run_closed_ns = now_ns;
                }
            }
            for (m, t) in trades {
                if *m != o.run.mint {
                    continue;
                }
                if idx < o.last_index && o.last_index != u32::MAX {
                    o.run.out_of_order += 1;
                }
                o.last_index = o.last_index.max(idx);
                o.run.trades.push(RunTrade {
                    index: idx,
                    is_create: false,
                    ..*t
                });
            }
        }
    }

    /// The stream reached `slot`: runs of earlier slots are complete.
    pub fn on_slot(&mut self, slot: u64, now_ns: u64) {
        let mut i = 0;
        while i < self.open.len() {
            if self.open[i].run.slot < slot {
                let o = self.open.remove(i);
                self.close(o, CloseReason::SlotEnded, now_ns);
            } else {
                i += 1;
            }
        }
    }

    /// Close runs older than the timeout.
    pub fn on_tick(&mut self, now_ns: u64) {
        let t = self.timeout_ns;
        let mut i = 0;
        while i < self.open.len() {
            if now_ns.saturating_sub(self.open[i].run.create_ns) > t {
                let o = self.open.remove(i);
                self.close(o, CloseReason::Timeout, now_ns);
            } else {
                i += 1;
            }
        }
    }

    fn close(&mut self, mut o: Open, why: CloseReason, now_ns: u64) {
        // stable: the create transaction's own trades keep their order
        o.run.trades.sort_by_key(|t| (t.index, !t.is_create));
        o.run.closed = why;
        o.run.closed_ns = now_ns;
        if o.run.run_closed_ns == 0 {
            o.run.run_closed_ns = now_ns;
        }
        self.closed.push(o.run);
    }

    /// Hand over the closed runs.
    pub fn drain(&mut self) -> Vec<BundleRun> {
        std::mem::take(&mut self.closed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(user: u8, sol: u64) -> RunTrade {
        RunTrade {
            index: 0,
            is_create: false,
            user: Pubkey::new_from_array([user; 32]),
            is_buy: true,
            sol,
            tok: sol * 30,
            v: 30_000_000_000 + sol,
            t: 1_000_000_000_000_000,
        }
    }

    #[test]
    fn run_is_collected_ordered_and_closed_by_the_next_slot() {
        let mint = Pubkey::new_from_array([9; 32]);
        let other = Pubkey::new_from_array([8; 32]);
        let mut a = BundleAssembler::new(16, 2_000);
        a.on_create(mint, Sig([1; 64]), 100, Some(40), 1_000, &[tr(1, 500)]);
        // out of index order: 43 before 41
        a.on_trades(100, Some(43), 2_000, &[(mint, tr(3, 200))]);
        a.on_trades(
            100,
            Some(41),
            3_000,
            &[(mint, tr(2, 300)), (other, tr(7, 1))],
        );
        a.on_trades(101, Some(1), 3_500, &[(mint, tr(4, 100))]); // later slot: not slot 0
        assert_eq!(a.open_runs(), 1);
        // index 46 > 40 + 5: the bundle run is complete, later slot-0 trades still count
        a.on_trades(100, Some(46), 4_000, &[(mint, tr(5, 50))]);
        a.on_slot(101, 5_000);
        let runs = a.drain();
        assert_eq!(runs.len(), 1);
        let r = &runs[0];
        assert_eq!(r.closed, CloseReason::SlotEnded);
        assert_eq!(r.create_index, Some(40));
        let idx: Vec<u32> = r.trades.iter().map(|t| t.index).collect();
        assert_eq!(idx, vec![40, 41, 43, 46]);
        assert!(r.trades[0].is_create && !r.trades[1].is_create);
        assert_eq!(r.out_of_order, 1);
        assert_eq!((r.run_closed_ns, r.closed_ns), (4_000, 5_000));
        assert_eq!(a.open_runs(), 0);
    }

    #[test]
    fn timeout_and_eviction() {
        let mut a = BundleAssembler::new(2, 1);
        for i in 0..3u8 {
            a.on_create(
                Pubkey::new_from_array([i; 32]),
                Sig([i; 64]),
                5,
                None,
                0,
                &[],
            );
        }
        let ev = a.drain();
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].closed, CloseReason::Evicted);
        a.on_tick(500_000);
        assert!(a.drain().is_empty());
        a.on_tick(2_000_000);
        let t = a.drain();
        assert_eq!(t.len(), 2);
        assert!(t.iter().all(|r| r.closed == CloseReason::Timeout));
    }
}
