//! Shutdown: a real sell already sent when the process stops is booked
//! before the ledger closes (2026-10-23, twice: `systemctl restart` between
//! a max_hold sell and its fill left the position open with no `exits` row,
//! and the guard closed it as a -100% trade).
//!
//! The hot loop knows which positions have sell orders out
//! ([`crate::hot::HotLoop::inflight_sells`]). Their signatures are the
//! `sends` rows (`kind = 'sell'`) the cold thread wrote from the sender
//! thread's reports (one signed message per sell job, the same signature at
//! every provider) and flushed before it returned. Each unbooked one is
//! resolved with [`ChainRead::tx_sol_delta`] (a landed sale raises the
//! wallet's lamports) on a worker thread, a few tries apart, and what is
//! found within the budget is written: the `exits` row (the order's reason,
//! the owner's lamport delta fee included, the sale's slot) and the position
//! closed at that slot, or resized when the wallet still holds some.
//! Anything not found is left open: the guard (or the next start) reads the
//! chain.

use pump_codec::Pubkey;
use rugsnipe_state::chain::ChainRead;
use rugsnipe_state::recovery::load_open_positions;
use rugsnipe_state::rows::{b58, ExitRow, PosState};
use rugsnipe_state::Ledger;
use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Sell signatures of one position looked up at most (newest first).
pub const MAX_SIGS: usize = 8;

/// A real position with sell orders out when the hot loop stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InflightSell {
    pub mint: Pubkey,
    /// Exit reason label of the oldest order out (`max_hold`, `tp0`, ...).
    pub reason: String,
    pub venue: &'static str,
    /// Tokens of every order out (at most the position's).
    pub tokens: u64,
    /// The orders out sell the whole position.
    pub closes: bool,
    pub try_no: u32,
}

/// How hard the shutdown looks.
#[derive(Debug, Clone, Copy)]
pub struct BookCfg {
    /// Rounds over the unresolved signatures.
    pub tries: u32,
    /// Pause between rounds.
    pub gap: Duration,
    /// Wall-clock bound of the whole lookup.
    pub budget: Duration,
}

impl Default for BookCfg {
    fn default() -> Self {
        Self {
            tries: 3,
            gap: Duration::from_millis(400),
            budget: Duration::from_secs(2),
        }
    }
}

/// What the shutdown did with one in-flight sell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Booked {
    /// Landed: the `exits` row is written; `closed` = the position too.
    Sale {
        mint: Pubkey,
        sig: [u8; 64],
        sol_delta: i64,
        slot: u64,
        closed: bool,
    },
    /// Sent (`sigs` signatures) but not seen on chain within the budget.
    NotFound { mint: Pubkey, sigs: usize },
}

impl core::fmt::Display for Booked {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Sale {
                mint,
                sig,
                sol_delta,
                slot,
                closed,
            } => write!(
                f,
                "shutdown: sell of {mint} landed ({} slot {slot}): booked {sol_delta} lamports, position {}",
                b58(sig),
                if *closed { "closed" } else { "still open (the wallet holds some)" }
            ),
            Self::NotFound { mint, sigs } => write!(
                f,
                "shutdown: sell of {mint} sent ({sigs} signatures) but not seen on chain in time: left open for the guard"
            ),
        }
    }
}

/// The sell signatures of an intent with no `exits` row yet, newest first.
pub fn sell_sigs(c: &Connection, intent_id: i64) -> rusqlite::Result<Vec<[u8; 64]>> {
    let mut st = c.prepare(
        "SELECT s.sig FROM sends s WHERE s.intent_id = ?1 AND s.kind = 'sell'
           AND NOT EXISTS (SELECT 1 FROM exits e WHERE e.sig = s.sig)
         ORDER BY s.sent_unix_ms DESC LIMIT ?2",
    )?;
    let rows = st.query_map(rusqlite::params![intent_id, MAX_SIGS as i64], |r| {
        r.get::<_, String>(0)
    })?;
    let mut out = vec![];
    for s in rows {
        if let Ok(v) = bs58::decode(s?).into_vec() {
            if let Ok(sig) = <[u8; 64]>::try_from(v) {
                out.push(sig);
            }
        }
    }
    Ok(out)
}

enum Found {
    Sale(usize, [u8; 64], i64, u64),
    Balances(HashMap<Pubkey, u64>),
}

