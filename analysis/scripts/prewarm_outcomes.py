#!/usr/bin/env python3
"""Pre-warm a probe state.db's live outcome store from a shadow state.db (docs/research/probe-design-v2.md).

The entry filter's operator features (op_lead_n, op_lead_hit, op_lead_ev, op_s0t, and the best bundle wallet's
history) come from the `outcomes` table: our own LEAD outcome of every launch we decided on, written 10 s after the
launch and reloaded by rugsnipe-hot at start (crates/rugsnipe-state/src/store.rs `load_outcomes`). A fresh probe
file starts with an empty store, so skip-bottom-tertile mode would score every signer as first-seen for days. This
copies the shadow run's rows into the probe file so the probe starts with the shadow run's history:

  ../.venv/bin/python -I scripts/prewarm_outcomes.py --src /var/lib/rugsnipe/shadow/state.db \
      --dst /var/lib/rugsnipe/state.db [--max-age-days 14] [--dry-run]

* rows are copied as they are (mint, signer, launch_ms, outcome, lead, ret, tp_hit, s0_third, bundle_wallets,
  written_ms); `intent_id` is stored negated (-id), so a pre-warmed row can never collide with (and be replaced by)
  an intent the probe itself writes (positive ids from 1). Re-running is idempotent (INSERT OR REPLACE on -id).
* --dst is created with the schema's DDL (parsed from crates/rugsnipe-state/src/schema.rs, user_version stamped)
  when it does not exist; an existing file must already have an `outcomes` table.
* the source is opened read-only; a source without an `outcomes` table (schema < 3) is refused.
* point in time: every copied row was written before the probe starts, so the history a probe decision sees is
  still history as of that decision (the scorer excludes the launch being decided).
"""
from __future__ import annotations

import argparse
import re
import sqlite3
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
SCHEMA_RS = REPO / "crates" / "rugsnipe-state" / "src" / "schema.rs"
COLS = ("mint", "signer", "launch_ms", "outcome", "lead", "ret", "tp_hit", "s0_third", "bundle_wallets", "written_ms")
OUTCOMES_DDL = ("CREATE TABLE IF NOT EXISTS outcomes(intent_id INTEGER PRIMARY KEY, mint TEXT, signer TEXT, launch_ms INT, "
                "outcome TEXT, lead INT, ret REAL, tp_hit INT, s0_third INT, bundle_wallets TEXT, written_ms INT);")


def schema_ddl() -> tuple[str, int | None]:
    """The hot box's DDL and schema version, from the Rust source (fallback: the outcomes table only)."""
    try:
        src = SCHEMA_RS.read_text()
        ddl = re.search(r'pub const DDL: &str = r#"(.*?)"#;', src, re.S).group(1)
        ver = int(re.search(r"pub const VERSION: i64 = (\d+);", src).group(1))
        return ddl, ver
    except (OSError, AttributeError, ValueError):
        return OUTCOMES_DDL, None


def has_table(con, name) -> bool:
    return con.execute("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?", (name,)).fetchone() is not None


def prewarm(src: str, dst: str, max_age_days: float | None = None, dry_run: bool = False) -> dict:
    s = sqlite3.connect(f"file:{src}?mode=ro", uri=True)
    if not has_table(s, "outcomes"):
        raise SystemExit(f"{src}: no outcomes table (schema < 3): nothing to pre-warm from")
    q = f"SELECT intent_id, {', '.join(COLS)} FROM outcomes"
    args: tuple = ()
    if max_age_days is not None:
        top = s.execute("SELECT MAX(launch_ms) FROM outcomes").fetchone()[0] or 0
        q += " WHERE launch_ms >= ?"
        args = (int(top - max_age_days * 86_400_000),)
    rows = s.execute(q, args).fetchall()
    s.close()
    rows = [r for r in rows if r[0] is not None and r[0] > 0]       # pre-warmed rows of the source are not re-copied
    info = dict(src=src, dst=dst, rows=len(rows), signers=len({r[2] for r in rows}),
                launch_ms=[min((r[3] for r in rows), default=None), max((r[3] for r in rows), default=None)])
    if dry_run:
        return info
    new = not Path(dst).exists()
    d = sqlite3.connect(dst)
    if new:
        ddl, ver = schema_ddl()
        d.executescript(ddl)
        if ver is not None:
            d.execute(f"PRAGMA user_version = {ver}")
    elif not has_table(d, "outcomes"):
        d.close()
        raise SystemExit(f"{dst}: exists without an outcomes table; start rugsnipe-hot on it once (migrates) or remove it")
    before = d.execute("SELECT COUNT(*) FROM outcomes").fetchone()[0]
    d.executemany(f"INSERT OR REPLACE INTO outcomes(intent_id, {', '.join(COLS)}) VALUES ({', '.join('?' * (len(COLS) + 1))})",
                  [(-r[0], *r[1:]) for r in rows])
    d.execute("CREATE TABLE IF NOT EXISTS kv(k TEXT PRIMARY KEY, v TEXT)")
    d.execute("INSERT OR REPLACE INTO kv(k, v) VALUES ('outcomes_prewarmed_from', ?)",
              (f"{Path(src).resolve()} rows={len(rows)} at_unix={int(time.time())}",))
    d.commit()
    info.update(created=new, dst_rows_before=before, dst_rows_after=d.execute("SELECT COUNT(*) FROM outcomes").fetchone()[0])
    d.close()
    return info


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--src", required=True, help="shadow state.db (read-only)")
    ap.add_argument("--dst", required=True, help="probe state.db (created if missing)")
    ap.add_argument("--max-age-days", type=float, help="copy only launches within D days of the newest one")
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()
    if Path(a.src).resolve() == Path(a.dst).resolve():
        raise SystemExit("--src and --dst are the same file")
    info = prewarm(a.src, a.dst, a.max_age_days, a.dry_run)
    for k, v in info.items():
        print(f"{k:16s} {v}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
