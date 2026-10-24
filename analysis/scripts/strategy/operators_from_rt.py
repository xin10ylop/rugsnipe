#!/usr/bin/env python3
"""Operator table for the operator-wave rule set (docs/09-operator-wave.md).

Reads the copied wallet's round trips (``rt.json``: one row per landed round
trip, as written by the execution decode) and writes the table the hot loop
loads from ``entry.operators_file``: a JSON object keyed by the create
transaction's signer (``csig_signer``, the fee payer), each value

    {"hold_slots": 10, "n": 48, "wins": 23, "pnl_sol": 0.93,
     "dev_sol": 4.03, "bundle_wallets": 3, "stake_tier": 0}

* ``n``: round trips on the signer's launches; ``wins``: those with pnl > 0;
  ``pnl_sol``: their summed pnl (SOL); ``dev_sol``: mean bundle SOL (the
  create's dev buy plus the bundle run, rt.json ``dev_sol``);
  ``bundle_wallets``: modal ``n_bw`` (distinct bundle wallets outside the
  creator set).
* ``hold_slots``: the modal sell DECISION offset from the create slot. The
  sell transaction carries a slot guard whose lower bound is the decision
  slot L (rt.json ``swin[0]``; ``ss`` / ``s_off_c`` are the landing slot,
  which trails L by 0-7 slots). L - create is checked to lie in {1, 3, 6, 10}
  (printed). Ties go to the value of the signer's most recent launch.
  Signers with fewer than 2 round trips get ``--default-hold`` (6).

Public addresses only; no keys. Usage:

    python3 operators_from_rt.py rt.json --out data/operators/ahw-2026-10-23.json
"""

from __future__ import annotations

import argparse
import collections
import json
import os
import sys
from typing import Any

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from operators_refresh import stake_tier_of  # noqa: E402  (the ladder decode shared with the refresh)

HOLDS = (1, 3, 6, 10)


def decision_offset(r: dict[str, Any]) -> int | None:
    """L - create: the sell's slot-guard minimum minus the create slot."""
    w = r.get("swin")
    if not w or w[0] is None or r.get("cs") is None:
        return None
    return int(w[0]) - int(r["cs"])


def modal(values: list[tuple[int, int]]) -> int | None:
    """Most frequent value of ``(value, create_slot)`` pairs; a tie goes to
    the value seen at the latest create slot."""
    if not values:
        return None
    cnt = collections.Counter(v for v, _ in values)
    top = max(cnt.values())
    tied = {v for v, c in cnt.items() if c == top}
    return max((cs, v) for v, cs in values if v in tied)[1]


def build(rows: list[dict[str, Any]], min_n: int = 1, default_hold: int = 6) -> dict[str, dict]:
    by: dict[str, list[dict[str, Any]]] = collections.defaultdict(list)
    for r in rows:
        if r.get("skip") or not r.get("csig_signer"):
            continue
        by[r["csig_signer"]].append(r)
    out: dict[str, dict] = {}
    for signer, rs in by.items():
        n = len(rs)
        if n < min_n:
            continue
        offs = [(o, int(r["cs"])) for r in rs if (o := decision_offset(r)) is not None]
        hold = modal(offs) if n >= 2 else None
        out[signer] = {
            "hold_slots": int(hold if hold is not None and hold >= 1 else default_hold),
            "n": n,
            "wins": sum(1 for r in rs if (r.get("pnl") or 0) > 0),
            "pnl_sol": round(sum(r.get("pnl") or 0.0 for r in rs), 6),
            "dev_sol": round(sum(r.get("dev_sol") or 0.0 for r in rs) / n, 4),
            "bundle_wallets": int(modal([(int(r.get("n_bw") or 0), int(r["cs"])) for r in rs]) or 0),
            # the wallet's confidence tier: its modal stake level on this signer (rt.json ``mx`` = 1.01 x the
            # spend, else ``sol``), docs/09 "stake tier"
            "stake_tier": int(modal([(t, int(r["cs"])) for r in rs
                                     if (t := stake_tier_of((r["mx"] / 1.01e9) if r.get("mx") else r.get("sol"))) is not None]) or 0),
        }
    return dict(sorted(out.items(), key=lambda kv: (-kv[1]["n"], -kv[1]["pnl_sol"], kv[0])))


def check(rows: list[dict[str, Any]]) -> tuple[int, int, collections.Counter]:
    """(rows with L - create in {1,3,6,10}, rows with an offset, the offsets)."""
    offs = [o for r in rows if not r.get("skip") and (o := decision_offset(r)) is not None]
    c = collections.Counter(offs)
    return sum(v for k, v in c.items() if k in HOLDS), len(offs), c


def table(ops: dict[str, dict], top: int) -> str:
    lines = [f"{'signer':<44} {'n':>3} {'wins':>4} {'pnl_sol':>8} {'dev_sol':>7} {'bw':>2} {'hold':>4}"]
    for k, v in list(ops.items())[:top]:
        lines.append(
            f"{k:<44} {v['n']:>3} {v['wins']:>4} {v['pnl_sol']:>8.3f} {v['dev_sol']:>7.2f} "
            f"{v['bundle_wallets']:>2} {v['hold_slots']:>4}"
        )
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("rt", help="rt.json (one row per round trip)")
    ap.add_argument("--out", help="write the operator table here (default: stdout)")
    ap.add_argument("--min-n", type=int, default=1, help="keep signers with at least this many round trips (default 1: all)")
    ap.add_argument("--default-hold", type=int, default=6, help="hold for signers with n < 2 (default 6)")
    ap.add_argument("--top", type=int, default=40, help="rows of the printed table (default 40)")
    a = ap.parse_args(argv)
    with open(a.rt) as f:
        rows = json.load(f)
    if isinstance(rows, dict):
        rows = rows.get("rows", [])
    ok, total, offs = check(rows)
    print(f"L - create in {{1,3,6,10}}: {ok}/{total} rows; offsets {dict(sorted(offs.items()))}", file=sys.stderr)
    ops = build(rows, a.min_n, a.default_hold)
    text = json.dumps(ops, indent=1) + "\n"
    if a.out:
        with open(a.out, "w") as f:
            f.write(text)
        print(f"wrote {len(ops)} signers to {a.out}", file=sys.stderr)
    else:
        sys.stdout.write(text)
    hold_n = collections.Counter(v["hold_slots"] for v in ops.values())
    trades = sum(v["n"] for v in ops.values())
    rep = {k: v for k, v in ops.items() if v["n"] >= 3}
    print(
        f"signers {len(ops)}, round trips {trades}; holds {dict(sorted(hold_n.items()))}; "
        f"n>=3: {len(rep)} signers, {sum(v['n'] for v in rep.values())} trades, "
        f"{sum(v['pnl_sol'] for v in rep.values()):.2f} of {sum(v['pnl_sol'] for v in ops.values()):.2f} SOL",
        file=sys.stderr,
    )
    print(table(ops, a.top), file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
