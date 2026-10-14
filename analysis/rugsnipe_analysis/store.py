"""analytics.db: the SQLite (WAL) store of the analytics box (docs/06-architecture.md section 5.2).

Conventions
-----------
* SOL amounts are INTEGER lamports, token amounts INTEGER raw units (6 decimals),
  pubkeys and signatures base58 TEXT, times INTEGER unix seconds unless the
  column name says ``_ms`` / ``_ns``.  Booleans are 0/1.
* Writes are idempotent upserts keyed on the natural key.  By default a NULL in
  the new row never overwrites a known value (``COALESCE(excluded.c, t.c)``), so
  sources with partial information (logs, PumpPortal, getTransaction, the API)
  can be merged in any order.  ``op_assign`` is immutable (insert-or-ignore).
* ``schema_version`` records applied migrations; :func:`connect` migrates.

Loaders for the pre-M0 JSONL datasets (``tokens_light``, ``tokens_sample``,
``funding_creators``) live at the end of this module; ``scripts/ingest_existing.py``
runs them.
"""
from __future__ import annotations

import json
import sqlite3
import time
from pathlib import Path
from typing import Any, Iterable, Sequence
from urllib.parse import urlparse

from .cache import REPO_ROOT

LOCAL_DB = REPO_ROOT / "data" / "analytics.db"
OHIO_DB = Path("/var/lib/rugsnipe/analytics.db")
LAMPORTS = 1_000_000_000
TOKEN_UNIT = 1_000_000

# --------------------------------------------------------------------------- regimes
# Slot boundaries: epoch 1053 starts at 1053 * 432000 = 454,896,000 (200 ms slots since
# Oct 9 2026 [V, docs/01]).  The Alpenglow boundary is the start of epoch 1064
# (459,648,000) inside the config's freeze window [459,640,000, 459,869,000] [U]: move
# it once the cutover slot is observed.  v1 transactions exist from Sep 15 2026.
SEP15_2026 = 1789430400          # 2026-09-15T00:00:00Z
SLOT_200MS_FROM = 454_896_000
SLOT_200MS_FROM_TIME = 1791556858  # blockTime of slot 454,896,000 (2026-10-09T14:40:58Z, gap-1 index)
ALPENGLOW_FROM_SLOT = 459_648_000
ALPENGLOW_FROM_TIME = 1792540800  # 2026-10-21T00:00:00Z, used when the slot is unknown
REGIMES = ("pre_v1", "slot400", "slot200", "alpenglow")


def regime_for(slot: int | None, block_time: int | None = None) -> str | None:
    """Regime tag (section 9.1): pre_v1 < Sep 15 <= slot400 < Oct 9 <= slot200 < Oct 21 <= alpenglow."""
    if block_time is not None and block_time < SEP15_2026:
        return "pre_v1"
    if slot is not None:
        if slot < SLOT_200MS_FROM:
            return "slot400" if block_time is None or block_time >= SEP15_2026 else "pre_v1"
        return "slot200" if slot < ALPENGLOW_FROM_SLOT else "alpenglow"
    if block_time is None:
        return None
    if block_time < SLOT_200MS_FROM_TIME:
        return "slot400"
    return "slot200" if block_time < ALPENGLOW_FROM_TIME else "alpenglow"


# --------------------------------------------------------------------------- schema
SCHEMA_VERSION = 1

