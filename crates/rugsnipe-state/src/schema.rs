//! `state.db` schema: the tables of docs/06 section 5.1 (SQLite WAL,
//! integer lamports and raw token units, base58 text keys), plus (version 2)
//! the shadow-mode record of docs/04 section 7: `fills.fail_reason` /
//! `fills.fail_cost` for buys that would have failed and what they cost, a
//! `shadow` row per resolved paper intent (where the create sat in its
//! block, the bundle, our would-be index, third-party buyers ahead/behind,
//! premium, post-bundle state) and `intents.via = 'class'` (ArmSet class arm).
//! [`migrate`] upgrades a version-1 file in place.

/// Schema version stored in `PRAGMA user_version`.
pub const VERSION: i64 = 2;

/// DDL (idempotent).
pub const DDL: &str = r#"
CREATE TABLE IF NOT EXISTS intents(intent_id INTEGER PRIMARY KEY, mint TEXT NOT NULL, op_id INT, box TEXT, mode TEXT, armset_ver INT, slot INT,
  bank_id INT, via TEXT CHECK(via IN('signer','fee_payer','funding','class')), feed_class TEXT, spend INT, v_max INT,
  min_tokens_out INT, nonce_idx INT, nonce_value TEXT, decided_ns INT,
  state TEXT CHECK(state IN('paper','sent','landed','failed','burned','late_fill')), UNIQUE(mint, mode));
CREATE TABLE IF NOT EXISTS sends(sig TEXT PRIMARY KEY, intent_id INT, variant INT, sender TEXT, prio INT, tip INT, sent_ns INT, result TEXT);
CREATE TABLE IF NOT EXISTS fills(sig TEXT PRIMARY KEY, intent_id INT, mint TEXT, slot INT, tx_index INT, tokens INT, sol_delta INT, err TEXT,
  buyers_ahead INT, rival_sol_ahead INT, late INT, fail_reason TEXT, fail_cost INT);
CREATE TABLE IF NOT EXISTS positions(mint TEXT PRIMARY KEY, intent_id INT, opened_slot INT, tokens INT, cost INT, v_fill INT, peak_v INT,
  bundle_json TEXT, policy_json TEXT, origin TEXT CHECK(origin IN('intent','orphan')),
  state TEXT CHECK(state IN('open','exiting','closed')), sell_spend INT, closed_slot INT);
CREATE TABLE IF NOT EXISTS exits(sig TEXT PRIMARY KEY, mint TEXT, reason TEXT, venue TEXT, tokens INT, sol_delta INT, slot INT, owner TEXT, try_no INT);
CREATE TABLE IF NOT EXISTS nonces(pubkey TEXT PRIMARY KEY, pool TEXT CHECK(pool IN('buy','sell')), idx INT, value TEXT, last_change_slot INT,
  state TEXT CHECK(state IN('free','inflight','burning','quarantined')), intent_id INT, since_slot INT);
CREATE TABLE IF NOT EXISTS watch_dyn(key TEXT PRIMARY KEY, kind TEXT, op_id INT, rule_id INT, added_slot INT, expires_unix INT,
  remove_if_no_launch_unix INT, keep INT, parent TEXT, state TEXT);
CREATE TABLE IF NOT EXISTS risk(day TEXT PRIMARY KEY, start_balance INT, deposits INT, sweeps INT, realized INT, buys INT, killed INT, kill_reason TEXT);
CREATE TABLE IF NOT EXISTS op_risk(op_id INT PRIMARY KEY, consec_losses INT, buys_today INT, disarmed_until INT);
CREATE TABLE IF NOT EXISTS lease(name TEXT PRIMARY KEY, owner TEXT, hb_unix_ms INT, epoch INT);
CREATE TABLE IF NOT EXISTS kv(k TEXT PRIMARY KEY, v TEXT);
CREATE TABLE IF NOT EXISTS shadow(intent_id INTEGER PRIMARY KEY, mint TEXT, model TEXT, outcome TEXT, create_slot INT,
  create_index INT, block_txs INT, bundle_trades INT, bundle_wallets INT, our_index INT, tp_ahead INT, tp_behind INT,
  s0_failed INT, premium_bps INT, post_bundle_v INT, post_bundle_t INT, post_bundle_mcap INT, cost INT);
"#;

/// The version-2 `intents` DDL under a temporary name (CHECK constraints
/// cannot be altered in place: a version-1 table is copied over).
const INTENTS_V2: &str = r#"
CREATE TABLE intents_v2(intent_id INTEGER PRIMARY KEY, mint TEXT NOT NULL, op_id INT, box TEXT, mode TEXT, armset_ver INT, slot INT,
  bank_id INT, via TEXT CHECK(via IN('signer','fee_payer','funding','class')), feed_class TEXT, spend INT, v_max INT,
  min_tokens_out INT, nonce_idx INT, nonce_value TEXT, decided_ns INT,
  state TEXT CHECK(state IN('paper','sent','landed','failed','burned','late_fill')), UNIQUE(mint, mode));
INSERT INTO intents_v2 SELECT * FROM intents;
DROP TABLE intents;
ALTER TABLE intents_v2 RENAME TO intents;
"#;

/// Table names in DDL order.
pub const TABLES: [&str; 12] = [
    "intents",
    "sends",
    "fills",
    "positions",
    "exits",
    "nonces",
    "watch_dyn",
    "risk",
    "op_risk",
    "lease",
    "kv",
    "shadow",
];

/// Open settings for every connection: WAL, NORMAL sync, busy timeout,
/// foreign keys off (the spec has none).
pub fn configure(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}

/// Create the tables (idempotent), upgrade a version-1 file, stamp the version.
pub fn migrate(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch(DDL)?;
    // v1 -> v2: failed-buy columns on fills, `via = 'class'` on intents
    for (col, ty) in [("fail_reason", "TEXT"), ("fail_cost", "INT")] {
        if !has_column(conn, "fills", col)? {
            conn.execute_batch(&format!("ALTER TABLE fills ADD COLUMN {col} {ty}"))?;
        }
    }
    let intents_sql: String = conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'intents'",
        [],
        |r| r.get(0),
    )?;
    if !intents_sql.contains("'class'") {
        conn.execute_batch(&format!("BEGIN;{INTENTS_V2}COMMIT;"))?;
    }
    conn.pragma_update(None, "user_version", VERSION)?;
    Ok(())
}

/// `table` has a column named `col`.
pub fn has_column(conn: &rusqlite::Connection, table: &str, col: &str) -> rusqlite::Result<bool> {
    let mut st = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = st.query_map([], |r| r.get::<_, String>(1))?;
    for n in names {
        if n? == col {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Row counts of every table (reports).
pub fn row_counts(conn: &rusqlite::Connection) -> rusqlite::Result<Vec<(&'static str, i64)>> {
    TABLES
        .iter()
        .map(|t| {
            conn.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))
                .map(|n| (*t, n))
        })
        .collect()
}
