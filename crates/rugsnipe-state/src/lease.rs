//! The `exits` lease (docs/06 section 8 "Guard"): the hot box's exit loop
//! renews `lease('exits')` while it advances; if the heartbeat is older than
//! `takeover_after_ms` and a position is open, the guard takes over with a
//! compare-and-set on `epoch` (`epoch + 1`). A holder whose renew fails has
//! lost the lease and must stop selling.
//!
//! Each call is one SQLite statement on the caller's own connection (hot
//! and guard are different processes sharing `state.db` in WAL mode).

use rusqlite::{params, Connection, OptionalExtension};

/// One lease row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseRow {
    pub name: String,
    pub owner: String,
    pub hb_unix_ms: u64,
    pub epoch: u64,
}

impl LeaseRow {
    pub fn age_ms(&self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(self.hb_unix_ms)
    }
}

/// Read a lease.
pub fn read(c: &Connection, name: &str) -> rusqlite::Result<Option<LeaseRow>> {
    c.query_row(
        "SELECT name, owner, hb_unix_ms, epoch FROM lease WHERE name = ?1",
        params![name],
        |r| {
            Ok(LeaseRow {
                name: r.get(0)?,
                owner: r.get(1)?,
                hb_unix_ms: r.get::<_, i64>(2)? as u64,
                epoch: r.get::<_, i64>(3)? as u64,
            })
        },
    )
    .optional()
}

/// Name of the hot process's liveness row (not a lease: never taken over).
/// The hot box writes it whenever its exit loop advances, whether or not it
/// holds `exits`; a guard holding `exits` hands the lease back once this
/// heartbeat is fresh again (the hot box came back).
pub const HOT_HEARTBEAT: &str = "hot_alive";

/// Write a liveness heartbeat row (`epoch` 0, owner = the writer).
pub fn beat(c: &Connection, name: &str, owner: &str, now_ms: u64) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO lease(name, owner, hb_unix_ms, epoch) VALUES(?1, ?2, ?3, 0)
         ON CONFLICT(name) DO UPDATE SET owner = excluded.owner, hb_unix_ms = excluded.hb_unix_ms",
        params![name, owner, now_ms as i64],
    )?;
    Ok(())
}

/// A lease held (or wanted) by `owner`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    pub name: String,
    pub owner: String,
    /// Epoch we hold (0 = not held).
    pub epoch: u64,
}

/// Result of [`Lease::acquire`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Acquire {
    /// We hold it at this epoch (new, renewed or taken over).
    Held {
        epoch: u64,
        took_over_from: Option<String>,
    },
    /// Someone else holds a fresh lease.
    Busy(LeaseRow),
}

impl Lease {
    pub fn new(name: &str, owner: &str) -> Self {
        Self {
            name: name.into(),
            owner: owner.into(),
            epoch: 0,
        }
    }

    pub fn is_held(&self) -> bool {
        self.epoch > 0
    }

    /// Take the lease if it is free, ours, or older than `stale_after_ms`
    /// (compare-and-set on the epoch read).
    pub fn acquire(
        &mut self,
        c: &Connection,
        now_ms: u64,
        stale_after_ms: u64,
    ) -> rusqlite::Result<Acquire> {
        match read(c, &self.name)? {
            None => {
                let n = c.execute(
                    "INSERT OR IGNORE INTO lease(name, owner, hb_unix_ms, epoch) VALUES(?1, ?2, ?3, 1)",
                    params![self.name, self.owner, now_ms as i64],
                )?;
                if n == 1 {
                    self.epoch = 1;
                    return Ok(Acquire::Held {
                        epoch: 1,
                        took_over_from: None,
                    });
                }
                // lost an insert race: retry once against the new row
                self.acquire(c, now_ms, stale_after_ms)
            }
            Some(row) if row.owner == self.owner => {
                let n = c.execute(
                    "UPDATE lease SET hb_unix_ms = ?1 WHERE name = ?2 AND owner = ?3 AND epoch = ?4",
                    params![now_ms as i64, self.name, self.owner, row.epoch as i64],
                )?;
                if n == 1 {
                    self.epoch = row.epoch;
                    Ok(Acquire::Held {
                        epoch: row.epoch,
                        took_over_from: None,
                    })
                } else {
                    self.epoch = 0;
                    Ok(Acquire::Busy(row))
                }
            }
            Some(row) if row.age_ms(now_ms) > stale_after_ms => {
                let n = c.execute(
                    "UPDATE lease SET owner = ?1, hb_unix_ms = ?2, epoch = epoch + 1 WHERE name = ?3 AND epoch = ?4",
                    params![self.owner, now_ms as i64, self.name, row.epoch as i64],
                )?;
                if n == 1 {
                    self.epoch = row.epoch + 1;
                    Ok(Acquire::Held {
                        epoch: self.epoch,
                        took_over_from: Some(row.owner),
                    })
                } else {
                    self.epoch = 0;
                    Ok(Acquire::Busy(read(c, &self.name)?.unwrap_or(row)))
                }
            }
            Some(row) => {
                self.epoch = 0;
                Ok(Acquire::Busy(row))
            }
        }
    }

    /// Heartbeat. `false` = we lost the lease (someone took over).
    pub fn renew(&mut self, c: &Connection, now_ms: u64) -> rusqlite::Result<bool> {
        if self.epoch == 0 {
            return Ok(false);
        }
        let n = c.execute(
            "UPDATE lease SET hb_unix_ms = ?1 WHERE name = ?2 AND owner = ?3 AND epoch = ?4",
            params![now_ms as i64, self.name, self.owner, self.epoch as i64],
        )?;
        if n != 1 {
            self.epoch = 0;
        }
        Ok(n == 1)
    }

    /// Give the lease up (heartbeat to 0 so the next acquirer takes it at once).
    pub fn release(&mut self, c: &Connection) -> rusqlite::Result<()> {
        if self.epoch > 0 {
            c.execute(
                "UPDATE lease SET hb_unix_ms = 0 WHERE name = ?1 AND owner = ?2 AND epoch = ?3",
                params![self.name, self.owner, self.epoch as i64],
            )?;
        }
        self.epoch = 0;
        Ok(())
    }
}