SCHEMA_V1 = """
CREATE TABLE IF NOT EXISTS schema_version(version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);

-- one row per successful pump.fun create (key: mint)
CREATE TABLE IF NOT EXISTS creates(
  mint TEXT PRIMARY KEY, slot INTEGER, tx_index INTEGER, block_time INTEGER, sig TEXT,
  signer TEXT,              -- CreateEvent.user (a signer of the create tx)
  fee_payer TEXT, signers_json TEXT, creator_arg TEXT,
  launcher TEXT,            -- first top-level non-infrastructure program, NULL = direct
  launcher_programs_json TEXT, tx_version TEXT, ix_variant TEXT,
  bonding_curve TEXT, quote_mint TEXT, token_program TEXT, is_mayhem INTEGER, is_cashback INTEGER,
  name TEXT, symbol TEXT, uri_host TEXT,
  fee_lamports INTEGER, priority_lamports INTEGER, tip_lamports INTEGER, cu_used INTEGER,
  intx_sol_in INTEGER, intx_tokens_out INTEGER, intx_n_buys INTEGER, v_after_intx INTEGER,
  init_vsol INTEGER, init_vtok INTEGER, n_creates_in_tx INTEGER,
  regime TEXT, sample_arm TEXT, src TEXT, decoded_from TEXT, first_seen_ms INTEGER, updated_at INTEGER);
CREATE INDEX IF NOT EXISTS creates_slot ON creates(slot);
CREATE INDEX IF NOT EXISTS creates_signer ON creates(signer);
CREATE INDEX IF NOT EXISTS creates_creator ON creates(creator_arg);
CREATE INDEX IF NOT EXISTS creates_sig ON creates(sig);

-- every pump buy inside the create tx (ev_index = order among the tx's TradeEvents)
CREATE TABLE IF NOT EXISTS create_buys(
  sig TEXT, ev_index INTEGER, mint TEXT, signer TEXT, token_owner TEXT, ix_name TEXT,
  sol_in INTEGER, tokens_out INTEGER, vsol_post INTEGER, vtok_post INTEGER, fee_bps INTEGER,
  creator_fee_bps INTEGER, PRIMARY KEY(sig, ev_index));
CREATE INDEX IF NOT EXISTS create_buys_mint ON create_buys(mint);

-- first-seen time of a create signature per feed (src: pp | logs | repair | relay ...)
CREATE TABLE IF NOT EXISTS feed_seen(
  sig TEXT, src TEXT, recv_ns INTEGER, slot INTEGER, mint TEXT, err INTEGER, PRIMARY KEY(sig, src));
CREATE INDEX IF NOT EXISTS feed_seen_mint ON feed_seen(mint);

-- trades (swap-api tapes or decoded TradeEvents). sub_index: swap-api sub index / event index
CREATE TABLE IF NOT EXISTS trades(
  mint TEXT, slot INTEGER, tx_index INTEGER, sub_index INTEGER, sig TEXT, user TEXT, token_owner TEXT,
  is_buy INTEGER, sol INTEGER, tokens INTEGER, vsol_post INTEGER, vtok_post INTEGER,
  mcap_lamports INTEGER, ix_name TEXT, fee_bps INTEGER, creator_fee_bps INTEGER, ts INTEGER, src TEXT,
  PRIMARY KEY(mint, slot, tx_index, sub_index));
CREATE INDEX IF NOT EXISTS trades_user ON trades(user);
CREATE TABLE IF NOT EXISTS tapes(
  mint TEXT PRIMARY KEY, t0 INTEGER, fetched_at INTEGER, early_secs INTEGER, coverage TEXT,
  n_trades INTEGER, n_early_raw INTEGER, n_latest_raw INTEGER, src TEXT);

-- transactions touching a bonding curve (incl. FAILED ones: rivals' losses)
CREATE TABLE IF NOT EXISTS tx_attempts(
  mint TEXT, sig TEXT, slot INTEGER, tx_index INTEGER, block_time INTEGER, signer TEXT, ok INTEGER,
  err TEXT, fee_lamports INTEGER, cu_used INTEGER, priority_lamports INTEGER, tip_lamports INTEGER,
  tip_recipient TEXT, path TEXT, tx_version TEXT, src TEXT, PRIMARY KEY(mint, sig));
CREATE INDEX IF NOT EXISTS tx_attempts_slot ON tx_attempts(slot);
CREATE TABLE IF NOT EXISTS sol_transfers(
  slot INTEGER, sig TEXT, funder TEXT, recipient TEXT, lamports INTEGER, recipient_was_new INTEGER,
  PRIMARY KEY(sig, funder, recipient));
CREATE TABLE IF NOT EXISTS token_transfers(
  mint TEXT, slot INTEGER, tx_index INTEGER, sig TEXT, src_owner TEXT, dst_owner TEXT, amount INTEGER,
  PRIMARY KEY(sig, mint, src_owner, dst_owner));

-- funding of a wallet (usually a create signer) as of the create
CREATE TABLE IF NOT EXISTS funding(
  wallet TEXT PRIMARY KEY, status TEXT, funder TEXT, amount INTEGER, slot INTEGER, block_time INTEGER,
  sig TEXT, kind TEXT, main_funder TEXT, main_amount INTEGER, main_sig TEXT, funder_label TEXT,
  funder_category TEXT, depth INTEGER, chain_json TEXT, pattern TEXT, fresh INTEGER, n_prior INTEGER,
  lag_s INTEGER, as_of_slot INTEGER, as_of_mint TEXT, src TEXT, updated_at INTEGER);
CREATE INDEX IF NOT EXISTS funding_funder ON funding(funder);
CREATE TABLE IF NOT EXISTS labels(
  address TEXT PRIMARY KEY, entity TEXT, name TEXT, category TEXT, confidence TEXT, verified_at INTEGER);

CREATE TABLE IF NOT EXISTS slot0(
  mint TEXT PRIMARY KEY, n_adjacent INTEGER, adj_sol INTEGER, n_reactive INTEGER, reactive_sol INTEGER,
  slot0_sol INTEGER, slot0_n_buys INTEGER, slot0_wallets INTEGER, v_after_bundle INTEGER,
  v_eos0 INTEGER, v_eos1 INTEGER, shape TEXT, shared_tip_acct INTEGER, any_slot0_sell INTEGER,
  bundle_owners_json TEXT, src TEXT);
CREATE TABLE IF NOT EXISTS firstslot_metrics(mint TEXT PRIMARY KEY, arm TEXT, coverage TEXT, json TEXT);
CREATE TABLE IF NOT EXISTS coin_snapshots(mint TEXT, fetched_at INTEGER, json TEXT, PRIMARY KEY(mint, fetched_at));

-- operators (point in time)
CREATE TABLE IF NOT EXISTS operators(
  op_id INTEGER PRIMARY KEY, created_slot INTEGER, family TEXT, merged_into INTEGER, retired_slot INTEGER);
CREATE TABLE IF NOT EXISTS op_edges(
  op_id INTEGER, kind TEXT, key TEXT, first_slot INTEGER, last_slot INTEGER, weight REAL,
  PRIMARY KEY(op_id, kind, key));
CREATE INDEX IF NOT EXISTS op_edges_key ON op_edges(kind, key);
CREATE TABLE IF NOT EXISTS op_assign(
  mint TEXT PRIMARY KEY, op_id INTEGER, as_of_slot INTEGER, rule TEXT, conf REAL);

-- evaluation
CREATE TABLE IF NOT EXISTS outcomes(
  mint TEXT, policy_id TEXT, entry_model TEXT, k_hat INTEGER, cost_level TEXT, size INTEGER,
  v_entry INTEGER, filled INTEGER, exit_reason TEXT, exit_slot INTEGER, pnl INTEGER, hold_ms INTEGER,
  PRIMARY KEY(mint, policy_id, entry_model, k_hat, cost_level, size));
CREATE TABLE IF NOT EXISTS prereg(
  sha TEXT PRIMARY KEY, committed_at INTEGER, grid_json TEXT, gate_json TEXT, vault_from INTEGER);
CREATE TABLE IF NOT EXISTS runs(
  run_id TEXT PRIMARY KEY, code_sha TEXT, prereg_sha TEXT, n_configs_tried INTEGER,
  data_from INTEGER, data_to INTEGER, params_json TEXT, created_at INTEGER);
CREATE TABLE IF NOT EXISTS folds(
  run_id TEXT, fold INTEGER, test_from INTEGER, test_to INTEGER, n INTEGER, ev_sel REAL, ev_ctrl REAL,
  diff_lb_cluster REAL, hit REAL, hit_wilson_lb REAL, PRIMARY KEY(run_id, fold));
CREATE TABLE IF NOT EXISTS arm_decisions(
  op_id INTEGER, decided_at INTEGER, state TEXT, reason TEXT, n_oos INTEGER, ev_mean REAL, ev_lb REAL,
  p_ev_pos REAL, run_id TEXT, armset_ver INTEGER, PRIMARY KEY(op_id, decided_at));
CREATE TABLE IF NOT EXISTS armsets(
  ver INTEGER PRIMARY KEY, sha256 TEXT, created_at INTEGER, valid_until INTEGER, run_id TEXT,
  prereg_sha TEXT, regime TEXT, gate_passed INTEGER, n_armed INTEGER, path TEXT);
CREATE TABLE IF NOT EXISTS ab_assign(mint TEXT PRIMARY KEY, op_id INTEGER, arm TEXT, u REAL, salt_id TEXT);

-- ingested from the hot box (state.db / rslog)
CREATE TABLE IF NOT EXISTS intents(
  intent_id INTEGER, box TEXT, mint TEXT, op_id INTEGER, mode TEXT, armset_ver INTEGER, slot INTEGER,
  via TEXT, feed_class TEXT, spend INTEGER, v_max INTEGER, min_tokens_out INTEGER, decided_ns INTEGER,
  state TEXT, PRIMARY KEY(box, intent_id));
CREATE TABLE IF NOT EXISTS fills(
  sig TEXT PRIMARY KEY, box TEXT, intent_id INTEGER, mint TEXT, slot INTEGER, tx_index INTEGER,
  tokens INTEGER, sol_delta INTEGER, err TEXT, buyers_ahead INTEGER, rival_sol_ahead INTEGER, late INTEGER);
CREATE TABLE IF NOT EXISTS landing(
  intent_id INTEGER, box TEXT, sender TEXT, leader_class TEXT, slot_offset INTEGER, buyers_ahead INTEGER,
  rival_sol_ahead INTEGER, PRIMARY KEY(intent_id, box, sender));
CREATE TABLE IF NOT EXISTS box_events(
  id INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER, box TEXT, kind TEXT, detail TEXT);

-- data-quality bookkeeping
CREATE TABLE IF NOT EXISTS ingest_gaps(
  id INTEGER PRIMARY KEY AUTOINCREMENT, source TEXT, from_slot INTEGER, to_slot INTEGER,
  from_ts INTEGER, to_ts INTEGER, reason TEXT, repaired INTEGER DEFAULT 0);
CREATE TABLE IF NOT EXISTS samples(
  design TEXT, sig TEXT, slot INTEGER, block_time INTEGER, arm TEXT, u REAL, status TEXT,
  mint TEXT, updated_at INTEGER, PRIMARY KEY(design, sig));
CREATE INDEX IF NOT EXISTS samples_status ON samples(design, status);
CREATE TABLE IF NOT EXISTS jobs(
  kind TEXT, key TEXT, due_ts REAL, state TEXT, tries INTEGER DEFAULT 0, payload TEXT, last_err TEXT,
  updated_at INTEGER, PRIMARY KEY(kind, key));
CREATE INDEX IF NOT EXISTS jobs_due ON jobs(state, due_ts);
"""

