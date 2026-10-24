//! The risk gate's day across restarts (docs/08 sections 9 and 10).
//!
//! * [`day_seed`]: what today already spent, recomputed from the ledger at
//!   start: failed real attempts' `fills.fail_cost`, the P&L of real
//!   positions whose buy was sent today and are closed, the execution those
//!   positions paid beyond it (the filled buy's `fills.fee` + `fills.tip`,
//!   each filled sell's priority + tip + base fee from `sends`), today's real buys
//!   (all, this hour's, per operator), the operators' persisted
//!   consecutive-loss latches (`op_risk`) and the persisted `risk` row of
//!   the day (the gate's own running total, written by the cold thread).
//!   "Today" is the UTC day of the buy's first send (`sends.sent_unix_ms`).
//! * [`note_start`] / [`note_clean_shutdown`]: `kv.run_open` is set at every
//!   start and removed by a clean shutdown; a start that finds it set
//!   follows an unclean exit (crash, SIGKILL, watchdog, panic) and is
//!   appended to `kv.unclean_starts`. The caller latches the kill when too
//!   many fall within the window.

use rusqlite::{params, Connection, OptionalExtension};

/// What today already spent (see the module docs).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DaySeed {
    /// Sum of `fills.fail_cost` of today's real attempts.
    pub failed_cost: u64,
    /// Closed real positions bought today: exits' proceeds minus cost.
    pub closed_pnl: i64,
    /// Today's real buys that filled and their filled sells: what the
    /// transactions cost beyond the curve P&L (fees and tips).
    pub tx_cost: u64,
    /// Real buys sent today / this hour.
    pub real_buys: u64,
    pub real_buys_hour: u64,
    /// `(op_id, real buys today)`.
    pub op_buys: Vec<(u32, u64)>,
    /// `(op_id, consec_losses, disarmed_until unix)` from `op_risk`.
    pub op_risk: Vec<(u32, u32, u64)>,
    /// `risk.realized` of today's row, if one was written.
    pub persisted_realized: Option<i64>,
}

impl DaySeed {
    /// Today's realized P&L to seed the gate with: the more conservative of
    /// the recomputation and the persisted running total.
    pub fn realized(&self) -> i64 {
        let computed = self
            .closed_pnl
            .saturating_sub(self.failed_cost.min(i64::MAX as u64) as i64)
            .saturating_sub(self.tx_cost.min(i64::MAX as u64) as i64);
        match self.persisted_realized {
            Some(p) => p.min(computed),
            None => computed,
        }
    }
}

const REAL: &str = "i.mode IN ('probe','live')";

