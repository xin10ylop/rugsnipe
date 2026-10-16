#!/usr/bin/env python3
"""Read the docs/04 section 7 go/no-go rule off a shadow-mode state.db (docs/08 step 7), or with --probe
the probe kill rule and stage transitions off a probe state.db (docs/08 section 9).

  .venv/bin/python -I analysis/scripts/shadow_gate.py --state /var/lib/rugsnipe/shadow/state.db \
      [--summary /var/lib/rugsnipe/shadow/summary.json] [--round-trip-lamports 4000000] \
      [--fail-cost-lamports N] [--slot-ms 217] [--last-hours 168] [--json]
  .venv/bin/python -I analysis/scripts/shadow_gate.py --probe --state /var/lib/rugsnipe/state.db \
      [--config /etc/rugsnipe/rugsnipe.toml] [--stage a|b] [--pnl-model data/reports/strategy/pnl_model.json]

The rule (fixed before any shadow data existed):
  (a) over >= 1,000 qualifying launches and >= 1 full day, the shadow EV per attempt (fills at the
      measured premium, failed attempts charged at their cost) has a 90% lower bound above zero
      (bootstrap by hour clusters, docs/06 section 9.5);
  (b) we are ahead of the first follower on at least one hot launch in three at a premium <= +2%.

--probe (docs/04 section 7, [probe] in the config). Probe-A (0.02 SOL) measures landing, premium and the
failed-attempt cost; its EV is expected negative (fixed costs) and that is not a kill reason. Probe-B
(0.25 SOL) trades. Kill when
  * (B only) the EV per attempt, failed attempts included, has a 90% upper bound below 0 after
    kill.min_landed (100) landed trades or kill.min_attempts (400) attempts, whichever first;
  * the median realised premium of the landed trades exceeds kill.max_median_premium (+3%);
  * the landing rate stays below the break-even for the measured failed-attempt cost over
    kill.landing_attempts (200) attempts (break-even: pnl_model.break_even_landing for the 0.25 SOL LEAD at
    the measured mean failed-attempt cost and median premium, f_pay = 1);
  * a day lost more than the stage's daily loss cap (the risk gate latches the kill itself).
Probe-A is complete at 300 attempts or 3 days; probe-B is entered only if A's landing rate is above that
break-even and its median premium is at or below +2%. Probe-B is complete at 200 landed trades; then the
live gate (docs/06 section 9.5) is read.

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
import random
import sqlite3
import sys
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]

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


# a probe attempt: any intent with a fills row (failed: fail_reason set, or a shadow failure outcome)
SQL_PROBE = """
SELECT i.intent_id, i.mint, i.mode, i.slot, i.spend, f.sig, f.sol_delta, f.fail_reason, COALESCE(f.fail_cost, 0) AS fail_cost,
       s.outcome, s.premium_bps, s.create_slot, (SELECT v.variant FROM sends v WHERE v.sig = f.sig) AS variant,
       {leader_class} AS leader_class,
       (SELECT SUM(e.sol_delta) FROM exits e WHERE e.mint = i.mint) AS proceeds,
       (SELECT p.state FROM positions p WHERE p.mint = i.mint) AS pos_state
