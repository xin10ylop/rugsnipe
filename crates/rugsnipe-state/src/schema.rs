//! `state.db` schema: exactly the tables of docs/06 section 5.1 (SQLite WAL,
//! integer lamports and raw token units, base58 text keys).

/// Schema version stored in `PRAGMA user_version`.
pub const VERSION: i64 = 1;

/// DDL (idempotent).
pub const DDL: &str = r#"
CREATE TABLE IF NOT EXISTS intents(intent_id INTEGER PRIMARY KEY, mint TEXT NOT NULL, op_id INT, box TEXT, mode TEXT, armset_ver INT, slot INT,
  bank_id INT, via TEXT CHECK(via IN('signer','fee_payer','funding')), feed_class TEXT, spend INT, v_max INT,
  min_tokens_out INT, nonce_idx INT, nonce_value TEXT, decided_ns INT,
  state TEXT CHECK(state IN('paper','sent','landed','failed','burned','late_fill')), UNIQUE(mint, mode));
CREATE TABLE IF NOT EXISTS sends(sig TEXT PRIMARY KEY, intent_id INT, variant INT, sender TEXT, prio INT, tip INT, sent_ns INT, result TEXT);
CREATE TABLE IF NOT EXISTS fills(sig TEXT PRIMARY KEY, intent_id INT, mint TEXT, slot INT, tx_index INT, tokens INT, sol_delta INT, err TEXT,
  buyers_ahead INT, rival_sol_ahead INT, late INT);
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
"#;

/// Table names in DDL order.
pub const TABLES: [&str; 11] = [
    "intents", "sends", "fills", "positions", "exits", "nonces", "watch_dyn", "risk", "op_risk",
    "lease", "kv",
];

/// Open settings for every connection: WAL, NORMAL sync, busy timeout,
/// foreign keys off (the spec has none).
pub fn configure(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}

/// Create the tables (idempotent) and stamp the version.
pub fn migrate(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch(DDL)?;
    conn.pragma_update(None, "user_version", VERSION)?;
    Ok(())
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
