//! `state.db` schema: the tables of docs/06 section 5.1 (SQLite WAL,
//! integer lamports and raw token units, base58 text keys), plus (version 2)
//! the shadow-mode record of docs/04 section 7: `fills.fail_reason` /
//! `fills.fail_cost` for buys that would have failed and what they cost, a
//! `shadow` row per resolved paper intent (where the create sat in its
//! block, the bundle, our would-be index, third-party buyers ahead/behind,
//! premium, post-bundle state) and `intents.via = 'class'` (ArmSet class arm).
//! Version 3 adds the entry filter (docs/04 section 2): a `filter` row per
//! scored intent (P(take-profit), tertile, mode, whether it was dropped, the
//! operator history behind the score, the feature vector) and the live
//! outcome store `outcomes` (our own LEAD outcome of every launch we
//! decided on, written 10 s after the launch at the earliest, reloaded at
//! start). Version 4 (probe, real sends): `sends.bid` (the base bid),
//! `sends.sent_unix_ms`, `sends.kind` (`buy`/`sell`/`burn`) and `sends.err`,
//! and on `fills` the landed transaction's `fee` (`meta.fee`), `variant`,
//! `sender` and `tip`. Version 5 (task #12 second half): the leader class
//! (`intents.leader_class` of the create slot, `sends.leader_class` when the
//! variant was handed over, `fills.leader_class` of the landing slot) and
//! `feed_bundle`, the creation slot's bundle run as the gRPC feed assembled
//! it (the entry filter's bundle features from the stream). The kill latch
//! and its triggers live in `kv` (`kill_latch`, `kill_trigger:<name>`,
//! [`crate::killkv`]). `dumpers` (the operator-dump set) and `presend` (the
//! pre-send entry gate's score of every decision that reached the send) are
//! new tables only (`CREATE TABLE IF NOT EXISTS`, no version bump).
//! [`migrate`] upgrades version-1 to -4 files in place.

/// Schema version stored in `PRAGMA user_version`.
pub const VERSION: i64 = 5;

/// DDL (idempotent).
pub const DDL: &str = r#"
CREATE TABLE IF NOT EXISTS intents(intent_id INTEGER PRIMARY KEY, mint TEXT NOT NULL, op_id INT, box TEXT, mode TEXT, armset_ver INT, slot INT,
  bank_id INT, via TEXT CHECK(via IN('signer','fee_payer','funding','class')), feed_class TEXT, spend INT, v_max INT,
  min_tokens_out INT, nonce_idx INT, nonce_value TEXT, decided_ns INT,
  state TEXT CHECK(state IN('paper','sent','landed','failed','burned','late_fill')), leader_class TEXT, UNIQUE(mint, mode));
CREATE TABLE IF NOT EXISTS sends(sig TEXT PRIMARY KEY, intent_id INT, variant INT, sender TEXT, prio INT, tip INT, sent_ns INT, result TEXT,
  bid INT, sent_unix_ms INT, kind TEXT, err TEXT, leader_class TEXT);
CREATE TABLE IF NOT EXISTS fills(sig TEXT PRIMARY KEY, intent_id INT, mint TEXT, slot INT, tx_index INT, tokens INT, sol_delta INT, err TEXT,
  buyers_ahead INT, rival_sol_ahead INT, late INT, fail_reason TEXT, fail_cost INT, fee INT, variant INT, sender TEXT, tip INT,
  leader_class TEXT);
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
CREATE TABLE IF NOT EXISTS filter(intent_id INTEGER PRIMARY KEY, mint TEXT, model TEXT, score REAL, tertile INT, mode TEXT,
  skipped INT, op_prior INT, op_lead_n INT, op_lead_hit REAL, op_lead_ev REAL, bw_lead_n INT, bw_lead_ev REAL, features TEXT);
CREATE TABLE IF NOT EXISTS outcomes(intent_id INTEGER PRIMARY KEY, mint TEXT, signer TEXT, launch_ms INT, outcome TEXT,
  lead INT, ret REAL, tp_hit INT, s0_third INT, bundle_wallets TEXT, written_ms INT);
CREATE TABLE IF NOT EXISTS feed_bundle(mint TEXT PRIMARY KEY, intent_id INT, slot INT, create_index INT, trades INT,
  bundle_wallets INT, s0_third INT, dev_sol REAL, bundle_sol REAL, me_sol REAL, closed TEXT, run_close_ms REAL,
  out_of_order INT, features TEXT);
CREATE TABLE IF NOT EXISTS dumpers(wallet TEXT PRIMARY KEY, mint TEXT, slot INT, sold_bps INT, written_ms INT);
CREATE TABLE IF NOT EXISTS presend(intent_id INTEGER PRIMARY KEY, mint TEXT, model TEXT, score REAL, tertile INT, mode TEXT,
  skipped INT, features TEXT, written_ms INT);
"#;

/// The version-2 `intents` DDL under a temporary name (CHECK constraints
/// cannot be altered in place: a version-1 table is copied over).
const INTENTS_V2: &str = r#"
CREATE TABLE intents_v2(intent_id INTEGER PRIMARY KEY, mint TEXT NOT NULL, op_id INT, box TEXT, mode TEXT, armset_ver INT, slot INT,
  bank_id INT, via TEXT CHECK(via IN('signer','fee_payer','funding','class')), feed_class TEXT, spend INT, v_max INT,
  min_tokens_out INT, nonce_idx INT, nonce_value TEXT, decided_ns INT,
  state TEXT CHECK(state IN('paper','sent','landed','failed','burned','late_fill')), UNIQUE(mint, mode));
INSERT INTO intents_v2 SELECT intent_id, mint, op_id, box, mode, armset_ver, slot, bank_id, via, feed_class, spend, v_max,
  min_tokens_out, nonce_idx, nonce_value, decided_ns, state FROM intents;
DROP TABLE intents;
ALTER TABLE intents_v2 RENAME TO intents;
"#;

/// Table names in DDL order.
pub const TABLES: [&str; 17] = [
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
    "filter",
    "outcomes",
    "feed_bundle",
    "dumpers",
    "presend",
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
    // v3 -> v4: real-send columns
    for (table, col, ty) in [
        ("fills", "fail_reason", "TEXT"),
        ("fills", "fail_cost", "INT"),
        ("fills", "fee", "INT"),
        ("fills", "variant", "INT"),
        ("fills", "sender", "TEXT"),
        ("fills", "tip", "INT"),
        ("sends", "bid", "INT"),
        ("sends", "sent_unix_ms", "INT"),
        ("sends", "kind", "TEXT"),
        ("sends", "err", "TEXT"),
        ("sends", "leader_class", "TEXT"),
        ("fills", "leader_class", "TEXT"),
    ] {
        if !has_column(conn, table, col)? {
            conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {col} {ty}"))?;
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
    // v4 -> v5 (after the v1 copy, which keeps the v2 column set)
    if !has_column(conn, "intents", "leader_class")? {
        conn.execute_batch("ALTER TABLE intents ADD COLUMN leader_class TEXT")?;
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