MIGRATIONS: dict[int, str] = {1: SCHEMA_V1}

# primary keys used by upsert(); tables not listed must pass key= explicitly
KEYS: dict[str, tuple[str, ...]] = {
    "creates": ("mint",), "create_buys": ("sig", "ev_index"), "feed_seen": ("sig", "src"),
    "trades": ("mint", "slot", "tx_index", "sub_index"), "tapes": ("mint",),
    "tx_attempts": ("mint", "sig"), "sol_transfers": ("sig", "funder", "recipient"),
    "token_transfers": ("sig", "mint", "src_owner", "dst_owner"), "funding": ("wallet",),
    "labels": ("address",), "slot0": ("mint",), "firstslot_metrics": ("mint",),
    "coin_snapshots": ("mint", "fetched_at"), "operators": ("op_id",),
    "op_edges": ("op_id", "kind", "key"), "op_assign": ("mint",),
    "outcomes": ("mint", "policy_id", "entry_model", "k_hat", "cost_level", "size"),
    "prereg": ("sha",), "runs": ("run_id",), "folds": ("run_id", "fold"),
    "arm_decisions": ("op_id", "decided_at"), "armsets": ("ver",), "ab_assign": ("mint",),
    "intents": ("box", "intent_id"), "fills": ("sig",), "landing": ("intent_id", "box", "sender"),
    "samples": ("design", "sig"), "jobs": ("kind", "key"),
}
IMMUTABLE = {"op_assign"}
_COLS: dict[tuple[int, str], list[str]] = {}


