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

Review fixes (docs/research/probe-design-v2.md): EV at the measured mean premium of the landed fills and
re-priced at +1.1% / +2.4% / +3.6% (pnl_model's EV slope per premium point); hours and UTC-hour clusters from
the wall clock (outcomes.launch_ms, sends.sent_unix_ms; slots x --slot-ms only with a loud warning); a --summary
must match the state.db (started_unix / state path, else its row counts) or the script refuses it; a rule (b)
proxy (EV per landed trade), tertile and buyer-behind splits with hour-cluster CIs and unresolved by phase;
--cluster signer adds a signer-clustered bootstrap. --probe adds the per-UTC-hour attempt cap (--hour-cap, 4),
prices the landing break-even at the mean measured premium (the premium thresholds stay on the median) and, with
--shadow-ev, the break-even landing at the probe stake from the shadow EV per landed trade.

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


PREMS_FIXED = (0.011, 0.024, 0.036)


def warn(msg):
    print(f"WARNING: {msg}", file=sys.stderr)


def has_table(con, name):
    return con.execute("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?", (name,)).fetchone() is not None


def has_col(con, table, col):
    return any(r[1] == col for r in con.execute(f"PRAGMA table_info({table})"))


def kv_get(con, k):
    if not has_table(con, "kv"):
        return None
    r = con.execute("SELECT v FROM kv WHERE k = ?", (k,)).fetchone()
    return r[0] if r else None


def wall_ms(con, ids):
    """Unix ms per intent from the tables that carry wall-clock time: outcomes.launch_ms (the create's block time,
    written for every decided launch) and, on a probe file, the first sends.sent_unix_ms. intents.decided_ns is a
    monotonic clock and is not used."""
    out = {}
    if has_table(con, "outcomes") and has_col(con, "outcomes", "launch_ms"):
        out.update({i: t for i, t in con.execute(
            "SELECT intent_id, launch_ms FROM outcomes WHERE launch_ms IS NOT NULL AND launch_ms > 0")})
    if has_table(con, "sends") and has_col(con, "sends", "sent_unix_ms"):
        for i, t in con.execute("SELECT intent_id, MIN(sent_unix_ms) FROM sends WHERE sent_unix_ms > 0 GROUP BY intent_id"):
            out.setdefault(i, t)
    return {i: out[i] for i in ids if i in out}


def clock(con, ids, slots, slot_ms, what):
    """Per-attempt unix ms (or slot-derived ms) and how they were obtained. Wall clock when at least 90% of the
    attempts carry one (the rest interpolated from a least-squares fit of ms on slot); otherwise slot x slot_ms with a
    loud warning (slot length is not constant: 217 ms measured after Oct 9, about 400 ms nominal, re-measure after
    Alpenglow)."""
    wm = wall_ms(con, ids)
    n = len(ids)
    if n and len(wm) >= 0.9 * n:
        pts = [(sl, wm[i]) for i, sl in zip(ids, slots) if i in wm]
        if len(pts) >= 2 and max(p[0] for p in pts) > min(p[0] for p in pts):
            mx = sum(p[0] for p in pts) / len(pts)
            my = sum(p[1] for p in pts) / len(pts)
            sxx = sum((p[0] - mx) ** 2 for p in pts)
            b = sum((p[0] - mx) * (p[1] - my) for p in pts) / sxx
        else:
            mx, my, b = (pts[0][0], pts[0][1], slot_ms) if pts else (0, 0, slot_ms)
        ms = [wm[i] if i in wm else int(my + b * (sl - mx)) for i, sl in zip(ids, slots)]
        src = f"wall clock (outcomes.launch_ms / sends.sent_unix_ms) on {len(wm)} of {n} attempts"
        return ms, src, (b if len(pts) >= 2 else None), None
    s0 = min(slots) if slots else 0
    st = kv_get(con, "started_unix")
    base = int(st) * 1000 if st and str(st).isdigit() else 0
    ms = [base + int((sl - s0) * slot_ms) for sl in slots]
    w = (f"{what}: hours and UTC-hour clusters come from the slot span x {slot_ms} ms (no wall-clock column on "
         f"{n - len(wm)} of {n} attempts); slot length drifts (217 ms measured after Oct 9, ~400 ms nominal), so the "
         f"hours and the 24 h condition are estimates")
    warn(w)
    return ms, f"slot span x {slot_ms} ms (NO WALL CLOCK)", None, w


def signer_clusters(con, ids):
    """Per-attempt signer key: outcomes.signer, else intents.op_id, else the intent itself. Returns (keys, source)."""
    smap, src = {}, []
    if has_table(con, "outcomes") and has_col(con, "outcomes", "signer"):
        smap = {i: s for i, s in con.execute("SELECT intent_id, signer FROM outcomes WHERE signer IS NOT NULL")}
        src.append("outcomes.signer")
    if len([i for i in ids if i in smap]) < 0.9 * max(len(ids), 1):
        for i, o in con.execute("SELECT intent_id, op_id FROM intents WHERE op_id IS NOT NULL"):
            smap.setdefault(i, f"op{o}")
        src.append("intents.op_id")
    keys = [smap.get(i, f"intent{i}") for i in ids]
    return keys, (" + ".join(src) or "none") + f" ({sum(1 for i in ids if i in smap)} of {len(ids)} attempts; the rest are their own cluster)"


def boot_ci(xs, clusters, B=2000, seed=7):
    """Mean and 90% cluster-bootstrap CI (or NaNs)."""
    n = len(xs)
    if not n:
        return None, float("nan"), float("nan")
    lo, hi = cluster_boot(xs, clusters, B=B, seed=seed) if n > 1 else (float("nan"), float("nan"))
    return sum(xs) / n, lo, hi


def fmt_ci(m, lo, hi):
    if m is None:
        return "n/a"
    return f"{m:+.2%} [{lo:+.2%}, {hi:+.2%}]" if lo == lo else f"{m:+.2%} [n/a]"


def slope_for(pnl_model_path, stake_sol):
    try:
        m = json.load(open(pnl_model_path))["measured"]
        size = min((0.25, 0.5), key=lambda s: abs(s - stake_sol))
        return float(m[f"slope_pooled_S{size}"]), f"pnl_model slope_pooled_S{size}"
    except (OSError, ValueError, KeyError):
        return 1.05, "default 1.05 (pnl_model.json unreadable)"


def check_summary(path, con, state):
    """The --summary file must belong to this state.db: its started_unix (and state path, when it records one) must
    match kv.started_unix and --state; a summary without either is tied by its state_rows (shadow / intents counts).
    Returns the summary dict or exits with a message."""
    try:
        sm = json.load(open(path))
    except (OSError, ValueError) as e:
        raise SystemExit(f"--summary {path}: unreadable ({e}); refusing to use it")
    kv_st = kv_get(con, "started_unix")
    for key in ("state", "state_path"):
        if sm.get(key) and Path(sm[key]).resolve() != Path(state).resolve():
            raise SystemExit(f"--summary {path} names state {sm[key]}, not {state}: refusing (different run)")
    if sm.get("started_unix") is not None:
        if kv_st is None or str(sm["started_unix"]) != str(kv_st):
            raise SystemExit(f"--summary {path}: started_unix {sm['started_unix']} != state.db kv.started_unix "
                             f"{kv_st}: refusing (the summary is from another run)")
        return sm
    rows = dict(sm.get("state_rows") or [])
    if not rows:
        raise SystemExit(f"--summary {path} has neither started_unix nor state_rows, so it cannot be tied to {state}: "
                         "refusing (rugsnipe-hot should print started_unix and state_path in the summary)")
    for t in ("intents", "shadow"):
        if t in rows and has_table(con, t):
            n = con.execute(f"SELECT COUNT(*) FROM {t}").fetchone()[0]
            if n != rows[t]:
                raise SystemExit(f"--summary {path}: state_rows[{t}] = {rows[t]} but {state} has {n}: refusing "
                                 "(different run, or the file grew after the summary)")
    warn(f"--summary {path} has no started_unix: tied to {state} by its row counts only")
    return sm


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
    ret, slots, ids, landed, prem, fail_costs, open_left = [], [], [], 0, [], [], 0
    spends = []
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
        ids.append(r["intent_id"])
        spends.append(spend)
    n = len(ret)
    xs = [x for x, _ in ret]
    mean = sum(xs) / n if n else float("nan")
    sd = math.sqrt(sum((x - mean) ** 2 for x in xs) / (n - 1)) if n > 1 else float("nan")
    se = sd / math.sqrt(n) if n > 1 else float("nan")
    lo, hi = mean - 1.645 * se, mean + 1.645 * se
    # wall clock (sends.sent_unix_ms / outcomes.launch_ms) when the file has it, else slots x slot_ms (warned)
    ms, clock_src, _, clock_warn = clock(con, ids, slots, a.slot_ms, "probe")
    hour = [int(t // 3_600_000) for t in ms]           # UTC hour
    day = [int(t // 86_400_000) for t in ms]           # UTC day
    clo, chi = cluster_boot(xs, hour) if n > 1 else (float("nan"), float("nan"))
    signer, sig_src = signer_clusters(con, ids) if a.cluster == "signer" else (None, None)
    slo, shi = cluster_boot(xs, signer) if (signer and n > 1) else (float("nan"), float("nan"))
    span_days = (max(ms) - min(ms)) / 8.64e7 if ms else 0.0
    landing = landed / n if n else 0.0
    fail_mean = sum(fail_costs) / len(fail_costs) if fail_costs else 0.0
    med_prem = median(prem)
    mean_prem = sum(prem) / len(prem) if prem else None
    rep = json.load(open(a.pnl_model))
    pol = pm.lead_policy(rep, B_SIZE)
    # the break-even is priced at the MEAN measured premium (a mean-premium model input), the kill / entry
    # thresholds on the premium itself stay on the median (docs/04 section 7)
    be = pm.break_even_landing(pol, fail_mean / 1e9, prem=mean_prem if mean_prem is not None else 0.011,
                               c_rt=a.round_trip_lamports / 1e9, f_pay=1.0)
    # (d) per-UTC-hour attempt cap (probe design v2: at most --hour-cap attempts per UTC hour)
    per_hour: dict = {}
    for h in hour:
        per_hour[h] = per_hour.get(h, 0) + 1
    over = {f"{h * 3600}": c for h, c in sorted(per_hour.items()) if c > a.hour_cap}
    hour_cap = {"cap": a.hour_cap, "hours_with_attempts": len(per_hour), "max_per_hour": max(per_hour.values()) if per_hour else 0,
                "hours_over_cap": over, "clock": clock_src}
    # (d) break-even landing at the probe stake from the shadow EV per landed trade (pnl_model.break_even_at_stake)
    stake = (sum(spends) / len(spends) / 1e9) if spends else caps["max_probe_lamports"] / 1e9
    be_shadow = None
    if a.shadow_ev is not None:
        cf = fail_mean / 1e9 if fail_costs else 0.00073
        be_shadow = {"shadow_ev_per_landed": a.shadow_ev, "shadow_stake": a.shadow_stake, "c_fail_sol": cf,
                     "at_probe_stake": {"stake": stake, "break_even": pm.break_even_at_stake(
                         a.shadow_ev, a.shadow_stake, stake, cf, a.round_trip_lamports / 1e9)},
                     "at_0.25": pm.break_even_at_stake(a.shadow_ev, a.shadow_stake, 0.25, cf, a.round_trip_lamports / 1e9),
                     "note": "NaN = a landed trade loses at that stake (fixed cost per SOL), no landing rate breaks even"}
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
        "median_premium": med_prem, "mean_premium": mean_prem, "premiums_measured": len(prem),
        "break_even_landing": be, "break_even_basis": f"pnl_model LEAD {B_SIZE} SOL pooled OOS, f_pay = 1",
        "ev_per_attempt": mean, "ev_90ci": [lo, hi], "ev_90ci_hour_cluster": [clo, chi],
        "ev_90ci_signer_cluster": [slo, shi] if signer else None, "signer_cluster_source": sig_src,
        "days_covered": span_days, "clock": clock_src, "clock_warning": clock_warn,
        "hour_cap": hour_cap, "break_even_from_shadow": be_shadow, "worst_day_net_lamports": worst_day, "daily_loss_cap_lamports": loss_cap,
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
    print(f"median premium      {'n/a' if med_prem is None else f'{med_prem:+.2%}'}  (mean "
          f"{'n/a' if mean_prem is None else f'{mean_prem:+.2%}'}, {len(prem)} measured; break-even priced at the mean)")
    if clock_warn:
        print(f"!!! WARNING         {clock_warn}")
    print(f"hour cap            max {hour_cap['max_per_hour']} attempts in a UTC hour (cap {a.hour_cap}); "
          f"{len(over)} hour(s) over the cap" + (f": {over}" if over else ""))
    if be_shadow:
        b1, b2 = be_shadow["at_probe_stake"]["break_even"], be_shadow["at_0.25"]
        f = lambda v: "none (a landed trade loses)" if v != v else f"{v:.1%}"
        print(f"break-even (shadow) at {stake:g} SOL: {f(b1)}; at 0.25 SOL: {f(b2)}  (shadow EV per landed trade "
              f"{a.shadow_ev:+.2%} at {a.shadow_stake:g} SOL, failed cost {be_shadow['c_fail_sol']:.5f} SOL)")
    if n:
        cl = f"[{clo:+.2%}, {chi:+.2%}]" if clo == clo else "n/a (one hour)"
        print(f"EV per attempt      {mean:+.2%}  90% CI [{lo:+.2%}, {hi:+.2%}]  hour-cluster {cl}")
        if signer:
            print(f"                    signer-cluster [{slo:+.2%}, {shi:+.2%}]  ({sig_src})")
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
    ap.add_argument("--cluster", choices=["hour", "signer"], default="hour",
                    help="also bootstrap by signer clusters (outcomes.signer, else intents.op_id) next to the hour one")
    ap.add_argument("--hour-cap", type=int, default=4,
                    help="--probe: flag UTC hours with more than N attempts (probe design v2 thinning, default 4)")
    ap.add_argument("--shadow-ev", type=float,
                    help="--probe: the shadow run's EV per landed trade (fraction, e.g. 0.042) for the break-even "
                         "landing at the probe stake (pnl_model.break_even_at_stake)")
    ap.add_argument("--shadow-stake", type=float, default=0.5, help="--probe: stake (SOL) the --shadow-ev was measured at")
    a = ap.parse_args()
    if a.probe:
        return probe(a)

    con = sqlite3.connect(f"file:{a.state}?mode=ro", uri=True)
    con.row_factory = sqlite3.Row
    outcomes = dict(con.execute("SELECT outcome, COUNT(*) FROM shadow GROUP BY outcome").fetchall())
    summary = check_summary(a.summary, con, a.state) if a.summary else None
    rows = con.execute(SQL_ATTEMPTS).fetchall()
    if a.last_hours is not None and rows:
        top = max(r["create_slot"] for r in rows)
        lo_slot = top - int(a.last_hours * 3.6e6 / a.slot_ms)
        rows = [r for r in rows if r["create_slot"] >= lo_slot]
    ret, slots, ids, fills, prem_ok, open_left = [], [], [], [], 0, 0
    fail_cost_total = 0
    landed_x, landed_prem, landed_idx = [], [], []   # per closed fill: return, premium, index into ret
    behind = []                                       # per attempt: >= 1 third-party buyer behind
    # entry filter (log mode): the frozen P(TP) tertile of every scored intent, when the build records it
    tert = {}
    if has_table(con, "filter"):
        tert = dict(con.execute("SELECT intent_id, tertile FROM filter WHERE tertile IS NOT NULL").fetchall())
    by_tert: dict[int, list] = {}
    tert_idx: dict[int, list] = {}
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
        if r["outcome"] == "filled":
            landed_x.append(x)
            landed_prem.append((r["premium_bps"] or 0) / 1e4)
            landed_idx.append(len(ret))
        ret.append(x)
        slots.append(r["create_slot"])
        ids.append(r["intent_id"])
        behind.append((r["tp_behind"] or 0) > 0)
        if r["intent_id"] in tert:
            by_tert.setdefault(tert[r["intent_id"]], []).append((x, r["outcome"] == "filled"))
            tert_idx.setdefault(tert[r["intent_id"]], []).append(len(ret) - 1)

    n = len(ret)
    mean = sum(ret) / n if n else float("nan")
    sd = math.sqrt(sum((x - mean) ** 2 for x in ret) / (n - 1)) if n > 1 else float("nan")
    se = sd / math.sqrt(n) if n > 1 else float("nan")
    lo, hi = mean - 1.645 * se, mean + 1.645 * se
    # wall clock (outcomes.launch_ms) when the file has it; else the summary's elapsed_s; else slots x slot_ms
    ms, clock_src, ms_slot_fit, clock_warn = clock(con, ids, slots, a.slot_ms, "shadow")
    wall = clock_warn is None
    elapsed_h = summary.get("elapsed_s", 0) / 3600 if summary and summary.get("elapsed_s") else None
    if wall:
        span_h = (max(ms) - min(ms)) / 3.6e6 if ms else 0.0
        hours_source = clock_src
    elif elapsed_h is not None and a.last_hours is None:
        span_h, hours_source = elapsed_h, "summary elapsed_s (checked against this state.db)"
    else:
        span_h, hours_source = ((max(slots) - min(slots)) * a.slot_ms / 3.6e6 if slots else 0.0), clock_src
    ms_per_slot = (ms_slot_fit if ms_slot_fit is not None else
                   (elapsed_h * 3.6e6 / (max(slots) - min(slots))
                    if (elapsed_h and slots and max(slots) > min(slots) and a.last_hours is None) else None))
    n_failed = sum(1 for r in rows if r["outcome"] in FAILED)
    hour = [int(t // 3_600_000) for t in ms]       # UTC hour (wall clock) or slot-derived hour
    # rule (a): the 90% lower bound by a bootstrap over UTC-hour clusters (docs/06 section 9.5)
    clo, chi = cluster_boot(ret, hour) if n > 1 else (float("nan"),) * 2
    # signer clusters (one operator's launches are not independent): outcomes.signer, else intents.op_id
    sig_src, signer = None, None
    if a.cluster == "signer":
        signer, sig_src = signer_clusters(con, ids)
    slo, shi = cluster_boot(ret, signer) if (signer and n > 1) else (float("nan"),) * 2
    # (a) premium: the fills as measured (mean premium of the landed fills) and re-priced at fixed premiums with the
    # measured EV slope per premium point (pnl_model)
    stake_sol = (sum(r["spend"] for r in fills) / len(fills) / 1e9) if fills else 0.5
    slope, slope_src = slope_for(a.pnl_model, stake_sol)
    mean_prem = sum(landed_prem) / len(landed_prem) if landed_prem else None
    sens = {}
    for p in PREMS_FIXED:
        xs = list(ret)
        for i, pr in zip(landed_idx, landed_prem):
            xs[i] = ret[i] - slope * (p - pr)
        m, l_, h_ = boot_ci(xs, hour)
        sens[f"{p:.3f}"] = {"ev": m, "ci90_hour_cluster": [l_, h_]}
    a_ok = n >= 1000 and span_h >= 24 and clo > 0
    verdict = ("GO on (a)" if a_ok else
               "NOT ENOUGH DATA" if (n < 1000 or span_h < 24) else "NO-GO on (a): stop (no edge)")
    # (c) rule (b) proxy: EV per landed trade; splits with hour-cluster CIs
    lhour = [hour[i] for i in landed_idx]
    lm, llo, lhi = boot_ci(landed_x, lhour)
    hot = [i for i in range(n) if behind[i]]
    hot_fills = [i for i in landed_idx if behind[i]]
    ahead_ok = sum(1 for r in fills if (r["tp_behind"] or 0) > 0 and (r["tp_ahead"] or 0) == 0
                   and (r["premium_bps"] or 0) <= 200)
    split = {}
    for name, sel in (("no third-party buyer behind", [i for i in range(n) if not behind[i]]),
                      (">= 1 buyer behind", hot)):
        m, l_, h_ = boot_ci([ret[i] for i in sel], [hour[i] for i in sel])
        split[name] = {"n": len(sel), "ev": m, "ci90_hour_cluster": [l_, h_]}
    tert_ci = {}
    for k, sel in sorted(tert_idx.items()):
        m, l_, h_ = boot_ci([ret[i] for i in sel], [hour[i] for i in sel])
        tert_ci[str(k)] = {"n": len(sel), "ev": m, "ci90_hour_cluster": [l_, h_]}
    if 0 in tert_idx:
        keep = [i for k, sel in tert_idx.items() if k != 0 for i in sel]
        m, l_, h_ = boot_ci([ret[i] for i in keep], [hour[i] for i in keep])
        tert_ci["skip_bottom (1+2)"] = {"n": len(keep), "ev": m, "ci90_hour_cluster": [l_, h_]}
    # (c) unresolved by phase (shadow rows; crates/rugsnipe/src/e1.rs): slot 0 never read (create not in history,
    # slot-0 tx unreadable, lane timeout) leaves create_index NULL; a cut replay tape keeps the slot-0 facts
    unres = {}
    if outcomes.get("unresolved"):
        err_col = next((c for c in ("err", "error", "phase", "fail_phase") if has_col(con, "shadow", c)), None)
        q = (f"SELECT COALESCE({err_col}, '?') AS ph, COUNT(*) FROM shadow WHERE outcome = 'unresolved' GROUP BY ph"
             if err_col else
             "SELECT CASE WHEN create_index IS NULL THEN 'slot 0 not read (create not in history / slot-0 tx "
             "unreadable / lane timeout)' ELSE 'slot 0 read, replay tape cut (RPC budget or errors)' END AS ph, "
             "COUNT(*) FROM shadow WHERE outcome = 'unresolved' GROUP BY ph")
        resolved = sum(v for k, v in outcomes.items() if k not in ("unresolved", "class"))
        unres = {
            "n": outcomes["unresolved"],
            "share_of_entry_class": outcomes["unresolved"] / (outcomes["unresolved"] + resolved) if resolved else None,
            "by_phase": dict(con.execute(q).fetchall()),
            "with_tp_ahead": con.execute("SELECT COUNT(*) FROM shadow WHERE outcome = 'unresolved' "
                                         "AND COALESCE(tp_ahead, 0) > 0").fetchone()[0],
            "with_tp_behind": con.execute("SELECT COUNT(*) FROM shadow WHERE outcome = 'unresolved' "
                                          "AND COALESCE(tp_behind, 0) > 0").fetchone()[0],
            "by_model": dict(con.execute("SELECT COALESCE(model, '?'), COUNT(*) FROM shadow WHERE outcome = "
                                         "'unresolved' GROUP BY 1").fetchall()),
            "phase_column": err_col or ("none: the Rust side should persist E1Res.error as shadow.err TEXT "
                                        "(the E1 lane's failure: 'create signature not in the curve's history', "
                                        "'slot-0 transaction not readable yet', 'replay tape cut at slot s+k', RPC "
                                        "errors); only the summary's first 8 e1_errors carry it today"),
        }
        if summary and summary.get("cold", {}).get("e1_errors"):
            unres["summary_e1_errors_first8"] = summary["cold"]["e1_errors"]
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
        "ev_90ci_signer_cluster": [slo, shi] if signer else None,
        "signer_cluster_source": sig_src,
        "mean_premium_landed": mean_prem,
        "ev_at_measured_premium": {"mean_premium": mean_prem, "ev": mean, "ci90_hour_cluster": [clo, chi]},
        "ev_at_fixed_premium": sens,
        "premium_slope": {"slope": slope, "source": slope_src, "stake_sol": stake_sol},
        "hours_covered": span_h,
        "hours_source": hours_source,
        "hours_warning": clock_warn,
        "ms_per_slot_measured": ms_per_slot,
        "ev_by_followers": {k: (sum(ret[i] for i in sel) / len(sel) if sel else None, len(sel))
                            for k, sel in (("no third-party buyer behind", [i for i in range(n) if not behind[i]]),
                                           (">= 1 buyer behind", hot))},
        "ev_by_followers_ci": split,
        "ev_by_filter_tertile": {str(k): {"n": len(v), "ev": sum(x for x, _ in v) / len(v),
                                          "shadow_landing": sum(f for _, f in v) / len(v)}
                                 for k, v in sorted(by_tert.items())},
        "ev_by_filter_tertile_ci": tert_ci,
        "unresolved": unres or None,
        "rule_a": {"need": ">= 1000 attempts, >= 24 h, 90% lower bound (hour-cluster bootstrap) > 0", "pass": a_ok},
        "rule_b": "not measurable in this build (needs real sends or a shred feed): decided in the probe",
        "rule_b_proxy": {
            "ev_per_landed_trade": lm, "ci90_hour_cluster": [llo, lhi], "landed": len(landed_x),
            "hot_fills": len(hot_fills), "hot_fills_rank1_at_le_2pct": ahead_ok,
            "share_hot_fills_rank1_at_le_2pct": ahead_ok / len(hot_fills) if hot_fills else None,
            "note": ("shadow assumes rank 1 behind the bundle (tp_ahead = 0 by construction): the share is an upper "
                     "bound; the per-landed-trade EV is the number the probe's landing rate multiplies"),
        },
        "verdict": verdict,
    }
    if a.json:
        print(json.dumps(out, indent=1))
        return 0
    print(f"outcomes            {outcomes}")
    print(f"attempts scored     {n}  (fills closed {len(fills)}, failed {n_failed}, open left out {open_left})")
    print(f"hours covered       {span_h:.1f}  ({out['hours_source']})")
    if clock_warn:
        print(f"!!! WARNING         {clock_warn}")
    if ms_per_slot:
        print(f"ms per slot         {ms_per_slot:.0f}  (run time / slot span; 217 before Alpenglow)")
    if n:
        print(f"shadow landing      {len(fills) / n:.1%}  (cap/position refusals only, not a measured race)")
        print(f"failed cost         {fail_cost_total} lamports total, "
              f"{(fail_cost_total / n_failed) if n_failed else 0:.0f} per failed attempt")
        cl = f"[{clo:+.2%}, {chi:+.2%}]" if clo == clo else "n/a (one hour)"
        print(f"EV per attempt      {mean:+.2%}  90% CI [{lo:+.2%}, {hi:+.2%}], by hour clusters {cl}")
        if signer:
            print(f"  by signer clusters [{slo:+.2%}, {shi:+.2%}]  ({sig_src})")
        mp = "n/a" if mean_prem is None else f"{mean_prem:+.2%}"
        print(f"premium             measured mean {mp} on {len(landed_prem)} fills; EV re-priced at fixed premiums "
              f"(slope {slope:.2f}, {slope_src}):")
        print("  " + "  ".join(f"{float(k):+.1%}: {fmt_ci(v['ev'], *v['ci90_hour_cluster'])}" for k, v in sens.items()))
        for k, (m, c) in out["ev_by_followers"].items():
            if c:
                s = split[k]
                print(f"  {k:28s} {m:+.2%}  (n = {c})  90% CI [{s['ci90_hour_cluster'][0]:+.2%}, {s['ci90_hour_cluster'][1]:+.2%}]")
    for k, d in out["ev_by_filter_tertile"].items():
        name = {"0": "bottom (skip)", "1": "middle", "2": "top"}.get(k, k)
        c = tert_ci.get(k)
        print(f"  P(TP) tertile {name:14s} {d['ev']:+.2%}  landing {d['shadow_landing']:.1%}  (n = {d['n']})"
              + (f"  90% CI [{c['ci90_hour_cluster'][0]:+.2%}, {c['ci90_hour_cluster'][1]:+.2%}]" if c else ""))
    if "skip_bottom (1+2)" in tert_ci:
        c = tert_ci["skip_bottom (1+2)"]
        print(f"  skip bottom tertile          {fmt_ci(c['ev'], *c['ci90_hour_cluster'])}  (n = {c['n']})")
    if unres:
        sh = unres["share_of_entry_class"]
        print(f"unresolved          {unres['n']}" + (f" ({sh:.1%} of the entry class)" if sh is not None else "")
              + f"; {unres['with_tp_ahead']} with tp_ahead > 0, {unres['with_tp_behind']} with tp_behind > 0")
        for k, v in unres["by_phase"].items():
            print(f"  {v:6d}  {k}")
        if unres["phase_column"].startswith("none"):
            print(f"  phase column: {unres['phase_column']}")
    print(f"rule (a)            {'PASS' if a_ok else 'not passed'}: {out['rule_a']['need']}")
    print(f"rule (b)            {out['rule_b']}")
    rb = out["rule_b_proxy"]
    sh = rb["share_hot_fills_rank1_at_le_2pct"]
    print(f"rule (b) proxy      EV per landed trade {fmt_ci(lm, llo, lhi)} (n = {len(landed_x)}); hot fills at rank 1 "
          f"<= +2%: {ahead_ok}/{len(hot_fills)}" + (f" ({sh:.0%}, need >= 33%; assumed rank 1)" if sh is not None else ""))
    print(f"verdict             {verdict}")
    return 0

if __name__ == "__main__":
    sys.exit(main())
