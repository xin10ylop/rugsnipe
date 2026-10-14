#!/usr/bin/env python3
"""Read the docs/04 section 7 go/no-go rule off a shadow-mode state.db (docs/08 step 7).

  .venv/bin/python -I analysis/scripts/shadow_gate.py --state /var/lib/rugsnipe/shadow/state.db \
      [--summary /var/lib/rugsnipe/shadow/summary.json] [--round-trip-lamports 4000000] \
      [--fail-cost-lamports N] [--slot-ms 217] [--last-hours 168] [--json]

The rule (fixed before any shadow data existed):
  (a) over >= 1,000 qualifying launches and >= 1 full day, the shadow EV per attempt (fills at the
      measured premium, failed attempts charged at their cost) has a 90% lower bound above zero;
  (b) we are ahead of the first follower on at least one hot launch in three at a premium <= +2%.

Per attempt, from the tables the hot box writes (docs/06 section 5.1):
  filled  : sum(exits.sol_delta of the mint) + fills.sol_delta (the stake, negative) - round trip
  failed  : -fills.fail_cost (premium above the cap, slippage, not in slot 0)
EV is net / stake. `--fail-cost-lamports` re-prices every failed attempt (e.g. 3000000 for docs/04's
0.003 SOL case) without re-running. Positions still open at shutdown are left out.

What this build cannot read: part (b) needs our real landing index against the first follower's
index, which needs real sends (probe) or a shred feed. The shadow lane assumes rank 1 right behind
the bundle and measures the premium of third-party buyers inside the bundle run; its "landing rate"
is the share of attempts that are not refused by the cap or the block position, not a measured race.
"""
from __future__ import annotations

import argparse
import json
import math
import sqlite3
import sys