def connect(path: str | Path = LOCAL_DB, readonly: bool = False) -> sqlite3.Connection:
    """Open (and migrate) analytics.db in WAL mode."""
    path = Path(path)
    if readonly:
        con = sqlite3.connect(f"file:{path}?mode=ro", uri=True, timeout=30)
    else:
        path.parent.mkdir(parents=True, exist_ok=True)
        con = sqlite3.connect(str(path), timeout=30)
        con.execute("PRAGMA journal_mode=WAL")
        con.execute("PRAGMA synchronous=NORMAL")
        migrate(con)
    con.row_factory = sqlite3.Row
    con.execute("PRAGMA foreign_keys=ON")
    return con


def schema_version(con: sqlite3.Connection) -> int:
    try:
        row = con.execute("SELECT MAX(version) FROM schema_version").fetchone()
    except sqlite3.OperationalError:
        return 0
    return int(row[0] or 0)


def migrate(con: sqlite3.Connection) -> int:
    """Apply missing migrations; returns the resulting version."""
    v = schema_version(con)
    for ver in sorted(MIGRATIONS):
        if ver > v:
            con.executescript(MIGRATIONS[ver])
            con.execute("INSERT OR REPLACE INTO schema_version(version, applied_at) VALUES(?, ?)",
                        (ver, int(time.time())))
            con.commit()
            v = ver
    return v