FROM intents i
JOIN fills f ON f.intent_id = i.intent_id
LEFT JOIN shadow s ON s.intent_id = i.intent_id
WHERE s.outcome IS NULL OR s.outcome IN ('filled', 'premium', 'slippage', 'not_in_slot0')
"""
STAGES = {"a": dict(max_probe_lamports=20_000_000, max_probes_per_day=50, max_open_positions=1,
                    max_daily_loss_lamports=200_000_000),
          "b": dict(max_probe_lamports=250_000_000, max_probes_per_day=300, max_open_positions=2,
                    max_daily_loss_lamports=1_000_000_000)}
KILL = dict(min_landed=100, min_attempts=400, max_median_premium=0.03, landing_attempts=200)
A_DONE_ATTEMPTS, A_DONE_DAYS, B_DONE_LANDED, B_ENTRY_MAX_PREMIUM, B_SIZE = 300, 3.0, 200, 0.02, 0.25


def cluster_boot(xs, clusters, B=4000, seed=7, q=(0.05, 0.95)):
    """Bootstrap of the mean resampling whole clusters (here: UTC hours). Returns (lo, hi)."""
    by: dict = {}
    for x, c in zip(xs, clusters):
        by.setdefault(c, []).append(x)
    groups = [(sum(v), len(v)) for v in by.values()]
    if len(groups) < 2:
        return float("nan"), float("nan")
    rng = random.Random(seed)
    k = len(groups)
    means = []
    for _ in range(B):
        sx = n = 0
        for _ in range(k):
            gs, gn = groups[rng.randrange(k)]
            sx += gs
            n += gn
        means.append(sx / n)
    means.sort()
    return means[int(q[0] * (B - 1))], means[int(q[1] * (B - 1))]


def median(v):
    v = sorted(v)
    n = len(v)
    if not n:
        return None
    return v[n // 2] if n % 2 else (v[n // 2 - 1] + v[n // 2]) / 2


def probe(a) -> int:
    """The probe kill rule and the stage transitions (docs/04 section 7)."""
    sys.path.insert(0, str(REPO / "analysis"))
    from rugsnipe_analysis.strategy import pnl_model as pm

    cfg = tomllib.load(open(a.config, "rb")) if a.config else {}
    pc = cfg.get("probe", {})
    stage = a.stage or pc.get("stage", "a")
    caps = dict(STAGES[stage], **pc.get(stage, {}))
    kill = dict(KILL, **pc.get("kill", {}))
    loss_cap = a.daily_loss_cap_lamports or caps["max_daily_loss_lamports"]
    con = sqlite3.connect(f"file:{a.state}?mode=ro", uri=True)
    con.row_factory = sqlite3.Row
    # schema v5 records the leader class of the landing slot (fills) and of the create slot (intents)
    def has_col(table, col):
        return any(r[1] == col for r in con.execute(f"PRAGMA table_info({table})"))
    if has_col("fills", "leader_class") and has_col("intents", "leader_class"):
        lc = "COALESCE(f.leader_class, i.leader_class)"
    elif has_col("fills", "leader_class"):
        lc = "f.leader_class"
    else:
        lc = "NULL"
    rows = con.execute(SQL_PROBE.format(leader_class=lc)).fetchall()
    if any(r["mode"] == "probe" for r in rows):
        rows = [r for r in rows if r["mode"] == "probe"]      # a probe file: its real attempts only
    ret, slots, landed, prem, fail_costs, open_left = [], [], 0, [], [], 0
    by_variant: dict = {}
    by_class: dict = {}
    for r in rows:
        spend = r["spend"] or 0
        if spend <= 0:
            continue
        failed = (r["outcome"] in FAILED) if r["outcome"] is not None else (r["fail_reason"] is not None)
        slot = r["create_slot"] or r["slot"] or 0
        if failed:
            cost = a.fail_cost_lamports if a.fail_cost_lamports is not None else r["fail_cost"]
            fail_costs.append(cost)
            net = -cost
        else:
            if r["pos_state"] != "closed" or r["proceeds"] is None:
                open_left += 1
                continue
            net = r["proceeds"] + (r["sol_delta"] or 0) - a.round_trip_lamports
            landed += 1
            if r["premium_bps"] is not None:
                prem.append(r["premium_bps"] / 1e4)
        v = by_variant.setdefault(r["variant"] if r["variant"] is not None else "?", [0, 0])
        v[0] += 1
        v[1] += 0 if failed else 1
        k = by_class.setdefault(r["leader_class"] or "unknown", [0, 0, 0])
        k[0] += 1
        k[1] += 0 if failed else 1
        k[2] += cost if failed else 0
        ret.append((net / spend, net))
        slots.append(slot)
    n = len(ret)
    xs = [x for x, _ in ret]
    mean = sum(xs) / n if n else float("nan")
    sd = math.sqrt(sum((x - mean) ** 2 for x in xs) / (n - 1)) if n > 1 else float("nan")
    se = sd / math.sqrt(n) if n > 1 else float("nan")
    lo, hi = mean - 1.645 * se, mean + 1.645 * se
    s0 = min(slots) if slots else 0
    hour = [int((sl - s0) * a.slot_ms / 3.6e6) for sl in slots]
    day = [int((sl - s0) * a.slot_ms / 8.64e7) for sl in slots]
    clo, chi = cluster_boot(xs, hour) if n > 1 else (float("nan"), float("nan"))
    span_days = (max(slots) - s0) * a.slot_ms / 8.64e7 if slots else 0.0
    landing = landed / n if n else 0.0
    fail_mean = sum(fail_costs) / len(fail_costs) if fail_costs else 0.0
    med_prem = median(prem)
    rep = json.load(open(a.pnl_model))
    pol = pm.lead_policy(rep, B_SIZE)
    be = pm.break_even_landing(pol, fail_mean / 1e9, prem=med_prem if med_prem is not None else 0.011,
                               c_rt=a.round_trip_lamports / 1e9, f_pay=1.0)
    day_net: dict = {}
    hour_net: dict = {}
    for (_, net), d, h in zip(ret, day, hour):
        day_net[d] = day_net.get(d, 0) + net
        hour_net[h] = hour_net.get(h, 0) + net
    worst_day = min(day_net.values()) if day_net else 0
    pos = sum(v for v in hour_net.values() if v > 0)
    hour_share = max(hour_net.values()) / pos if pos > 0 else None
    reasons = []
    if stage == "b" and (landed >= kill["min_landed"] or n >= kill["min_attempts"]) and hi < 0:
        reasons.append(f"EV per attempt 90% upper bound {hi:+.2%} < 0 after {landed} landed / {n} attempts")
    if med_prem is not None and med_prem > kill["max_median_premium"]:
        reasons.append(f"median realised premium {med_prem:+.2%} > {kill['max_median_premium']:+.0%}")
    if n >= kill["landing_attempts"] and landing < be:
        reasons.append(f"landing {landing:.1%} < break-even {be:.1%} over {n} attempts")
    if worst_day <= -loss_cap:
        reasons.append(f"a day lost {-worst_day} lamports >= the daily loss cap {loss_cap}")
    if reasons:
        verdict = "KILL: " + "; ".join(reasons)
    elif stage == "a" and (n >= A_DONE_ATTEMPTS or span_days >= A_DONE_DAYS):
        enter = landing > be and med_prem is not None and med_prem <= B_ENTRY_MAX_PREMIUM
        verdict = ("PROBE-A COMPLETE: ENTER PROBE-B" if enter else
                   "PROBE-A COMPLETE: DO NOT ENTER PROBE-B (landing at or below break-even, or median premium "
                   "above +2% or unmeasured): stop")
    elif stage == "b" and landed >= B_DONE_LANDED:
        verdict = "PROBE-B COMPLETE: read the live gate (docs/06 section 9.5)"
    else:
        verdict = f"CONTINUE probe-{stage.upper()}"
    out = {
        "stage": stage, "caps": caps, "kill": kill,
        "attempts": n, "landed_closed": landed, "failed_attempts": len(fail_costs), "open_positions_left_out": open_left,
        "landing_rate": landing, "failed_cost_lamports_mean": fail_mean,
        "median_premium": med_prem, "premiums_measured": len(prem),
        "break_even_landing": be, "break_even_basis": f"pnl_model LEAD {B_SIZE} SOL pooled OOS, f_pay = 1",
        "ev_per_attempt": mean, "ev_90ci": [lo, hi], "ev_90ci_hour_cluster": [clo, chi],
        "days_covered": span_days, "worst_day_net_lamports": worst_day, "daily_loss_cap_lamports": loss_cap,
        "max_hour_share_of_positive_pnl": hour_share,
        "by_variant": {str(k): {"attempts": v[0], "landed": v[1]} for k, v in by_variant.items()},
        "by_leader_class": ({str(k): {"attempts": v[0], "landed": v[1], "landing_rate": v[1] / v[0] if v[0] else 0.0,
                                      "failed_cost_lamports": v[2]} for k, v in by_class.items()}
                            if lc != "NULL" else "not recorded (state.db before schema v5: no leader_class column)"),
        "verdict": verdict,
    }
    if a.json:
        print(json.dumps(out, indent=1, default=float))
        return 0
    print(f"stage               probe-{stage.upper()}  caps {caps}")
    print(f"attempts            {n}  (landed {landed}, failed {len(fail_costs)}, open left out {open_left}); "
          f"{span_days:.2f} days")
    print(f"landing rate        {landing:.1%}  break-even {be:.1%} ({out['break_even_basis']})")
    print(f"failed cost         {fail_mean:.0f} lamports per failed attempt")
    print(f"median premium      {'n/a' if med_prem is None else f'{med_prem:+.2%}'}  ({len(prem)} measured)")
    if n:
        cl = f"[{clo:+.2%}, {chi:+.2%}]" if clo == clo else "n/a (one hour)"
        print(f"EV per attempt      {mean:+.2%}  90% CI [{lo:+.2%}, {hi:+.2%}]  hour-cluster {cl}")
    print(f"worst day net       {worst_day:+d} lamports (daily loss cap {loss_cap})")
    if hour_share is not None:
        print(f"largest hour        {hour_share:.0%} of the positive P&L")
    for k, v in out["by_variant"].items():
        print(f"  variant {k:3s} attempts {v['attempts']}, landed {v['landed']}")
    if isinstance(out["by_leader_class"], dict):
        for k, v in sorted(out["by_leader_class"].items()):
            print(f"  leader {k:10s} attempts {v['attempts']}, landed {v['landed']} ({v['landing_rate']:.1%}), "
                  f"failed cost {v['failed_cost_lamports']} lamports")
    print(f"verdict             {verdict}")
    return 0


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
    ap.add_argument("--probe", action="store_true", help="the probe kill rule and stage transitions (docs/04 section 7)")
    ap.add_argument("--config", help="--probe: the rugsnipe TOML whose [probe] stage / caps / kill to use")
    ap.add_argument("--stage", choices=["a", "b"], help="--probe: override [probe] stage")
    ap.add_argument("--pnl-model", default=str(REPO / "data/reports/strategy/pnl_model.json"),
                    help="--probe: pnl_run.py output with the measured LEAD inputs for the break-even")
    ap.add_argument("--daily-loss-cap-lamports", type=int, help="--probe: override the stage's daily loss cap")
    a = ap.parse_args()
    if a.probe:
        return probe(a)

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
    # rule (a): the 90% lower bound by a bootstrap over UTC-hour clusters (docs/06 section 9.5)
    s0 = min(slots) if slots else 0
    clo, chi = cluster_boot(ret, [int((sl - s0) * a.slot_ms / 3.6e6) for sl in slots]) if n > 1 else (float("nan"),) * 2
    a_ok = n >= 1000 and span_h >= 24 and clo > 0
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
        "ev_90ci_hour_cluster": [clo, chi],
        "hours_covered": span_h,
        "hours_source": ("summary elapsed_s" if (elapsed_h is not None and a.last_hours is None)
                         else f"slot span x {a.slot_ms} ms"),
        "ms_per_slot_measured": ms_per_slot,
        "ev_by_followers": {k: (sum(v) / len(v) if v else None, len(v)) for k, v in by_follow.items()},
        "ev_by_filter_tertile": {str(k): {"n": len(v), "ev": sum(x for x, _ in v) / len(v),
                                          "shadow_landing": sum(f for _, f in v) / len(v)}
                                 for k, v in sorted(by_tert.items())},
        "rule_a": {"need": ">= 1000 attempts, >= 24 h, 90% lower bound (hour-cluster bootstrap) > 0", "pass": a_ok},
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
        cl = f"[{clo:+.2%}, {chi:+.2%}]" if clo == clo else "n/a (one hour)"
        print(f"EV per attempt      {mean:+.2%}  90% CI [{lo:+.2%}, {hi:+.2%}], by hour clusters {cl}")
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
