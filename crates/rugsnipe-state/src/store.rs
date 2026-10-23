//! `StateStore`: SQLite in WAL mode with **one writer thread** fed by a
//! channel. Writes are batched into one transaction per drain (up to
//! [`BATCH`] statements), so the cold thread never waits on `fsync`.
//! Readers open their own connections ([`StateStore::reader`]).

use crate::rows::*;
use crate::schema;
use crate::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::mpsc::{channel, Receiver, Sender, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;

/// Statements per transaction at most.
pub const BATCH: usize = 512;

/// One write for the writer thread.
#[derive(Debug, Clone)]
pub enum Write {
    /// `INSERT OR IGNORE` (a wallet's first dump is the one kept).
    Dumper(DumperRow),
    /// `INSERT OR IGNORE` (an intent is decided once per mint and mode).
    Intent(IntentRow),
    /// Move an intent to `to` if its current state allows it.
    IntentState {
        intent_id: i64,
        to: IntentState,
    },
    Send(SendRow),
    Fill(FillRow),
    /// Upsert by intent.
    Shadow(ShadowRow),
    /// Upsert by intent.
    Filter(FilterRow),
    /// Upsert by intent (the live outcome store).
    Outcome(OutcomeRow),
    /// Upsert by mint.
    Position(PositionRow),
    Exit(ExitRow),
    /// Upsert by pubkey.
    Nonce(NonceRow),
    WatchDyn(WatchDynRow),
    WatchDynDelete(Pubkey),
    Risk(RiskRow),
    OpRisk {
        op_id: u32,
        consec_losses: u32,
        buys_today: u32,
        disarmed_until: u64,
    },
    Kv(String, String),
    KvDelete(String),
    /// Upsert by mint.
    FeedBundle(FeedBundleRow),
    /// Reply once everything before it is committed.
    Barrier(SyncSender<()>),
}

use pump_codec::Pubkey;

/// Writer counters.
#[derive(Debug, Default)]
pub struct StoreStats {
    pub written: AtomicU64,
    pub errors: AtomicU64,
    pub batches: AtomicU64,
    /// Intent state changes refused by the state machine.
    pub refused_transitions: AtomicU64,
}

/// Cloneable handle that queues writes (never blocks).
#[derive(Debug, Clone)]
pub struct StateWriter {
    tx: Sender<Write>,
    stats: Arc<StoreStats>,
}

impl StateWriter {
    /// Queue a write; `false` if the writer thread is gone.
    pub fn send(&self, w: Write) -> bool {
        self.tx.send(w).is_ok()
    }
    /// Wait until every queued write is committed.
    pub fn flush(&self) -> bool {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.send(Write::Barrier(tx)) && rx.recv().is_ok()
    }
    pub fn stats(&self) -> &StoreStats {
        &self.stats
    }
}

/// The state database.
#[derive(Debug)]
pub struct StateStore {
    path: PathBuf,
    writer: StateWriter,
    handle: Option<JoinHandle<()>>,
}

impl StateStore {
    /// Open (creating directories and tables) and start the writer thread.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(d) = path.parent() {
            if !d.as_os_str().is_empty() {
                std::fs::create_dir_all(d)?;
            }
        }
        let conn = Connection::open(&path)?;
        schema::configure(&conn)?;
        schema::migrate(&conn)?;
        let (tx, rx) = channel::<Write>();
        let stats = Arc::new(StoreStats::default());
        let st = stats.clone();
        let handle = std::thread::Builder::new()
            .name("state-writer".into())
            .spawn(move || writer_loop(conn, rx, st))?;
        Ok(Self {
            path,
            writer: StateWriter { tx, stats },
            handle: Some(handle),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A handle for queuing writes.
    pub fn writer(&self) -> StateWriter {
        self.writer.clone()
    }

    /// A new read (and lease) connection.
    pub fn reader(&self) -> Result<Connection> {
        open_conn(&self.path)
    }

    /// Commit everything queued so far.
    pub fn flush(&self) -> bool {
        self.writer.flush()
    }

    /// Flush and return the number of statements written so far. The
    /// writer thread ends once every [`StateWriter`] clone is dropped.
    pub fn close(self) -> u64 {
        self.writer.flush();
        self.writer.stats.written.load(Relaxed)
    }

    /// Writer counters.
    pub fn stats(&self) -> &StoreStats {
        &self.writer.stats
    }

    /// `true` once the writer thread has exited.
    pub fn writer_finished(&self) -> bool {
        self.handle.as_ref().is_none_or(|h| h.is_finished())
    }
}

/// Open a configured connection to an existing (or new) state.db.
pub fn open_conn(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    schema::configure(&conn)?;
    schema::migrate(&conn)?;
    Ok(conn)
}

fn writer_loop(mut conn: Connection, rx: Receiver<Write>, stats: Arc<StoreStats>) {
    let mut barriers: Vec<SyncSender<()>> = Vec::new();
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        while batch.len() < BATCH {
            match rx.try_recv() {
                Ok(w) => batch.push(w),
                Err(_) => break,
            }
        }
        let tx = match conn.transaction() {
            Ok(t) => t,
            Err(_) => {
                stats.errors.fetch_add(batch.len() as u64, Relaxed);
                continue;
            }
        };
        for w in batch {
            if let Write::Barrier(b) = w {
                barriers.push(b);
                continue;
            }
            match apply(&tx, &w) {
                Ok(true) => {
                    stats.written.fetch_add(1, Relaxed);
                }
                Ok(false) => {
                    stats.refused_transitions.fetch_add(1, Relaxed);
                }
                Err(_) => {
                    stats.errors.fetch_add(1, Relaxed);
                }
            }
        }
        if tx.commit().is_err() {
            stats.errors.fetch_add(1, Relaxed);
        }
        stats.batches.fetch_add(1, Relaxed);
        for b in barriers.drain(..) {
            let _ = b.send(());
        }
    }
}

fn opt_i64(v: Option<u64>) -> Option<i64> {
    v.map(|x| x as i64)
}

/// Apply one write. `Ok(false)` = refused by the intent state machine.
pub fn apply(c: &Connection, w: &Write) -> rusqlite::Result<bool> {
    match w {
        Write::Intent(r) => {
            c.execute(
                "INSERT OR IGNORE INTO intents(intent_id, mint, op_id, box, mode, armset_ver, slot, bank_id, via, feed_class,
                   spend, v_max, min_tokens_out, nonce_idx, nonce_value, decided_ns, state, leader_class)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
                params![
                    r.intent_id,
                    r.mint.to_string(),
                    r.op_id,
                    r.box_id,
                    r.mode,
                    r.armset_ver as i64,
                    r.slot as i64,
                    r.bank_id as i64,
                    r.via.as_str(),
                    r.feed_class,
                    r.spend as i64,
                    r.v_max as i64,
                    r.min_tokens_out as i64,
                    r.nonce_idx,
                    b58(&r.nonce_value),
                    r.decided_ns as i64,
                    r.state.as_str(),
                    r.leader_class
                ],
            )?;
        }
        Write::IntentState { intent_id, to } => {
            let from = IntentState::allowed_from(*to);
            if from.is_empty() {
                return Ok(false);
            }
            let list = from
                .iter()
                .map(|s| format!("'{}'", s.as_str()))
                .collect::<Vec<_>>()
                .join(",");
            let n = c.execute(
                &format!(
                    "UPDATE intents SET state = ?1 WHERE intent_id = ?2 AND state IN ({list})"
                ),
                params![to.as_str(), intent_id],
            )?;
            return Ok(n == 1);
        }
        Write::Send(r) => {
            c.execute(
                "INSERT OR REPLACE INTO sends(sig, intent_id, variant, sender, prio, tip, sent_ns, result, bid, sent_unix_ms,
                   kind, err, leader_class) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                params![
                    b58(&r.sig),
                    r.intent_id,
                    r.variant,
                    r.sender,
                    r.prio as i64,
                    r.tip as i64,
                    r.sent_ns as i64,
                    r.result,
                    r.bid as i64,
                    r.sent_unix_ms as i64,
                    r.kind,
                    r.err,
                    r.leader_class
                ],
            )?;
        }
        Write::Fill(r) => {
            c.execute(
                "INSERT OR REPLACE INTO fills(sig, intent_id, mint, slot, tx_index, tokens, sol_delta, err, buyers_ahead,
                   rival_sol_ahead, late, fail_reason, fail_cost, fee, variant, sender, tip, leader_class)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
                params![
                    b58(&r.sig),
                    r.intent_id,
                    r.mint.to_string(),
                    r.slot as i64,
                    r.tx_index,
                    r.tokens as i64,
                    r.sol_delta,
                    r.err,
                    r.buyers_ahead,
                    r.rival_sol_ahead as i64,
                    r.late as i64,
                    r.fail_reason,
                    r.fail_cost as i64,
                    r.fee.map(|x| x as i64),
                    r.variant,
                    r.sender,
                    r.tip.map(|x| x as i64),
                    r.leader_class
                ],
            )?;
        }
        Write::Shadow(r) => {
            c.execute(
                "INSERT OR REPLACE INTO shadow(intent_id, mint, model, outcome, create_slot, create_index, block_txs,
                   bundle_trades, bundle_wallets, our_index, tp_ahead, tp_behind, s0_failed, premium_bps, post_bundle_v,
                   post_bundle_t, post_bundle_mcap, cost)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
                params![
                    r.intent_id,
                    r.mint.to_string(),
                    r.model,
                    r.outcome,
                    r.create_slot as i64,
                    r.create_index,
                    r.block_txs,
                    r.bundle_trades,
                    r.bundle_wallets,
                    r.our_index,
                    r.tp_ahead,
                    r.tp_behind,
                    r.s0_failed,
                    r.premium_bps,
                    r.post_bundle_v as i64,
                    r.post_bundle_t as i64,
                    r.post_bundle_mcap as i64,
                    r.cost as i64
                ],
            )?;
        }
        Write::Filter(r) => {
            c.execute(
                "INSERT OR REPLACE INTO filter(intent_id, mint, model, score, tertile, mode, skipped, op_prior, op_lead_n,
                   op_lead_hit, op_lead_ev, bw_lead_n, bw_lead_ev, features)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                params![
                    r.intent_id,
                    r.mint.to_string(),
                    r.model,
                    r.score,
                    r.tertile,
                    r.mode,
                    r.skipped as i64,
                    r.op_prior,
                    r.op_lead_n,
                    r.op_lead_hit,
                    r.op_lead_ev,
                    r.bw_lead_n,
                    r.bw_lead_ev,
                    r.features
                ],
            )?;
        }
        Write::Outcome(r) => {
            let bw: Vec<String> = r.bundle_wallets.iter().map(|k| k.to_string()).collect();
            c.execute(
                "INSERT OR REPLACE INTO outcomes(intent_id, mint, signer, launch_ms, outcome, lead, ret, tp_hit, s0_third,
                   bundle_wallets, written_ms)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                params![
                    r.intent_id,
                    r.mint.to_string(),
                    r.signer.to_string(),
                    r.launch_ms as i64,
                    r.outcome,
                    r.lead as i64,
                    r.ret,
                    r.tp_hit as i64,
                    r.s0_third,
                    bw.join(","),
                    r.written_ms as i64
                ],
            )?;
        }
        Write::Position(r) => {
            c.execute(
                "INSERT INTO positions(mint, intent_id, opened_slot, tokens, cost, v_fill, peak_v, bundle_json, policy_json,
                   origin, state, sell_spend, closed_slot) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
                 ON CONFLICT(mint) DO UPDATE SET intent_id=excluded.intent_id, opened_slot=excluded.opened_slot,
                   tokens=excluded.tokens, cost=excluded.cost, v_fill=excluded.v_fill, peak_v=excluded.peak_v,
                   bundle_json=excluded.bundle_json, policy_json=excluded.policy_json, origin=excluded.origin,
                   state=excluded.state, sell_spend=excluded.sell_spend, closed_slot=excluded.closed_slot",
                params![
                    r.mint.to_string(),
                    r.intent_id,
                    r.opened_slot as i64,
                    r.tokens as i64,
                    r.cost as i64,
                    r.v_fill as i64,
                    r.peak_v as i64,
                    r.bundle_json,
                    r.policy_json,
                    r.origin.as_str(),
                    r.state.as_str(),
                    r.sell_spend as i64,
                    opt_i64(r.closed_slot)
                ],
            )?;
        }
        Write::Exit(r) => {
            c.execute(
                "INSERT OR REPLACE INTO exits(sig, mint, reason, venue, tokens, sol_delta, slot, owner, try_no)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    b58(&r.sig),
                    r.mint.to_string(),
                    r.reason,
                    r.venue,
                    r.tokens as i64,
                    r.sol_delta,
                    r.slot as i64,
                    r.owner.to_string(),
                    r.try_no
                ],
            )?;
        }
        Write::Nonce(r) => {
            c.execute(
                "INSERT INTO nonces(pubkey, pool, idx, value, last_change_slot, state, intent_id, since_slot)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
                 ON CONFLICT(pubkey) DO UPDATE SET pool=excluded.pool, idx=excluded.idx, value=excluded.value,
                   last_change_slot=excluded.last_change_slot, state=excluded.state, intent_id=excluded.intent_id,
                   since_slot=excluded.since_slot",
                params![
                    r.pubkey.to_string(),
                    r.pool,
                    r.idx,
                    b58(&r.value),
                    opt_i64(r.last_change_slot),
                    r.state.as_str(),
                    r.intent_id,
                    opt_i64(r.since_slot)
                ],
            )?;
        }
        Write::WatchDyn(r) => {
            c.execute(
                "INSERT OR REPLACE INTO watch_dyn(key, kind, op_id, rule_id, added_slot, expires_unix, remove_if_no_launch_unix,
                   keep, parent, state) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![
                    r.key.to_string(),
                    r.kind,
                    r.op_id,
                    r.rule_id,
                    r.added_slot as i64,
                    r.expires_unix as i64,
                    r.remove_if_no_launch_unix as i64,
                    r.keep as i64,
                    r.parent.map(|p| p.to_string()),
                    r.state
                ],
            )?;
        }
        Write::WatchDynDelete(k) => {
            c.execute(
                "DELETE FROM watch_dyn WHERE key = ?1",
                params![k.to_string()],
            )?;
        }
        Write::Risk(r) => {
            c.execute(
                "INSERT OR REPLACE INTO risk(day, start_balance, deposits, sweeps, realized, buys, killed, kill_reason)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    r.day,
                    r.start_balance as i64,
                    r.deposits as i64,
                    r.sweeps as i64,
                    r.realized,
                    r.buys as i64,
                    r.killed as i64,
                    r.kill_reason
                ],
            )?;
        }
        Write::OpRisk {
            op_id,
            consec_losses,
            buys_today,
            disarmed_until,
        } => {
            c.execute(
                "INSERT OR REPLACE INTO op_risk(op_id, consec_losses, buys_today, disarmed_until) VALUES(?1,?2,?3,?4)",
                params![op_id, consec_losses, buys_today, *disarmed_until as i64],
            )?;
        }
        Write::Kv(k, v) => {
            c.execute(
                "INSERT OR REPLACE INTO kv(k, v) VALUES(?1, ?2)",
                params![k, v],
            )?;
        }
        Write::KvDelete(k) => {
            c.execute("DELETE FROM kv WHERE k = ?1", params![k])?;
        }
        Write::Dumper(r) => {
            c.execute(
                "INSERT OR IGNORE INTO dumpers(wallet, mint, slot, sold_bps, written_ms) VALUES(?1,?2,?3,?4,?5)",
                params![
                    r.wallet.to_string(),
                    r.mint.to_string(),
                    r.slot as i64,
                    r.sold_bps as i64,
                    r.written_ms as i64
                ],
            )?;
        }
        Write::FeedBundle(r) => {
            c.execute(
                "INSERT OR REPLACE INTO feed_bundle(mint, intent_id, slot, create_index, trades, bundle_wallets, s0_third,
                   dev_sol, bundle_sol, me_sol, closed, run_close_ms, out_of_order, features)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                params![
                    r.mint.to_string(),
                    r.intent_id,
                    r.slot as i64,
                    r.create_index,
                    r.trades,
                    r.bundle_wallets,
                    r.s0_third,
                    r.dev_sol,
                    r.bundle_sol,
                    r.me_sol,
                    r.closed,
                    r.run_close_ms,
                    r.out_of_order,
                    r.features
                ],
            )?;
        }
        Write::Barrier(_) => {}
    }
    Ok(true)
}