def columns(con: sqlite3.Connection, table: str) -> list[str]:
    key = (id(con), table)
    if key not in _COLS:
        _COLS[key] = [r[1] for r in con.execute(f"PRAGMA table_info({table})")]
        if not _COLS[key]:
            raise KeyError(f"unknown table {table}")
    return _COLS[key]


def upsert(con: sqlite3.Connection, table: str, rows: dict | Iterable[dict],
           key: Sequence[str] | None = None, overwrite: bool = False) -> int:
    """Idempotent insert-or-merge of ``rows`` into ``table``.

    Unknown keys in a row raise.  ``overwrite=False`` (default): a NULL never
    replaces a stored value.  ``overwrite=True``: non-key columns present in the
    row are replaced as given.  Immutable tables use INSERT OR IGNORE.
    Does not commit.
    """
    if isinstance(rows, dict):
        rows = [rows]
    keycols = tuple(key or KEYS[table])
    valid = set(columns(con, table))
    n = 0
    sql_cache: dict[tuple[str, ...], str] = {}
    for row in rows:
        cols = tuple(row.keys())
        bad = [c for c in cols if c not in valid]
        if bad:
            raise KeyError(f"{table}: unknown columns {bad}")
        sql = sql_cache.get(cols)
        if sql is None:
            ph = ",".join("?" * len(cols))
            if table in IMMUTABLE:
                sql = f"INSERT OR IGNORE INTO {table}({','.join(cols)}) VALUES({ph})"
            else:
                upd = [c for c in cols if c not in keycols]
                if upd:
                    sets = ",".join(f"{c}=excluded.{c}" if overwrite else f"{c}=COALESCE(excluded.{c}, {table}.{c})"
                                    for c in upd)
                    tail = f"ON CONFLICT({','.join(keycols)}) DO UPDATE SET {sets}"
                else:
                    tail = f"ON CONFLICT({','.join(keycols)}) DO NOTHING"
                sql = f"INSERT INTO {table}({','.join(cols)}) VALUES({ph}) {tail}"
            sql_cache[cols] = sql
        con.execute(sql, [_sqlval(row[c]) for c in cols])
        n += 1
    return n


def _sqlval(v: Any) -> Any:
    if isinstance(v, bool):
        return int(v)
    if isinstance(v, (dict, list, tuple)):
        return json.dumps(v, separators=(",", ":"))
    return v


def gap(con: sqlite3.Connection, source: str, reason: str, from_ts: int | None = None,
        to_ts: int | None = None, from_slot: int | None = None, to_slot: int | None = None) -> None:
    """Record an ingest gap (a stalled or disconnected source)."""
    con.execute("INSERT INTO ingest_gaps(source, from_slot, to_slot, from_ts, to_ts, reason) "
                "VALUES(?,?,?,?,?,?)", (source, from_slot, to_slot, from_ts, to_ts, reason))