/// Resolve and book the in-flight sells (see the module docs); bounded by
/// `cfg.budget`. Writes go through `ledger` and are flushed before return.
pub fn book_inflight_sells(
    c: &Connection,
    ledger: &Ledger,
    chain: Arc<dyn ChainRead + Send + Sync>,
    owner: Pubkey,
    sells: &[InflightSell],
    cfg: BookCfg,
) -> Vec<Booked> {
    let t0 = Instant::now();
    let open: HashMap<Pubkey, _> = match load_open_positions(c) {
        Ok(v) => v.into_iter().map(|p| (p.mint, p)).collect(),
        Err(e) => {
            eprintln!("shutdown: open positions: {e}");
            return vec![];
        }
    };
    // (sell, its position, its unbooked signatures); a closed position
    // (the guard took it) or an order never sent has nothing to resolve
    let jobs: Vec<(
        &InflightSell,
        &rugsnipe_state::rows::PositionRow,
        Vec<[u8; 64]>,
    )> = sells
        .iter()
        .filter_map(|s| {
            let p = open.get(&s.mint)?;
            let sigs = sell_sigs(c, p.intent_id).unwrap_or_default();
            (!sigs.is_empty()).then_some((s, p, sigs))
        })
        .collect();
    if jobs.is_empty() {
        return vec![];
    }
    let work: Vec<(usize, Vec<[u8; 64]>)> = jobs
        .iter()
        .enumerate()
        .map(|(k, (_, _, sigs))| (k, sigs.clone()))
        .collect();
    let (tx, rx) = mpsc::channel::<Found>();
    let spawned = std::thread::Builder::new()
        .name("shutdown-book".into())
        .spawn(move || {
            let mut left = work;
            let mut any = false;
            for round in 0..cfg.tries {
                left.retain(|(k, sigs)| {
                    for sig in sigs {
                        if let Ok(Some((d, slot))) = chain.tx_sol_delta(sig, &owner) {
                            // a sale raises the wallet's lamports (a failed
                            // try only costs its fee)
                            if d > 0 {
                                any = true;
                                let _ = tx.send(Found::Sale(*k, *sig, d, slot));
                                return false;
                            }
                        }
                    }
                    true
                });
                if left.is_empty() || round + 1 >= cfg.tries {
                    break;
                }
                std::thread::sleep(cfg.gap);
            }
            if any {
                // what the wallet still holds: closed or partly sold
                if let Ok(b) = chain.token_balances(&owner) {
                    let mut m: HashMap<Pubkey, u64> = HashMap::new();
                    for x in b {
                        *m.entry(x.mint).or_default() += x.amount;
                    }
                    let _ = tx.send(Found::Balances(m));
                }
            }
        });
    let mut sales: Vec<Option<([u8; 64], i64, u64)>> = vec![None; jobs.len()];
    let mut balances: Option<HashMap<Pubkey, u64>> = None;
    if spawned.is_ok() {
        while let Some(wait) = cfg.budget.checked_sub(t0.elapsed()) {
            match rx.recv_timeout(wait) {
                Ok(Found::Sale(k, sig, d, slot)) => sales[k] = Some((sig, d, slot)),
                Ok(Found::Balances(b)) => balances = Some(b),
                // done, or out of time (the worker is left behind)
                Err(_) => break,
            }
        }
    }
    let mut out = vec![];
    for ((s, p, sigs), sale) in jobs.iter().zip(sales) {
        let Some((sig, sol_delta, slot)) = sale else {
            out.push(Booked::NotFound {
                mint: s.mint,
                sigs: sigs.len(),
            });
            continue;
        };
        // the chain's balance decides when it was read; else the orders out
        let left = balances
            .as_ref()
            .map(|b| b.get(&s.mint).copied().unwrap_or(0));
        let (sold, closed) = match left {
            Some(0) => (p.tokens, true),
            Some(l) => (p.tokens.saturating_sub(l), false),
            None => (s.tokens, s.closes),
        };
        ledger.exit(ExitRow {
            sig,
            mint: s.mint,
            reason: s.reason.clone(),
            venue: s.venue.into(),
            tokens: if sold > 0 { sold } else { s.tokens },
            sol_delta,
            slot,
            owner,
            try_no: s.try_no,
        });
        let mut row = (*p).clone();
        if closed {
            row.tokens = 0;
            row.state = PosState::Closed;
            row.closed_slot = Some(slot);
            ledger.position(row);
        } else if let Some(l) = left.filter(|l| *l < p.tokens) {
            row.tokens = l;
            ledger.position(row);
        }
        out.push(Booked::Sale {
            mint: s.mint,
            sig,
            sol_delta,
            slot,
            closed,
        });
    }
    ledger.flush();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rugsnipe_state::chain::TokenBalance;
    use rugsnipe_state::rows::{Origin, PositionRow};
    use rugsnipe_state::StateStore;
    use std::sync::atomic::{AtomicU32, Ordering::Relaxed};

    /// Sales by signature, the wallet's balances, optionally a slow node.
    struct Stub {
        landed: HashMap<[u8; 64], (i64, u64)>,
        held: Vec<(Pubkey, u64)>,
        delay: Duration,
        calls: AtomicU32,
    }

    impl ChainRead for Stub {
        fn slot(&self) -> Result<u64, String> {
            Ok(1)
        }
        fn token_balances(&self, _o: &Pubkey) -> Result<Vec<TokenBalance>, String> {
            Ok(self
                .held
                .iter()
                .map(|(m, a)| TokenBalance {
                    mint: *m,
                    account: Pubkey::default(),
                    amount: *a,
                })
                .collect())
        }
        fn accounts(&self, k: &[Pubkey]) -> Result<Vec<Option<Vec<u8>>>, String> {
            Ok(vec![None; k.len()])
        }
        fn tx_sol_delta(&self, sig: &[u8; 64], _o: &Pubkey) -> Result<Option<(i64, u64)>, String> {
            self.calls.fetch_add(1, Relaxed);
            std::thread::sleep(self.delay);
            Ok(self.landed.get(sig).copied())
        }
    }

    fn pos(mint: Pubkey, intent_id: i64) -> PositionRow {
        PositionRow {
            mint,
            intent_id,
            opened_slot: 100,
            tokens: 1_000,
            cost: 250_000_000,
            v_fill: 32_000_000_000,
            peak_v: 33_000_000_000,
            bundle_json: "[]".into(),
            policy_json: "{}".into(),
            origin: Origin::Intent,
            state: PosState::Exiting,
            sell_spend: 0,
            closed_slot: None,
        }
    }

    fn sell(mint: Pubkey, tokens: u64) -> InflightSell {
        InflightSell {
            mint,
            reason: "max_hold".into(),
            venue: "curve",
            tokens,
            closes: tokens >= 1_000,
            try_no: 1,
        }
    }

    fn send(c: &Connection, sig: [u8; 64], intent: i64, kind: &str, t: i64) {
        c.execute(
            "INSERT INTO sends(sig, intent_id, kind, result, sent_unix_ms) VALUES(?1, ?2, ?3, 'accepted', ?4)",
            rusqlite::params![b58(&sig), intent, kind, t],
        )
        .unwrap();
    }

    fn setup(name: &str) -> (std::path::PathBuf, StateStore) {
        let dir = std::env::temp_dir().join(format!("rugsnipe-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let s = StateStore::open(dir.join("state.db")).unwrap();
        (dir, s)
    }

    /// The incident's shape: m1's max_hold sell landed after the stop (its
    /// first try; the re-send, looked at first, did not) and the wallet holds
    /// none: booked at the lamport delta, closed at the sale's slot. m2's
    /// sell never shows: left open. m3 sold 600 of 1000: resized to what
    /// the wallet still holds. m4 was closed by the guard meanwhile and an
    /// order of m5 was never sent: nothing to do.
    #[test]
    fn a_sell_sent_before_the_stop_is_booked_from_the_chain() {
        let (dir, s) = setup("inflight");
        let l = Ledger::new(s.writer());
        let c = s.reader().unwrap();
        let ms: Vec<Pubkey> = (0..5).map(|_| Pubkey::new_unique()).collect();
        for (i, m) in ms.iter().enumerate() {
            let mut p = pos(*m, 11 + i as i64);
            if i == 3 {
                p.state = PosState::Closed;
            }
            l.position(p);
        }
        l.flush();
        let sig = |b: u8| [b; 64];
        send(&c, sig(1), 11, "buy", 500);
        send(&c, sig(2), 11, "sell", 1_000);
        send(&c, sig(3), 11, "sell", 2_000);
        send(&c, sig(4), 12, "sell", 1_000);
        send(&c, sig(5), 13, "sell", 1_000);
        send(&c, sig(6), 14, "sell", 1_000);
        let stub = Arc::new(Stub {
            landed: [
                (sig(1), (-250_000_000, 99)),
                (sig(2), (241_600_000, 105)),
                (sig(5), (100_000_000, 106)),
                (sig(6), (1, 107)),
            ]
            .into_iter()
            .collect(),
            held: vec![(ms[2], 400)],
            delay: Duration::ZERO,
            calls: AtomicU32::new(0),
        });
        let owner = Pubkey::new_unique();
        let cfg = BookCfg {
            gap: Duration::from_millis(10),
            ..BookCfg::default()
        };
        let sells: Vec<InflightSell> = vec![
            sell(ms[0], 1_000),
            sell(ms[1], 1_000),
            sell(ms[2], 600),
            sell(ms[3], 1_000),
            sell(ms[4], 1_000),
        ];
        let got = book_inflight_sells(&c, &l, stub.clone(), owner, &sells, cfg);
        assert_eq!(
            got,
            vec![
                Booked::Sale {
                    mint: ms[0],
                    sig: sig(2),
                    sol_delta: 241_600_000,
                    slot: 105,
                    closed: true
                },
                Booked::NotFound {
                    mint: ms[1],
                    sigs: 1
                },
                Booked::Sale {
                    mint: ms[2],
                    sig: sig(5),
                    sol_delta: 100_000_000,
                    slot: 106,
                    closed: false
                },
            ]
        );
        // m1: 3 then 2; m2: 4 in three rounds; m3: 5; never the buy, m4 or m5
        assert_eq!(stub.calls.load(Relaxed), 2 + 3 + 1);
        let row = |m: &Pubkey| -> (String, i64, Option<i64>) {
            c.query_row(
                "SELECT state, tokens, closed_slot FROM positions WHERE mint = ?1",
                [m.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
        };
        assert_eq!(row(&ms[0]), ("closed".into(), 0, Some(105)));
        assert_eq!(row(&ms[1]), ("exiting".into(), 1_000, None));
        assert_eq!(row(&ms[2]), ("exiting".into(), 400, None));
        let exit = |m: &Pubkey| -> Vec<(String, String, i64, i64, i64, String, i64)> {
            let mut st = c
                .prepare("SELECT sig, reason, tokens, sol_delta, slot, owner, try_no FROM exits WHERE mint = ?1")
                .unwrap();
            st.query_map([m.to_string()], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            })
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
        };
        assert_eq!(
            exit(&ms[0]),
            vec![(
                b58(&sig(2)),
                "max_hold".into(),
                1_000,
                241_600_000,
                105,
                owner.to_string(),
                1
            )]
        );
        assert!(exit(&ms[1]).is_empty());
        assert_eq!(exit(&ms[2])[0].2, 600);
        // booked once: a second pass finds nothing left to resolve for m1
        let again = book_inflight_sells(&c, &l, stub.clone(), owner, &sells[..1], cfg);
        assert!(again.is_empty(), "{again:?}");
        drop(c);
        drop(l);
        s.close();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A node that does not answer cannot hold the shutdown past the budget:
    /// nothing is booked, the position stays for the guard.
    #[test]
    fn the_lookup_is_bounded_by_the_budget() {
        let (dir, s) = setup("inflight-slow");
        let l = Ledger::new(s.writer());
        let c = s.reader().unwrap();
        let m = Pubkey::new_unique();
        l.position(pos(m, 21));
        l.flush();
        send(&c, [9; 64], 21, "sell", 1_000);
        let stub = Arc::new(Stub {
            landed: [([9; 64], (241_600_000, 105))].into_iter().collect(),
            held: vec![],
            delay: Duration::from_secs(3),
            calls: AtomicU32::new(0),
        });
        let t0 = Instant::now();
        let got = book_inflight_sells(
            &c,
            &l,
            stub,
            Pubkey::new_unique(),
            &[sell(m, 1_000)],
            BookCfg {
                budget: Duration::from_millis(200),
                ..BookCfg::default()
            },
        );
        assert!(
            t0.elapsed() < Duration::from_millis(1_000),
            "{:?}",
            t0.elapsed()
        );
        assert_eq!(got, vec![Booked::NotFound { mint: m, sigs: 1 }]);
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM exits", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
        drop(c);
        drop(l);
        s.close();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