/// Recompute today's spend (`day_start_ms` / `hour_start_ms`: unix ms of
/// the UTC day / hour start, `day_key` = `risk.day` of today).
pub fn day_seed(
    c: &Connection,
    day_start_ms: u64,
    hour_start_ms: u64,
    day_key: &str,
) -> rusqlite::Result<DaySeed> {
    // first buy send of each real intent
    let first = format!(
        "SELECT i.intent_id AS intent_id, i.op_id AS op_id, MIN(s.sent_unix_ms) AS t FROM intents i \
         JOIN sends s ON s.intent_id = i.intent_id \
         WHERE {REAL} AND (s.kind IS NULL OR s.kind = 'buy') AND s.sent_unix_ms IS NOT NULL \
         GROUP BY i.intent_id"
    );
    let failed_cost: i64 = c.query_row(
        &format!(
            "SELECT COALESCE(SUM(f.fail_cost), 0) FROM fills f JOIN ({first}) b ON b.intent_id = f.intent_id \
             WHERE b.t >= ?1 AND f.fail_cost > 0"
        ),
        params![day_start_ms as i64],
        |r| r.get(0),
    )?;
    let closed_pnl: i64 = c.query_row(
        &format!(
            "SELECT COALESCE(SUM(COALESCE((SELECT SUM(e.sol_delta) FROM exits e WHERE e.mint = p.mint), 0) - COALESCE(p.cost, 0)), 0) \
             FROM positions p JOIN ({first}) b ON b.intent_id = p.intent_id \
             WHERE b.t >= ?1 AND p.state = 'closed'"
        ),
        params![day_start_ms as i64],
        |r| r.get(0),
    )?;
    // a sell's base fee (one signature) is not in `sends`
    let tx_cost: i64 = c.query_row(
        &format!(
            "SELECT COALESCE((SELECT SUM(COALESCE(f.fee, 0) + COALESCE(f.tip, 0)) FROM fills f \
               JOIN ({first}) b ON b.intent_id = f.intent_id WHERE b.t >= ?1 AND f.fail_reason IS NULL), 0) \
             + COALESCE((SELECT SUM(COALESCE(s.prio, 0) + COALESCE(s.tip, 0) + 5000) FROM exits e \
               JOIN sends s ON s.sig = e.sig JOIN positions p ON p.mint = e.mint \
               JOIN ({first}) b ON b.intent_id = p.intent_id WHERE b.t >= ?1), 0)"
        ),
        params![day_start_ms as i64],
        |r| r.get(0),
    )?;
    let count = |since: u64| -> rusqlite::Result<i64> {
        c.query_row(
            &format!("SELECT COUNT(*) FROM ({first}) b WHERE b.t >= ?1"),
            params![since as i64],
            |r| r.get(0),
        )
    };
    let real_buys = count(day_start_ms)? as u64;
    let real_buys_hour = count(hour_start_ms)? as u64;
    let mut op_buys = vec![];
    {
        let mut st = c.prepare(&format!(
            "SELECT b.op_id, COUNT(*) FROM ({first}) b WHERE b.t >= ?1 AND b.op_id IS NOT NULL GROUP BY b.op_id"
        ))?;
        let rows = st.query_map(params![day_start_ms as i64], |r| {
            Ok((r.get::<_, i64>(0)? as u32, r.get::<_, i64>(1)? as u64))
        })?;
        for r in rows {
            op_buys.push(r?);
        }
    }
    let mut op_risk = vec![];
    {
        let mut st = c.prepare(
            "SELECT op_id, COALESCE(consec_losses, 0), COALESCE(disarmed_until, 0) FROM op_risk",
        )?;
        let rows = st.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)? as u32,
                r.get::<_, i64>(1)?.max(0) as u32,
                r.get::<_, i64>(2)?.max(0) as u64,
            ))
        })?;
        for r in rows {
            op_risk.push(r?);
        }
    }
    let persisted_realized: Option<i64> = c
        .query_row(
            "SELECT realized FROM risk WHERE day = ?1",
            params![day_key],
            |r| r.get::<_, Option<i64>>(0),
        )
        .optional()?
        .flatten();
    Ok(DaySeed {
        failed_cost: failed_cost.max(0) as u64,
        closed_pnl,
        tx_cost: tx_cost.max(0) as u64,
        real_buys,
        real_buys_hour,
        op_buys,
        op_risk,
        persisted_realized,
    })
}

/// `kv` key set while a process runs (removed by a clean shutdown).
pub const RUN_OPEN_KEY: &str = "run_open";
/// `kv` key: comma-separated unix times of starts after an unclean exit.
pub const UNCLEAN_KEY: &str = "unclean_starts";

fn kv(c: &Connection, k: &str) -> rusqlite::Result<Option<String>> {
    c.query_row("SELECT v FROM kv WHERE k = ?1", params![k], |r| r.get(0))
        .optional()
}

/// Record a start. Returns the number of unclean starts within the last
/// `window_s` seconds, this one included when the previous run did not
/// shut down cleanly.
pub fn note_start(c: &Connection, now_unix: u64, window_s: u64) -> rusqlite::Result<usize> {
    let unclean_now = kv(c, RUN_OPEN_KEY)?.is_some();
    let mut list: Vec<u64> = kv(c, UNCLEAN_KEY)?
        .unwrap_or_default()
        .split(',')
        .filter_map(|t| t.trim().parse().ok())
        .filter(|t: &u64| now_unix.saturating_sub(*t) < window_s)
        .collect();
    if unclean_now {
        list.push(now_unix);
    }
    let text = list
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    c.execute(
        "INSERT OR REPLACE INTO kv(k, v) VALUES(?1, ?2)",
        params![UNCLEAN_KEY, text],
    )?;
    c.execute(
        "INSERT OR REPLACE INTO kv(k, v) VALUES(?1, ?2)",
        params![RUN_OPEN_KEY, now_unix.to_string()],
    )?;
    Ok(list.len())
}