def event(con: sqlite3.Connection, box: str, kind: str, detail: Any) -> None:
    con.execute("INSERT INTO box_events(ts, box, kind, detail) VALUES(?,?,?,?)",
                (int(time.time()), box, kind, detail if isinstance(detail, str) else json.dumps(detail)))


def counts(con: sqlite3.Connection) -> dict[str, int]:
    """Row count per table."""
    tabs = [r[0] for r in con.execute("SELECT name FROM sqlite_master WHERE type='table' "
                                      "AND name NOT LIKE 'sqlite_%' ORDER BY name")]
    return {t: con.execute(f"SELECT COUNT(*) FROM {t}").fetchone()[0] for t in tabs}


# --------------------------------------------------------------------------- unit helpers
def lamports(sol: float | None) -> int | None:
    """SOL float -> integer lamports (exact for values that came from lamports / 1e9)."""
    return None if sol is None else int(round(float(sol) * LAMPORTS))


def raw_tokens(tok: float | None) -> int | None:
    return None if tok is None else int(round(float(tok) * TOKEN_UNIT))


def uri_host(uri: str | None) -> str | None:
    if not uri:
        return None
    try:
        return urlparse(uri).hostname
    except ValueError:
        return None


def mcap_to_vsol(mcap_sol: float | None) -> int | None:
    """Standard-curve virtual SOL reserves (lamports) for a market cap in SOL."""
    if not mcap_sol or mcap_sol <= 0:
        return None
    from .curve import reserves_from_mcap
    return int(reserves_from_mcap(mcap_sol)[0])


# --------------------------------------------------------------------------- JSONL loaders
def _read_jsonl(path: Path) -> Iterable[dict]:
    with open(path) as f:
        for line in f:
            line = line.strip()
            if line:
                yield json.loads(line)


def create_row_from_record(rec: dict, src: str, sample_arm: str | None = None) -> dict:
    """``dataset.creation_record()``-shaped dict -> creates row (no tx-level fields)."""
    trades = rec.get("create_tx_trades") or []
    buys = [t for t in trades if t.get("buy")]
    last = trades[-1] if trades else None
    progs = rec.get("launcher_programs") or []
    return {
        "mint": rec["mint"], "slot": rec.get("create_slot"), "block_time": rec.get("create_time"),
        "sig": rec.get("create_sig"), "signer": rec.get("create_user"), "fee_payer": rec.get("fee_payer"),
        "creator_arg": rec.get("creator"), "bonding_curve": rec.get("bonding_curve"),
        "launcher": launcher_of(progs), "launcher_programs_json": progs or None,
        "quote_mint": rec.get("quote_mint"), "token_program": rec.get("token_program"),
        "is_mayhem": rec.get("is_mayhem_mode"), "name": rec.get("name"), "symbol": rec.get("symbol"),
        "intx_sol_in": sum(lamports(t["sol"]) for t in buys) if trades else None,
        "intx_tokens_out": sum(raw_tokens(t["tok"]) for t in buys) if trades else None,
        "intx_n_buys": len(buys) if "create_tx_trades" in rec else None,
        "v_after_intx": (last or {}).get("vsol") if trades else rec.get("init_vsol"),
        "init_vsol": rec.get("init_vsol"), "init_vtok": rec.get("init_vtok"),
        "n_creates_in_tx": rec.get("n_creates_in_tx"),
        "regime": regime_for(rec.get("create_slot"), rec.get("create_time")),
        "sample_arm": sample_arm, "src": src, "updated_at": int(time.time()),
    }


def create_buy_rows_from_record(rec: dict) -> list[dict]:
    rows = []
    for i, t in enumerate(rec.get("create_tx_trades") or []):
        if not t.get("buy"):
            continue
        rows.append({"sig": rec["create_sig"], "ev_index": i, "mint": rec["mint"], "signer": t.get("user"),
                     "ix_name": t.get("ix"), "sol_in": lamports(t.get("sol")), "tokens_out": raw_tokens(t.get("tok")),
                     "vsol_post": t.get("vsol"), "vtok_post": t.get("vtok"), "fee_bps": t.get("fee_bps"),
                     "creator_fee_bps": t.get("creator_fee_bps")})
    return rows