/// Read one `kv` value.
pub fn kv_get(c: &Connection, k: &str) -> rusqlite::Result<Option<String>> {
    c.query_row("SELECT v FROM kv WHERE k = ?1", params![k], |r| r.get(0))
        .optional()
}

/// Every row of the live outcome store (`outcomes`), oldest launch first
/// (reloaded at start; rows with an unparsable key are skipped).
pub fn load_outcomes(c: &Connection) -> rusqlite::Result<Vec<OutcomeRow>> {
    let mut st = c.prepare(
        "SELECT intent_id, mint, signer, launch_ms, outcome, lead, ret, tp_hit, s0_third, bundle_wallets, written_ms
         FROM outcomes ORDER BY launch_ms, intent_id",
    )?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, i64>(5)?,
            r.get::<_, f64>(6)?,
            r.get::<_, i64>(7)?,
            r.get::<_, Option<u32>>(8)?,
            r.get::<_, Option<String>>(9)?,
            r.get::<_, i64>(10)?,
        ))
    })?;
    let mut out = vec![];
    for row in rows {
        let (id, mint, signer, launch_ms, outcome, lead, ret, tp, s0, bw, written) = row?;
        let (Ok(mint), Ok(signer)) = (mint.parse::<Pubkey>(), signer.parse::<Pubkey>()) else {
            continue;
        };
        out.push(OutcomeRow {
            intent_id: id,
            mint,
            signer,
            launch_ms: launch_ms as u64,
            outcome,
            lead: lead != 0,
            ret,
            tp_hit: tp != 0,
            s0_third: s0,
            bundle_wallets: bw
                .unwrap_or_default()
                .split(',')
                .filter_map(|k| k.parse().ok())
                .collect(),
            written_ms: written as u64,
        });
    }
    Ok(out)
}
