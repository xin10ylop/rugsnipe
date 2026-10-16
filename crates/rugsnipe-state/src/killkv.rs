//! The kill latch in `state.db` (docs/06 section 13, docs/08 section 10).
//!
//! The latch survives a restart, a crash and a SIGKILL: when the cold thread
//! sees the risk gate latch (any reason), it writes
//!
//! * `kv.kill_latch` = `{"reason": .., "unix": .., "detail": ..}` (the first
//!   reason wins, like the gate), flushed before the next tick, and
//! * `kv.kill_trigger:<name>` = `{"unix": .., "reason": .., "detail": ..}` for
//!   every trigger that fired (the latest per trigger; the summary and the
//!   status line list them).
//!
//! At start the process re-latches from `kill_latch` before any decision
//! ([`latched`]); only a local `rugsnipe-ctl unkill --state <db>` clears it
//! ([`unkill`]: deletes `kill_latch`, records `unkill_unix`; the trigger
//! rows stay as history). Removing the kill file alone no longer unkills.

use rusqlite::{params, Connection, OptionalExtension};

/// `kv` key of the latch.
pub const LATCH_KEY: &str = "kill_latch";
/// Prefix of the per-trigger rows.
pub const TRIGGER_PREFIX: &str = "kill_trigger:";

/// A persisted latch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Latch {
    pub reason: String,
    pub unix: u64,
    pub detail: String,
}

impl Latch {
    pub fn to_json(&self) -> String {
        serde_json::json!({"reason": self.reason, "unix": self.unix, "detail": self.detail})
            .to_string()
    }

    pub fn from_json(s: &str) -> Option<Latch> {
        let v: serde_json::Value = serde_json::from_str(s).ok()?;
        Some(Latch {
            reason: v["reason"].as_str()?.to_string(),
            unix: v["unix"].as_u64().unwrap_or(0),
            detail: v["detail"].as_str().unwrap_or("").to_string(),
        })
    }
}

/// The persisted latch, if the box is killed.
pub fn latched(c: &Connection) -> rusqlite::Result<Option<Latch>> {
    let v: Option<String> = c
        .query_row("SELECT v FROM kv WHERE k = ?1", params![LATCH_KEY], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(v.and_then(|s| {
        Latch::from_json(&s).or(Some(Latch {
            reason: "unparsable".into(),
            unix: 0,
            detail: s,
        }))
    }))
}

/// Every trigger row: `(name, latch-shaped record)`, by name.
pub fn triggers(c: &Connection) -> rusqlite::Result<Vec<(String, Latch)>> {
    let mut st = c.prepare("SELECT k, v FROM kv WHERE k LIKE 'kill_trigger:%' ORDER BY k")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = vec![];
    for row in rows {
        let (k, v) = row?;
        let name = k.trim_start_matches(TRIGGER_PREFIX).to_string();
        if let Some(l) = Latch::from_json(&v) {
            out.push((name, l));
        }
    }
    Ok(out)
}

/// Local unkill: delete the latch (the trigger rows stay). Returns the
/// latch that was cleared. A `program_upgrade` trigger since the previous
/// unkill also drops the stored programdata baseline (`pump_programdata`):
/// the operator has checked the new program (`rugsnipe-ctl sim`), so the
/// next start takes the deployed one as the new baseline.
pub fn unkill(c: &Connection, now_unix: u64) -> rusqlite::Result<Option<Latch>> {
    let was = latched(c)?;
    let last: u64 = c
        .query_row("SELECT v FROM kv WHERE k = 'unkill_unix'", [], |r| {
            r.get::<_, String>(0)
        })
        .optional()?
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let upgraded = triggers(c)?
        .iter()
        .any(|(n, l)| n == "program_upgrade" && l.unix >= last);
    if upgraded {
        c.execute("DELETE FROM kv WHERE k = 'pump_programdata'", [])?;
    }
    c.execute("DELETE FROM kv WHERE k = ?1", params![LATCH_KEY])?;
    c.execute(
        "INSERT OR REPLACE INTO kv(k, v) VALUES('unkill_unix', ?1)",
        params![now_unix.to_string()],
    )?;
    Ok(was)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latch_roundtrip_and_unkill() {
        let dir = std::env::temp_dir().join(format!("rs-killkv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let c = crate::store::open_conn(&dir.join("s.db")).unwrap();
        assert!(latched(&c).unwrap().is_none());
        let l = Latch {
            reason: "send_errors".into(),
            unix: 1_791_000_000,
            detail: "12/50 sends failed".into(),
        };
        c.execute(
            "INSERT INTO kv(k, v) VALUES(?1, ?2)",
            params![LATCH_KEY, l.to_json()],
        )
        .unwrap();
        c.execute(
            "INSERT INTO kv(k, v) VALUES('kill_trigger:send_errors', ?1)",
            params![l.to_json()],
        )
        .unwrap();
        assert_eq!(latched(&c).unwrap(), Some(l.clone()));
        assert_eq!(
            triggers(&c).unwrap(),
            vec![("send_errors".to_string(), l.clone())]
        );
        c.execute("INSERT INTO kv(k, v) VALUES('pump_programdata', '1:')", [])
            .unwrap();
        assert_eq!(unkill(&c, 5).unwrap(), Some(l.clone()));
        assert!(latched(&c).unwrap().is_none());
        assert_eq!(triggers(&c).unwrap().len(), 1, "history stays");
        assert!(
            crate::store::kv_get(&c, "pump_programdata")
                .unwrap()
                .is_some(),
            "no upgrade: kept"
        );
        // an upgrade trigger since the last unkill resets the baseline
        let up = Latch {
            reason: "program_upgrade".into(),
            unix: 10,
            detail: "slot 1 -> 2".into(),
        };
        c.execute(
            "INSERT INTO kv(k, v) VALUES('kill_trigger:program_upgrade', ?1)",
            params![up.to_json()],
        )
        .unwrap();
        unkill(&c, 11).unwrap();
        assert!(crate::store::kv_get(&c, "pump_programdata")
            .unwrap()
            .is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