INFRA_PROGRAMS = {
    "11111111111111111111111111111111", "ComputeBudget111111111111111111111111111111",
    "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL", "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
    "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb", "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",
    "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr", "Memo1UhkJRfHyvLMcVucJwxXeuD728EqVDDwQDxFMNo",
    "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ",
}


def launcher_of(programs: Iterable[str]) -> str | None:
    """First top-level program that is not pump / system / token / ATA / compute budget / memo."""
    for p in programs:
        if p and p not in INFRA_PROGRAMS:
            return p
    return None


def load_tokens_light(con: sqlite3.Connection, path: Path, design: str = "D1_light_48h") -> int:
    """data/tokens_light.jsonl (D1: uniform 1,496 creates in 48 h, dataset.creation_record + coin)."""
    n = 0
    for rec in _read_jsonl(path):
        upsert(con, "creates", create_row_from_record(rec, src="tokens_light", sample_arm="D1"))
        upsert(con, "create_buys", create_buy_rows_from_record(rec))
        if rec.get("coin") and rec.get("coin_fetched_at"):
            upsert(con, "coin_snapshots", {"mint": rec["mint"], "fetched_at": rec["coin_fetched_at"],
                                            "json": rec["coin"]})
        upsert(con, "samples", {"design": design, "sig": rec["create_sig"], "slot": rec.get("create_slot"),
                                "block_time": rec.get("create_time"), "arm": rec.get("arm"),
                                "u": rec.get("sample_rank"), "status": "done", "mint": rec["mint"],
                                "updated_at": int(time.time())})
        n += 1
    return n


def load_tokens_sample(con: sqlite3.Connection, path: Path, design: str = "D2_sample") -> int:
    """data/tokens_sample.jsonl (D2: random + 'interesting' arms with firstslot metrics).

    D2's interesting arms are survivorship-biased (excluded from evaluation, section 9.1);
    the arm is kept in ``creates.sample_arm`` = 'D2_<arm>'.
    """
    n = 0
    for r in _read_jsonl(path):
        upsert(con, "creates", {
            "mint": r["mint"], "slot": r.get("create_slot"), "block_time": r.get("create_time"),
            "sig": r.get("create_sig"), "signer": r.get("create_user"), "fee_payer": r.get("fee_payer"),
            "creator_arg": r.get("creator"), "bonding_curve": r.get("bonding_curve"),
            "launcher": launcher_of(r.get("launcher_programs") or []),
            "launcher_programs_json": r.get("launcher_programs"), "token_program": r.get("token_program"),
            "is_mayhem": r.get("is_mayhem_mode"), "name": r.get("name"), "symbol": r.get("symbol"),
            "tx_index": r.get("create_txi"),
            "regime": regime_for(r.get("create_slot"), r.get("create_time")),
            "sample_arm": f"D2_{r.get('arm')}", "src": "tokens_sample", "updated_at": int(time.time())})
        buys = r.get("slot0_buys") or []          # [wallet, sol, txi, is_dev]
        create_txi = r.get("create_txi")
        adj = [b for b in buys if not b[3] and create_txi is not None and b[2] is not None
               and 0 < b[2] - create_txi <= 3]
        upsert(con, "slot0", {
            "mint": r["mint"], "slot0_sol": lamports(r.get("slot0_buy_sol")), "slot0_n_buys": r.get("slot0_n_buys"),
            "slot0_wallets": r.get("slot0_wallets"), "n_adjacent": r.get("slot0_adjacent_wallets"),
            "adj_sol": lamports(r.get("slot0_adjacent_sol")),
            "n_reactive": max(0, len(buys) - len(adj) - sum(1 for b in buys if b[3])) if buys else None,
            "v_eos0": mcap_to_vsol(r.get("eos0_mcap")), "v_eos1": mcap_to_vsol(r.get("eos1_mcap")),
            "any_slot0_sell": int((r.get("slot0_n_sells") or 0) > 0),
            "bundle_owners_json": sorted({b[0] for b in buys if not b[3]}) or None,
            "src": "tokens_sample(signers; swap-api mcap)"})
        upsert(con, "firstslot_metrics", {"mint": r["mint"], "arm": r.get("arm"), "coverage": r.get("coverage"),
                                          "json": r}, overwrite=True)
        upsert(con, "samples", {"design": design, "sig": r.get("create_sig"), "slot": r.get("create_slot"),
                                "block_time": r.get("create_time"), "arm": r.get("arm"),
                                "u": r.get("sample_rank"), "status": "done", "mint": r["mint"],
                                "updated_at": int(time.time())})
        n += 1
    return n