/// A clean shutdown: the next start is not counted as unclean.
pub fn note_clean_shutdown(c: &Connection) -> rusqlite::Result<()> {
    c.execute("DELETE FROM kv WHERE k = ?1", params![RUN_OPEN_KEY])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(crate::schema::DDL).unwrap();
        c
    }

    #[test]
    fn unclean_starts_are_counted_within_the_window() {
        let c = db();
        assert_eq!(note_start(&c, 1_000, 3_600).unwrap(), 0);
        note_clean_shutdown(&c).unwrap();
        assert_eq!(note_start(&c, 1_100, 3_600).unwrap(), 0);
        // no clean shutdown: each following start is unclean
        assert_eq!(note_start(&c, 1_200, 3_600).unwrap(), 1);
        assert_eq!(note_start(&c, 1_300, 3_600).unwrap(), 2);
        assert_eq!(note_start(&c, 1_400, 3_600).unwrap(), 3);
        // an hour later the old ones fall out
        assert_eq!(note_start(&c, 1_250 + 3_600, 3_600).unwrap(), 3);
        assert_eq!(note_start(&c, 9_000, 3_600).unwrap(), 1);
    }

    #[test]
    fn today_is_recomputed_from_the_ledger() {
        let c = db();
        let day0 = 1_700_006_400_000u64; // a UTC midnight, ms
        c.execute_batch(&format!(
            "INSERT INTO intents(intent_id, mint, op_id, mode, state) VALUES(1,'m1',7,'probe','failed'),(2,'m2',7,'probe','landed'),(3,'m3',7,'probe','failed'),(4,'m4',7,'paper','paper');
             INSERT INTO sends(sig, intent_id, sent_unix_ms, kind) VALUES('s1',1,{t1},'buy'),('s2',2,{t1},'buy'),('s3',3,{y},'buy'),('s4',4,{t1},'buy');
             INSERT INTO fills(sig, intent_id, fail_cost, fail_reason) VALUES('f1',1,600000,'slippage'),('f3',3,900000,'slippage'),('f4',4,5000000,'slippage');
             INSERT INTO fills(sig, intent_id, fee, tip) VALUES('s2',2,15000,6000000);
             INSERT INTO sends(sig, intent_id, prio, tip, sent_unix_ms, kind) VALUES('e2',2,10000,3400000,{t1},'sell'),('e2b',2,10000,3400000,{t1},'sell');
             INSERT INTO positions(mint, intent_id, cost, state) VALUES('m2',2,20000000,'closed');
             INSERT INTO exits(sig, mint, sol_delta) VALUES('e2','m2',15000000);
             INSERT INTO risk(day, realized) VALUES('2023-11-15', -9000000);",
            t1 = day0 + 3_600_000,
            y = day0 - 1_000
        ))
        .unwrap();
        let s = day_seed(&c, day0, day0 + 3_600_000, "2023-11-15").unwrap();
        assert_eq!(
            s.failed_cost, 600_000,
            "yesterday's and paper costs excluded"
        );
        assert_eq!(s.closed_pnl, -5_000_000);
        assert_eq!(
            s.tx_cost,
            15_000 + 6_000_000 + 10_000 + 3_400_000 + 5_000,
            "the filled buy's fee + tip and the landed sell variant only"
        );
        assert_eq!((s.real_buys, s.real_buys_hour), (2, 2));
        assert_eq!(s.op_buys, vec![(7, 2)]);
        assert_eq!(
            s.realized(),
            -5_000_000 - 600_000 - 9_430_000,
            "the recomputation, execution included, is lower"
        );
        c.execute("UPDATE risk SET realized = -20000000", [])
            .unwrap();
        let s = day_seed(&c, day0, day0 + 3_600_000, "2023-11-15").unwrap();
        assert_eq!(s.realized(), -20_000_000, "the persisted total is lower");
    }
}