FAILED = ("premium", "slippage", "not_in_slot0")
SQL_ATTEMPTS = """
SELECT s.intent_id, s.mint, s.outcome, s.create_slot, s.premium_bps, s.tp_ahead, s.tp_behind, i.spend,
       f.sol_delta, COALESCE(f.fail_cost, 0) AS fail_cost,
       (SELECT SUM(e.sol_delta) FROM exits e WHERE e.mint = s.mint) AS proceeds,
       (SELECT p.state FROM positions p WHERE p.mint = s.mint) AS pos_state
FROM shadow s
JOIN intents i ON i.intent_id = s.intent_id
JOIN fills f ON f.intent_id = s.intent_id
WHERE s.outcome IN ('filled', 'premium', 'slippage', 'not_in_slot0')
"""


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--state", required=True)
    ap.add_argument("--summary", help="the JSON rugsnipe-hot printed at exit (--summary FILE)")
    ap.add_argument("--round-trip-lamports", type=int, default=4_000_000, help="paper.round_trip_lamports")
    ap.add_argument("--fail-cost-lamports", type=int, help="re-price every failed attempt at this cost")
    ap.add_argument("--slot-ms", type=float, default=217.0,
                    help="slot length that turns the slot span into hours when no --summary is given "
                         "(217 ms measured after Oct 9; re-measure after Alpenglow)")
    ap.add_argument("--last-hours", type=float,
                    help="score only launches of the last H hours of the file (weekly re-check: 168)")
    ap.add_argument("--json", action="store_true")
    a = ap.parse_args()

    con = sqlite3.connect(f"file:{a.state}?mode=ro", uri=True)
    con.row_factory = sqlite3.Row
    outcomes = dict(con.execute("SELECT outcome, COUNT(*) FROM shadow GROUP BY outcome").fetchall())
    rows = con.execute(SQL_ATTEMPTS).fetchall()
    if a.last_hours is not None and rows:
        top = max(r["create_slot"] for r in rows)
        lo_slot = top - int(a.last_hours * 3.6e6 / a.slot_ms)
        rows = [r for r in rows if r["create_slot"] >= lo_slot]
    ret, slots, fills, prem_ok, open_left = [], [], [], 0, 0
    fail_cost_total = 0
    by_follow: dict[str, list[float]] = {"no third-party buyer behind": [], ">= 1 buyer behind": []}
    # entry filter (log mode): the frozen P(TP) tertile of every scored intent, when the build records it
    tert = {}
    if con.execute("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'filter'").fetchone():
        tert = dict(con.execute("SELECT intent_id, tertile FROM filter WHERE tertile IS NOT NULL").fetchall())
    by_tert: dict[int, list] = {}
    for r in rows:
        spend = r["spend"] or 0
        if spend <= 0:
            continue
        if r["outcome"] == "filled":
            if r["pos_state"] != "closed" or r["proceeds"] is None:
                open_left += 1
                continue
            net = r["proceeds"] + r["sol_delta"] - a.round_trip_lamports
            fills.append(r)
            prem_ok += (r["premium_bps"] or 0) <= 200
        else:
            cost = a.fail_cost_lamports if a.fail_cost_lamports is not None else r["fail_cost"]
            fail_cost_total += cost
            net = -cost
        x = net / spend
        ret.append(x)
        slots.append(r["create_slot"])
        by_follow[">= 1 buyer behind" if (r["tp_behind"] or 0) > 0 else "no third-party buyer behind"].append(x)
        if r["intent_id"] in tert:
            by_tert.setdefault(tert[r["intent_id"]], []).append((x, r["outcome"] == "filled"))

    n = len(ret)
    mean = sum(ret) / n if n else float("nan")
    sd = math.sqrt(sum((x - mean) ** 2 for x in ret) / (n - 1)) if n > 1 else float("nan")
    se = sd / math.sqrt(n) if n > 1 else float("nan")
    lo, hi = mean - 1.645 * se, mean + 1.645 * se
    hours = (max(slots) - min(slots)) * a.slot_ms / 3.6e6 if slots else 0.0
    elapsed_h = None
    if a.summary:
        try:
            elapsed_h = json.load(open(a.summary)).get("elapsed_s", 0) / 3600
        except (OSError, ValueError):
            elapsed_h = None
    ms_per_slot = (elapsed_h * 3.6e6 / (max(slots) - min(slots))
                   if (elapsed_h and slots and max(slots) > min(slots) and a.last_hours is None) else None)
    span_h = elapsed_h if (elapsed_h is not None and a.last_hours is None) else hours
    n_failed = sum(1 for r in rows if r["outcome"] in FAILED)
    a_ok = n >= 1000 and span_h >= 24 and lo > 0
    verdict = ("GO on (a)" if a_ok else
               "NOT ENOUGH DATA" if (n < 1000 or span_h < 24) else "NO-GO on (a): stop (no edge)")
    out = {
        "outcomes": outcomes,
        "qualifying_attempts_scored": n,
        "closed_fills": len(fills),
        "failed_attempts": n_failed,
        "open_positions_left_out": open_left,
        "landing_rate_shadow": len(fills) / n if n else None,
        "fills_at_premium_le_2pct": prem_ok,
        "failed_cost_lamports_total": fail_cost_total,
        "failed_cost_lamports_mean": fail_cost_total / n_failed if n_failed else None,
        "ev_per_attempt": mean,
        "ev_90ci": [lo, hi],
        "hours_covered": span_h,
        "hours_source": ("summary elapsed_s" if (elapsed_h is not None and a.last_hours is None)
                         else f"slot span x {a.slot_ms} ms"),
        "ms_per_slot_measured": ms_per_slot,
        "ev_by_followers": {k: (sum(v) / len(v) if v else None, len(v)) for k, v in by_follow.items()},
        "ev_by_filter_tertile": {str(k): {"n": len(v), "ev": sum(x for x, _ in v) / len(v),
                                          "shadow_landing": sum(f for _, f in v) / len(v)}
                                 for k, v in sorted(by_tert.items())},
        "rule_a": {"need": ">= 1000 attempts, >= 24 h, 90% lower bound > 0", "pass": a_ok},
        "rule_b": "not measurable in this build (needs real sends or a shred feed): decided in the probe",
        "verdict": verdict,
    }
    if a.json:
        print(json.dumps(out, indent=1))
        return 0
    print(f"outcomes            {outcomes}")
    print(f"attempts scored     {n}  (fills closed {len(fills)}, failed {n_failed}, open left out {open_left})")
    print(f"hours covered       {span_h:.1f}  ({out['hours_source']})")
    if ms_per_slot:
        print(f"ms per slot         {ms_per_slot:.0f}  (run time / slot span; 217 before Alpenglow)")
    if n:
        print(f"shadow landing      {len(fills) / n:.1%}  (cap/position refusals only, not a measured race)")
        print(f"failed cost         {fail_cost_total} lamports total, "
              f"{(fail_cost_total / n_failed) if n_failed else 0:.0f} per failed attempt")
        print(f"EV per attempt      {mean:+.2%}  90% CI [{lo:+.2%}, {hi:+.2%}]")
        for k, (m, c) in out["ev_by_followers"].items():
            if c:
                print(f"  {k:28s} {m:+.2%}  (n = {c})")
    for k, d in out["ev_by_filter_tertile"].items():
        name = {"0": "bottom (skip)", "1": "middle", "2": "top"}.get(k, k)
        print(f"  P(TP) tertile {name:14s} {d['ev']:+.2%}  landing {d['shadow_landing']:.1%}  (n = {d['n']})")
    print(f"rule (a)            {'PASS' if a_ok else 'not passed'}: {out['rule_a']['need']}")
    print(f"rule (b)            {out['rule_b']}")
    print(f"verdict             {verdict}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