def funding_row_from_chain(wallet: str, chain: list[dict], summary: dict | None, *, fresh: Any = None,
                           n_prior: int | None = None, lag_s: int | None = None, as_of_slot: int | None = None,
                           as_of_mint: str | None = None, src: str = "") -> dict:
    """``funding.trace_funding`` chain (list of Hop dicts) -> funding row."""
    f = (chain[0].get("funding") if chain else None) or {}
    main = f.get("main") or {}
    lab = (chain[1].get("label") if len(chain) > 1 else None) or {}
    return {
        "wallet": wallet, "status": f.get("status"), "funder": f.get("funder"),
        "amount": lamports(f.get("amount_sol")), "slot": f.get("slot"), "block_time": f.get("block_time"),
        "sig": f.get("signature"), "kind": f.get("kind"), "main_funder": main.get("funder"),
        "main_amount": lamports(main.get("amount_sol")), "main_sig": main.get("signature"),
        "funder_label": lab.get("name"), "funder_category": chain[1].get("kind") if len(chain) > 1 else None,
        "depth": max(0, len(chain) - 1), "chain_json": chain, "pattern": (summary or {}).get("pattern"),
        "fresh": None if fresh is None else int(bool(fresh)), "n_prior": n_prior, "lag_s": lag_s,
        "as_of_slot": as_of_slot, "as_of_mint": as_of_mint, "src": src, "updated_at": int(time.time())}


def load_funding_creators(con: sqlite3.Connection, path: Path) -> int:
    """data/funding_creators.jsonl (Task B funding traces of 180 creators)."""
    n = 0
    for r in _read_jsonl(path):
        wallet = r["creator"]
        upsert(con, "funding", funding_row_from_chain(
            wallet, r.get("chain") or [], r.get("summary"), fresh=r.get("fresh_creator"),
            n_prior=r.get("creator_txs_before_create"), lag_s=r.get("funding_to_create_s"),
            as_of_slot=r.get("create_slot"), as_of_mint=r.get("mint"), src=f"funding_creators:{r.get('arm')}"))
        if r.get("create_slot"):
            upsert(con, "creates", {
                "mint": r["mint"], "slot": r.get("create_slot"), "block_time": r.get("create_time"),
                "sig": r.get("create_sig"), "signer": r.get("create_event_user"),
                "fee_payer": r.get("create_fee_payer"), "signers_json": r.get("create_signers"),
                "symbol": r.get("symbol"), "regime": regime_for(r.get("create_slot"), r.get("create_time")),
                "sample_arm": f"funding_{r.get('arm')}", "src": "funding_creators",
                "updated_at": int(time.time())})
        n += 1
    return n


def load_labels(con: sqlite3.Connection) -> int:
    """labels.py (curated + on-chain verified) -> labels table."""
    from . import labels as L
    rows = [{"address": lab.address, "entity": lab.entity, "name": lab.name, "category": lab.category,
             "confidence": lab.confidence, "verified_at": int(time.time())} for lab in L.all_labels()]
    return upsert(con, "labels", rows, overwrite=True)


def ingest_existing(con: sqlite3.Connection, data_dir: Path = REPO_ROOT / "data") -> dict[str, int]:
    """Load every pre-M0 dataset that exists under ``data_dir``; idempotent."""
    out: dict[str, int] = {}
    # order matters for creates.sample_arm (last non-NULL wins): D1 (uniform) is loaded last
    for name, fn in (("funding_creators.jsonl", load_funding_creators), ("tokens_sample.jsonl", load_tokens_sample),
                     ("tokens_light.jsonl", load_tokens_light)):
        p = data_dir / name
        if p.exists():
            out[name] = fn(con, p)
            con.commit()
    out["labels"] = load_labels(con)
    con.commit()
    return out
