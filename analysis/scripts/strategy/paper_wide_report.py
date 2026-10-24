"""Paper-wide report (docs/09 "Paper wide", 2026-10-24): what opening the live bot to signers outside the
operator table would earn, measured by rugsnipe-paper-wide.service (paper mode, the live gates, only the
launches of unlisted signers), next to the live bot over the same window.

    sudo python3 -I /opt/rugsnipe/src/analysis/scripts/strategy/paper_wide_report.py \\
        [--paper /var/lib/rugsnipe/paper-wide/state.db] [--live /var/lib/rugsnipe/state.db] \\
        [--min-trades 150] [--bar 0.03] [--json]

Both databases are opened read-only. A closed position's P&L is the sum of its exits' SOL minus its cost; the
return is that over the cost. The rule: after at least --min-trades closed paper trades, OPEN (set the live
bot's entry.operators_only = false) only if the paper mean return per trade is at least --bar and both halves
(by open slot) are positive; otherwise KEEP the table. The bar is above zero on purpose: paper counts every
launch where we would have been first as landed, while live we land about 55% and lose the contested ones to
higher bids, so live opening earns less than paper.
"""
from __future__ import annotations

import argparse
import json
import sqlite3
import statistics
import sys

SLOT_S = 0.4
REAL = ("probe", "live")


def connect(path: str) -> sqlite3.Connection:
    return sqlite3.connect(f"file:{path}?mode=ro", uri=True)


def closed(con: sqlite3.Connection, modes: tuple[str, ...], since_slot: int | None = None) -> list[dict]:
    q = f"""SELECT p.mint, p.cost, p.opened_slot, p.closed_slot, i.mode,
                   (SELECT COALESCE(SUM(e.sol_delta), 0) FROM exits e WHERE e.mint = p.mint) AS proceeds
            FROM positions p JOIN intents i ON i.intent_id = p.intent_id
            WHERE p.state = 'closed' AND p.cost > 0 AND i.mode IN ({",".join("?" * len(modes))})
                  {"AND p.opened_slot >= ?" if since_slot is not None else ""}
            ORDER BY p.opened_slot"""
    args = list(modes) + ([since_slot] if since_slot is not None else [])
    out = []
    for mint, cost, o, c, mode, proceeds in con.execute(q, args):
        pnl = (proceeds or 0) - cost
        out.append({"mint": mint, "cost": cost, "opened_slot": o, "closed_slot": c, "mode": mode,
                    "pnl_sol": pnl / 1e9, "ret": pnl / cost})
    return out


def open_count(con: sqlite3.Connection, modes: tuple[str, ...]) -> int:
    return con.execute(f"""SELECT COUNT(*) FROM positions p JOIN intents i ON i.intent_id = p.intent_id
                           WHERE p.state != 'closed' AND i.mode IN ({",".join("?" * len(modes))})""", modes).fetchone()[0]


def summary(rows: list[dict]) -> dict:
    n = len(rows)
    if not n:
        return {"n": 0}
    rets = [r["ret"] for r in rows]
    h = n // 2
    first, second = rets[:h], rets[h:]
    span = (rows[-1]["opened_slot"] - rows[0]["opened_slot"]) * SLOT_S / 3600 if n > 1 else 0.0
    return {"n": n, "wins": sum(1 for x in rets if x > 0), "win_rate": sum(1 for x in rets if x > 0) / n,
            "mean_ret": statistics.mean(rets), "median_ret": statistics.median(rets),
            "net_sol": sum(r["pnl_sol"] for r in rows),
            "mean_ret_first_half": statistics.mean(first) if first else None,
            "mean_ret_second_half": statistics.mean(second) if second else None,
            "hours": span, "per_hour": n / span if span > 0 else None}


def verdict(s: dict, min_trades: int, bar: float) -> str:
    if s.get("n", 0) < min_trades:
        return f"KEEP MEASURING ({s.get('n', 0)} of {min_trades} closed paper trades)"
    halves_ok = (s["mean_ret_first_half"] or 0) > 0 and (s["mean_ret_second_half"] or 0) > 0
    if s["mean_ret"] >= bar and halves_ok:
        return f"OPEN: mean {100 * s['mean_ret']:+.1f}% per trade >= {100 * bar:.1f}%, both halves positive"
    return (f"KEEP THE TABLE: mean {100 * s['mean_ret']:+.1f}% per trade (bar {100 * bar:.1f}%), halves "
            f"{100 * (s['mean_ret_first_half'] or 0):+.1f}% / {100 * (s['mean_ret_second_half'] or 0):+.1f}%")


def decisions(con: sqlite3.Connection) -> dict:
    n = con.execute("SELECT COUNT(*) FROM intents WHERE mode = 'paper'").fetchone()[0]
    outcomes = dict(con.execute("SELECT COALESCE(outcome, '?'), COUNT(*) FROM shadow GROUP BY outcome").fetchall())
    return {"paper_intents": n, "shadow_outcomes": outcomes}


def line(name: str, s: dict) -> str:
    if not s.get("n"):
        return f"{name:<22} no closed trades yet"
    ph = f"{s['per_hour']:.1f}/h" if s.get("per_hour") else "-"
    return (f"{name:<22} {s['n']:>4} closed  {ph:>7}  win {100 * s['win_rate']:>3.0f}%  mean {100 * s['mean_ret']:+5.1f}%"
            f"  median {100 * s['median_ret']:+5.1f}%  net {s['net_sol']:+.3f} SOL  halves "
            f"{100 * (s['mean_ret_first_half'] or 0):+.1f}% / {100 * (s['mean_ret_second_half'] or 0):+.1f}%")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--paper", default="/var/lib/rugsnipe/paper-wide/state.db")
    ap.add_argument("--live", default="/var/lib/rugsnipe/state.db")
    ap.add_argument("--min-trades", type=int, default=150)
    ap.add_argument("--bar", type=float, default=0.03, help="mean return per trade needed to open (default 0.03)")
    ap.add_argument("--json", action="store_true")
    a = ap.parse_args(argv)
    p = connect(a.paper)
    prow = closed(p, ("paper",))
    ps = summary(prow)
    since = prow[0]["opened_slot"] if prow else None
    ls = {"n": 0}
    try:
        ls = summary(closed(connect(a.live), REAL, since))
    except sqlite3.Error as e:  # the live db is optional for the verdict
        print(f"live db: {e}", file=sys.stderr)
    out = {"paper_wide": ps, "live_same_window": ls, "paper_open_now": open_count(p, ("paper",)),
           "paper_decisions": decisions(p), "verdict": verdict(ps, a.min_trades, a.bar)}
    if a.json:
        print(json.dumps(out, indent=1))
        return 0
    print(line("paper-wide (unlisted)", ps))
    print(line("live (the table)", ls))
    d = out["paper_decisions"]
    print(f"paper intents {d['paper_intents']}, open now {out['paper_open_now']}, entry outcomes {d['shadow_outcomes']}")
    print(out["verdict"])
    return 0


if __name__ == "__main__":
    sys.exit(main())
