#!/usr/bin/env python3
"""Read the docs/04 section 7 go/no-go rule off a shadow-mode state.db (docs/08 step 7), or with --probe
the probe kill rule and stage transitions off a probe state.db (docs/08 section 9).

  .venv/bin/python -I analysis/scripts/shadow_gate.py --state /var/lib/rugsnipe/shadow/state.db \
      [--summary /var/lib/rugsnipe/shadow/summary.json] [--round-trip-lamports 4000000] \
      [--fail-cost-lamports N] [--slot-ms 217] [--last-hours 168] [--json]
  .venv/bin/python -I analysis/scripts/shadow_gate.py --probe --state /var/lib/rugsnipe/state.db \
      [--config /etc/rugsnipe/rugsnipe.toml] [--stage a|b] [--pnl-model data/reports/strategy/pnl_model.json] \
      [--hour-cap N] [--hard-stop-days 10] [--shadow-ev 0.042] [--unfiltered-ev 0.0132] [--late-ok 1]

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
The landing kill is read on all attempts and, once it has kill.landing_attempts of its own, on the in-class
subset too. Probe-B is complete at 200 landed trades; then the live gate (docs/06 section 9.5) is read.

Probe-A, v2 rules (docs/research/probe-design-v2.md section 2 with the readiness audit's M4 block there: probe A
buys every class-eligible create unfiltered, because this build cannot select bundled launches before it sends):
  * complete at >= 350 attempts AND >= 7 days, with >= kill.landing_attempts (200) attempts in the in-class subset
    for the stage-B read; hard stop at --hard-stop-days (default 10; longer when the owner extends stage A in
    calendar days, M4 option (i)). At the hard stop the B read is made on what the subset holds, except that a
    subset still short of 200 attempts with every other stage-B condition met (or with no in-class attempt at all)
    is INCONCLUSIVE: too little data is not a negative result, so the verdict says "escalate to the owner", never
    "stop";
  * in-class subset = attempts whose launch has feed_bundle.bundle_wallets >= 1 (grpc.bundle_run = true writes a
    feed_bundle row for every attempt); landing, EV and premium are printed for all attempts and for the subset;
  * landed = a fill at the creation slot + 0..N (--late-ok N, default 1: at 200 ms slots a slot+1 fill did no worse
    than a same-slot one) at a measured premium of +2% or less. The slot+1..+N fills are the landed_late bucket; the
    strict landing rate (slot 0 only) is printed next to the inclusive one, which the kill and the stage-B read use.
    Fills beyond the window (slot+N+1 and later; or, when the fill slot is unknown, after the nonce burn started:
    intents.state = 'late_fill', fills.late) are reported as late fills, fills above +2% apart (slot-0 and
    slot+1..+N separately); neither counts as landed, both stay in the denominator. The burn flag alone does not
    make a fill with a known slot late (at 200 ms slots the burn starts at ~s+3 and the next 400 ms poll flags
    s+1..s+3 fills too). --late-ok 0 is the strict creation-slot rule. (The real lane writes a missing premium as
    0 bps, so an unmeasured premium on a real fill reads as +0%.);
  * failures split by fills.fail_reason: slippage / premium / not_in_slot0 / dropped are lost races; anything else
    (instruction_error such as ComputationalBudgetExceeded, custom program errors) is a fault of the transaction and
    leaves the landing denominator; above 10% of attempts (and at least 5) the verdict is CHECK, with no landing
    kill and no stage-B entry, instead of a false "stop";
  * stage-B entry, on the in-class subset: landing above the bar = max(pooled LEAD break-even at the subset's mean
    failed cost and mean premium, ~9.9%; the unfiltered shadow EV's break-even at 0.25 SOL, ~36% from +1.32%,
    --unfiltered-ev), median premium at or below +2%. Both inputs are printed;
  * by_sender: per sender label, the buy variants handed over (sends), accepted, send errors, and the fills that
    landed through it (fills.sender).

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
--cluster signer adds a signer-clustered bootstrap. --probe adds the per-UTC-hour attempt cap (--hour-cap; default the
--config's risk.max_buys_per_hour, which the risk gate enforces, else the design's 4), prices the landing break-even at
the mean measured premium
(the premium thresholds stay on the median) and, with --shadow-ev, the break-even landing at the probe stake from
the shadow EV per landed trade.

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
SELECT i.intent_id, i.mint, i.mode, i.slot, i.spend, i.state AS istate, f.sig, f.slot AS fill_slot, f.late,
       f.sol_delta, f.fail_reason, COALESCE(f.fail_cost, 0) AS fail_cost,
       s.outcome, s.premium_bps, s.create_slot, (SELECT v.variant FROM sends v WHERE v.sig = f.sig) AS variant,
       {leader_class} AS leader_class, {sender} AS sender, {bundle_wallets} AS bundle_wallets,
       (SELECT SUM(e.sol_delta) FROM exits e WHERE e.mint = i.mint) AS proceeds,
       (SELECT p.state FROM positions p WHERE p.mint = i.mint) AS pos_state
FROM intents i
JOIN fills f ON f.intent_id = i.intent_id
LEFT JOIN shadow s ON s.intent_id = i.intent_id
{feed_bundle_join}
WHERE s.outcome IS NULL OR s.outcome IN ('filled', 'premium', 'slippage', 'not_in_slot0')
"""
STAGES = {"a": dict(max_probe_lamports=20_000_000, max_probes_per_day=50, max_open_positions=1,
                    max_daily_loss_lamports=200_000_000),
          "b": dict(max_probe_lamports=250_000_000, max_probes_per_day=300, max_open_positions=2,
                    max_daily_loss_lamports=1_000_000_000)}
KILL = dict(min_landed=100, min_attempts=400, max_median_premium=0.03, landing_attempts=200)
# probe design v2 (docs/research/probe-design-v2.md section 2, readiness audit M4): stage A ends at >= 350
# attempts AND >= 7 days (and the in-class subset needs kill.landing_attempts attempts for the B read), hard stop
# at 10 days by default (--hard-stop-days); a fill counts as landed at the creation slot + 0..--late-ok at a premium of
# +2% or less
A_DONE_ATTEMPTS, A_DONE_DAYS, A_HARD_STOP_DAYS = 350, 7.0, 10.0
B_DONE_LANDED, B_ENTRY_MAX_PREMIUM, B_SIZE = 200, 0.02, 0.25
LANDED_MAX_PREMIUM_BPS = 200
# --late-ok N: a fill at create_slot + k with 1 <= k <= N counts as landed (bucket landed_late); 0 = slot 0 only.
# Default 1: stage A measured one-slot-late fills doing no worse than same-slot fills at 200 ms slots
LATE_OK_DEFAULT = 1
LANDED = ("landed", "landed_late")          # the fill classes the landing rate counts (strict, inside the window)
# the unfiltered shadow EV that sets stage B's second bar: the 14-hour Ohio shadow run's +1.32% per attempt,
# used as if per landed trade at 0.5 SOL (probe-design-v2.md section 4: 36.0% break-even at 0.25 SOL)
UNFILTERED_SHADOW_EV = 0.0132
# failure classes (fills.fail_reason, else the shadow outcome): a lost race is a fill refused by our price cap,
# a landing behind the creation slot's block position, or nothing landed (dropped). Everything else (a compute
# budget or instruction error, another program error) is a fault of the transaction, not a lost race
RACE_REASONS = ("slippage", "premium", "not_in_slot0", "dropped")
# more than this share of attempts failing on non-race errors (and at least NONRACE_MIN of them) means the
# transaction is broken (e.g. tx.cu_limit too low for a fresh wallet): no landing kill and no stage-B entry
NONRACE_MAX_SHARE, NONRACE_MIN = 0.10, 5


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


def _mean_ci(xs, clusters):
    """Mean, normal 90% CI and hour-cluster 90% CI of the per-attempt returns."""
    n = len(xs)
    nan = float("nan")
    mean = sum(xs) / n if n else nan
    sd = math.sqrt(sum((x - mean) ** 2 for x in xs) / (n - 1)) if n > 1 else nan
    se = sd / math.sqrt(n) if n > 1 else nan
    clo, chi = cluster_boot(xs, clusters) if n > 1 else (nan, nan)
    return mean, (mean - 1.645 * se, mean + 1.645 * se), (clo, chi)


def late_key(off):
    """Bucket of a late fill: slot+1..+6 by offset, beyond it, or 'burn started' for a fill whose slot
    is unknown and that only the burn flag calls late. Fills inside the --late-ok window never reach it."""
    return f"slot+{off}" if 1 <= off <= 6 else ("slot+7 or later" if off > 6 else "burn started")


def probe_stats(recs):
    """Landing, failures, premium and EV of a set of probe attempts (the records built in probe()).

    landed = a fill at the creation slot + 0..N (--late-ok N) at a measured premium of +2% or less, the only fills
    the landing rate counts. `landed` is INCLUSIVE (`landed_strict`, the slot-0 fills, plus `landed_late`, the
    slot+1..+N ones, split in `landed_late_by_offset`) and so is `landing_rate`, the verdict's number;
    `landing_rate_strict` counts the slot-0 fills alone. Late fills (beyond the window, or a fill whose slot is
    unknown after the nonce burn started: intents.state = 'late_fill', fills.late), fills above +2% (slot-0 and
    slot+1..+N apart) and fills without a measured premium are reported apart and stay in the denominator (they
    raced and did not land in the measured sense). `flagged_late_within_window` counts the landed fills that carry
    the burn flag anyway (the flag over-reports at 200 ms slots). The denominator leaves out failures that are not
    lost races (RACE_REASONS): a compute or instruction error is a fault of the transaction."""
    n = len(recs)
    fills = [r for r in recs if not r["failed"]]
    landed_strict = sum(1 for r in fills if r["fill"] == "landed")
    landed_late = sum(1 for r in fills if r["fill"] == "landed_late")
    landed = landed_strict + landed_late
    late: dict = {}
    by_off: dict = {}
    for r in fills:
        if r["fill"] == "late":
            k = late_key(r["offset"])
            late[k] = late.get(k, 0) + 1
        elif r["fill"] == "landed_late":
            k = f"slot+{r['offset']}"
            by_off[k] = by_off.get(k, 0) + 1
    fails = [r for r in recs if r["failed"]]
    by_reason: dict = {}
    for r in fails:
        v = by_reason.setdefault(r["reason"], {"count": 0, "cost_lamports": 0, "race": r["race"]})
        v["count"] += 1
        v["cost_lamports"] += r["cost"]
    nonrace = sum(1 for r in fails if not r["race"])
    raced = n - nonrace
    prem = [r["premium"] for r in fills if r["premium"] is not None]
    xs = [r["x"] for r in recs]
    mean, ci, cci = _mean_ci(xs, [r["hour"] for r in recs])
    return {
        "attempts": n, "landed": landed, "landed_strict": landed_strict, "landed_late": landed_late,
        "landed_late_by_offset": dict(sorted(by_off.items(), key=lambda kv: int(kv[0].split("+")[1]))),
        "fills": len(fills),
        "late_fills": dict(sorted(late.items()), total=sum(late.values())),
        "slot0_fills_over_premium": sum(1 for r in fills if r["fill"] == "over_premium" and r["offset"] <= 0),
        "late_fills_over_premium": sum(1 for r in fills if r["fill"] == "over_premium" and r["offset"] >= 1),
        "fills_premium_unmeasured": sum(1 for r in fills if r["fill"] == "unmeasured"),
        "flagged_late_within_window": sum(1 for r in fills if r["fill"] in LANDED and r["flagged_late"]),
        "failed": len(fails), "race_failures": len(fails) - nonrace, "nonrace_failures": nonrace,
        "by_fail_reason": dict(sorted(by_reason.items(), key=lambda kv: -kv[1]["count"])),
        "raced": raced, "landing_rate": landed / raced if raced else 0.0,
        "landing_rate_strict": landed_strict / raced if raced else 0.0,
        "failed_cost_lamports_mean": sum(r["cost"] for r in fails) / len(fails) if fails else 0.0,
        "median_premium": median(prem), "mean_premium": sum(prem) / len(prem) if prem else None,
        "premiums_measured": len(prem),
        "ev_per_attempt": mean, "ev_90ci": list(ci), "ev_90ci_hour_cluster": list(cci),
    }


def fmt_pct(v, signed=False):
    if v is None or v != v:
        return "n/a"
    return f"{v:+.2%}" if signed else f"{v:.1%}"


def probe(a) -> int:
    """The probe kill rule and the stage transitions (docs/04 section 7, probe design v2)."""
    sys.path.insert(0, str(REPO / "analysis"))
    from rugsnipe_analysis.strategy import pnl_model as pm

    cfg = tomllib.load(open(a.config, "rb")) if a.config else {}
    pc = cfg.get("probe", {})
    stage = a.stage or pc.get("stage", "a")
    caps = dict(STAGES[stage], **pc.get(stage, {}))
    kill = dict(KILL, **pc.get("kill", {}))
    # the hour cap to check against: the flag, else the cap the risk gate enforces (the config's), else the design's
    if a.hour_cap is not None:
        cap_h, cap_src = a.hour_cap, "--hour-cap"
    elif "max_buys_per_hour" in cfg.get("risk", {}):
        cap_h, cap_src = int(cfg["risk"]["max_buys_per_hour"]), "the config's risk.max_buys_per_hour"
    else:
        cap_h, cap_src = 4, "default: probe design v2"
    hard = a.hard_stop_days
    loss_cap = a.daily_loss_cap_lamports or caps["max_daily_loss_lamports"]
    con = sqlite3.connect(f"file:{a.state}?mode=ro", uri=True)
    con.row_factory = sqlite3.Row
    # schema v5 records the leader class of the landing slot (fills) and of the create slot (intents)
    if has_col(con, "fills", "leader_class") and has_col(con, "intents", "leader_class"):
        lc = "COALESCE(f.leader_class, i.leader_class)"
    elif has_col(con, "fills", "leader_class"):
        lc = "f.leader_class"
    else:
        lc = "NULL"
    snd = "(SELECT v.sender FROM sends v WHERE v.sig = f.sig)"
    if has_col(con, "fills", "sender"):
        snd = f"COALESCE(f.sender, {snd})"
    fb = has_table(con, "feed_bundle")
    rows = con.execute(SQL_PROBE.format(
        leader_class=lc, sender=snd, bundle_wallets="fb.bundle_wallets" if fb else "NULL",
        feed_bundle_join="LEFT JOIN feed_bundle fb ON fb.mint = i.mint" if fb else "")).fetchall()
    if any(r["mode"] == "probe" for r in rows):
        rows = [r for r in rows if r["mode"] == "probe"]      # a probe file: its real attempts only
    recs, open_left = [], 0
    for r in rows:
        spend = r["spend"] or 0
        if spend <= 0:
            continue
        failed = (r["outcome"] in FAILED) if r["outcome"] is not None else (r["fail_reason"] is not None)
        cslot = r["create_slot"] if r["create_slot"] is not None else r["slot"]
        rec = {"id": r["intent_id"], "slot": cslot or 0, "spend": spend, "failed": failed,
               "variant": r["variant"] if r["variant"] is not None else "?",
               "leader_class": r["leader_class"] or "unknown", "sender": r["sender"] or "?",
               "in_class": None if r["bundle_wallets"] is None else r["bundle_wallets"] >= 1,
               "premium": None, "fill": None, "offset": 0, "flagged_late": False, "race": True, "reason": None,
               "cost": 0}
        if failed:
            rec["cost"] = a.fail_cost_lamports if a.fail_cost_lamports is not None else r["fail_cost"]
            rec["reason"] = r["fail_reason"] or r["outcome"] or "unknown"
            rec["race"] = rec["reason"] in RACE_REASONS
            net = -rec["cost"]
        else:
            if r["pos_state"] != "closed" or r["proceeds"] is None:
                open_left += 1
                continue
            net = r["proceeds"] + (r["sol_delta"] or 0) - a.round_trip_lamports
            off = (r["fill_slot"] - cslot) if (r["fill_slot"] is not None and cslot) else 0
            rec["offset"] = off
            if r["premium_bps"] is not None:
                rec["premium"] = r["premium_bps"] / 1e4
            # offset first: fills.slot - shadow.create_slot is the measured number. The burn flag (fills.late /
            # intents.state 'late_fill': the nonce burn had started when the fill was seen) decides only when the
            # fill slot is unknown, because at 200 ms slots the burn starts at ~s+3 and the next 400 ms poll flags
            # s+1..s+3 fills too. The +2% cap applies inside the window exactly as at slot 0 (the premium is
            # measured over the slot-0 post-bundle price, which is what the EV model prices)
            known = r["fill_slot"] is not None and bool(cslot)
            flagged = bool(r["late"]) or r["istate"] == "late_fill"
            rec["flagged_late"] = flagged
            if (known and off > a.late_ok) or (not known and flagged):
                rec["fill"] = "late"            # beyond the window (late_key(off)), or slot unknown after the burn
            elif r["premium_bps"] is None:
                rec["fill"] = "unmeasured"
            elif r["premium_bps"] > LANDED_MAX_PREMIUM_BPS:
                rec["fill"] = "over_premium"
            else:
                rec["fill"] = "landed" if off <= 0 else "landed_late"
        rec["net"], rec["x"] = net, net / spend
        recs.append(rec)
    n = len(recs)
    ids = [r["id"] for r in recs]
    # wall clock (sends.sent_unix_ms / outcomes.launch_ms) when the file has it, else slots x slot_ms (warned)
    ms, clock_src, _, clock_warn = clock(con, ids, [r["slot"] for r in recs], a.slot_ms, "probe")
    for r, t in zip(recs, ms):
        r["hour"], r["day"] = int(t // 3_600_000), int(t // 86_400_000)      # UTC hour and day
    span_days = (max(ms) - min(ms)) / 8.64e7 if ms else 0.0
    allst = probe_stats(recs)
    inc = [r for r in recs if r["in_class"] is True]
    inst = probe_stats(inc)
    n_out = sum(1 for r in recs if r["in_class"] is False)
    n_unk = n - len(inc) - n_out
    xs = [r["x"] for r in recs]
    mean, (lo, hi), (clo, chi) = allst["ev_per_attempt"], allst["ev_90ci"], allst["ev_90ci_hour_cluster"]
    signer, sig_src = signer_clusters(con, ids) if a.cluster == "signer" else (None, None)
    slo, shi = cluster_boot(xs, signer) if (signer and n > 1) else (float("nan"), float("nan"))
    landing, fail_mean = allst["landing_rate"], allst["failed_cost_lamports_mean"]
    landing_strict = allst["landing_rate_strict"]
    med_prem, mean_prem = allst["median_premium"], allst["mean_premium"]
    fills_closed = allst["fills"]
    rep = json.load(open(a.pnl_model))
    pol = pm.lead_policy(rep, B_SIZE)
    c_rt = a.round_trip_lamports / 1e9
    # the break-even is priced at the MEAN measured premium (a mean-premium model input), the kill / entry
    # thresholds on the premium itself stay on the median (docs/04 section 7)
    be = pm.break_even_landing(pol, fail_mean / 1e9, prem=mean_prem if mean_prem is not None else 0.011,
                               c_rt=c_rt, f_pay=1.0)
    # the stage-B bar, read on the in-class subset (its own failed cost and premium; the all-attempt ones
    # when the subset has none): max(pooled LEAD break-even, unfiltered shadow break-even at 0.25 SOL)
    cf_in = (inst["failed_cost_lamports_mean"] if inst["failed"] else (fail_mean if allst["failed"] else 730_000)) / 1e9
    pr_in = inst["mean_premium"] if inst["mean_premium"] is not None else (mean_prem if mean_prem is not None else 0.011)
    be_in = pm.break_even_landing(pol, cf_in, prem=pr_in, c_rt=c_rt, f_pay=1.0)
    be_unf = pm.break_even_at_stake(a.unfiltered_ev, a.shadow_stake, B_SIZE, cf_in, c_rt)
    bar = max(be_in, be_unf) if be_unf == be_unf else float("nan")
    b_entry = {
        "population": "in-class subset (feed_bundle.bundle_wallets >= 1)",
        "bar": bar,
        "pooled_lead_break_even": be_in,
        "pooled_lead_inputs": {"policy": f"pnl_model LEAD {B_SIZE} SOL pooled OOS, f_pay = 1",
                               "failed_cost_sol": cf_in, "mean_premium": pr_in},
        "unfiltered_shadow_break_even_at_0.25": be_unf,
        "unfiltered_shadow_inputs": {"ev_per_landed": a.unfiltered_ev, "at_stake_sol": a.shadow_stake,
                                     "failed_cost_sol": cf_in, "round_trip_sol": c_rt,
                                     "source": "--unfiltered-ev" if a.unfiltered_ev != UNFILTERED_SHADOW_EV else
                                     "default: the 14-hour Ohio shadow's +1.32% per attempt (probe-design-v2.md section 4)"},
        "in_class_attempts": inst["attempts"], "in_class_raced": inst["raced"],
        "in_class_landing_rate": inst["landing_rate"], "in_class_landing_rate_strict": inst["landing_rate_strict"],
        "late_ok": a.late_ok, "in_class_median_premium": inst["median_premium"],
        "min_in_class_attempts": kill["landing_attempts"], "max_median_premium": B_ENTRY_MAX_PREMIUM,
    }
    # (d) per-UTC-hour attempt cap (probe design v2: at most --hour-cap attempts per UTC hour)
    per_hour: dict = {}
    for r in recs:
        per_hour[r["hour"]] = per_hour.get(r["hour"], 0) + 1
    over = {f"{h * 3600}": c for h, c in sorted(per_hour.items()) if c > cap_h}
    hour_cap = {"cap": cap_h, "source": cap_src, "hours_with_attempts": len(per_hour),
                "max_per_hour": max(per_hour.values()) if per_hour else 0, "hours_over_cap": over, "clock": clock_src}
    # (d) break-even landing at the probe stake from the shadow EV per landed trade (pnl_model.break_even_at_stake)
    stake = (sum(r["spend"] for r in recs) / n / 1e9) if n else caps["max_probe_lamports"] / 1e9
    be_shadow = None
    if a.shadow_ev is not None:
        cf = fail_mean / 1e9 if allst["failed"] else 0.00073
        be_shadow = {"shadow_ev_per_landed": a.shadow_ev, "shadow_stake": a.shadow_stake, "c_fail_sol": cf,
                     "at_probe_stake": {"stake": stake, "break_even": pm.break_even_at_stake(
                         a.shadow_ev, a.shadow_stake, stake, cf, c_rt)},
                     "at_0.25": pm.break_even_at_stake(a.shadow_ev, a.shadow_stake, 0.25, cf, c_rt),
                     "note": "NaN = a landed trade loses at that stake (fixed cost per SOL), no landing rate breaks even"}
    # by sender: the buy variants handed to each sender (sends), what it accepted, what landed through it
    by_sender: dict = {}
    if has_table(con, "sends"):
        idset = set(ids)
        kind = "kind" if has_col(con, "sends", "kind") else "NULL AS kind"
        for intent_id, sender, result, k in con.execute(f"SELECT intent_id, sender, result, {kind} FROM sends"):
            if intent_id not in idset or (k is not None and k != "buy"):
                continue
            v = by_sender.setdefault(sender or "?", {"sent": 0, "accepted": 0, "send_errors": 0, "fills": 0,
                                                     "landed": 0, "landed_failed": 0})
            v["sent"] += 1
            v["accepted"] += result == "accepted"
            v["send_errors"] += result == "failed"
    for r in recs:
        if r["failed"] and r["reason"] == "dropped":
            continue                                            # nothing of ours landed
        v = by_sender.setdefault(r["sender"], {"sent": 0, "accepted": 0, "send_errors": 0, "fills": 0,
                                               "landed": 0, "landed_failed": 0})
        if r["failed"]:
            v["landed_failed"] += 1
        else:
            v["fills"] += 1
            v["landed"] += r["fill"] in LANDED
    by_variant: dict = {}
    by_class: dict = {}
    for r in recs:
        v = by_variant.setdefault(r["variant"], [0, 0])
        v[0] += 1
        v[1] += r["fill"] in LANDED
        # per leader class also the strict count: priority-ordered classes are where the slot+1 fills concentrate
        k = by_class.setdefault(r["leader_class"], [0, 0, 0, 0])
        k[0] += 1
        k[1] += r["fill"] in LANDED
        k[2] += r["cost"] if r["failed"] else 0
        k[3] += r["fill"] == "landed"
    day_net: dict = {}
    hour_net: dict = {}
    for r in recs:
        day_net[r["day"]] = day_net.get(r["day"], 0) + r["net"]
        hour_net[r["hour"]] = hour_net.get(r["hour"], 0) + r["net"]
    worst_day = min(day_net.values()) if day_net else 0
    pos = sum(v for v in hour_net.values() if v > 0)
    hour_share = max(hour_net.values()) / pos if pos > 0 else None
    # a broken transaction (compute / instruction errors) is not a lost race: no landing verdict on it
    nonrace = allst["nonrace_failures"]
    broken = nonrace >= NONRACE_MIN and nonrace > NONRACE_MAX_SHARE * n
    reasons = []
    if stage == "b" and (fills_closed >= kill["min_landed"] or n >= kill["min_attempts"]) and hi < 0:
        reasons.append(f"EV per attempt 90% upper bound {hi:+.2%} < 0 after {fills_closed} landed / {n} attempts")
    if med_prem is not None and med_prem > kill["max_median_premium"]:
        reasons.append(f"median realised premium {med_prem:+.2%} > {kill['max_median_premium']:+.0%}")
    if not broken and allst["raced"] >= kill["landing_attempts"] and landing < be:
        reasons.append(f"landing {landing:.1%} (strict slot-0 {landing_strict:.1%}) < break-even {be:.1%} over "
                       f"{allst['raced']} attempts")
    if not broken and inst["raced"] >= kill["landing_attempts"] and inst["landing_rate"] < be_in:
        reasons.append(f"in-class landing {inst['landing_rate']:.1%} (strict {inst['landing_rate_strict']:.1%}) < "
                       f"break-even {be_in:.1%} over {inst['raced']} in-class attempts")
    if worst_day <= -loss_cap:
        reasons.append(f"a day lost {-worst_day} lamports >= the daily loss cap {loss_cap}")
    a_done = n >= A_DONE_ATTEMPTS and span_days >= A_DONE_DAYS
    a_hard = span_days >= hard
    if reasons:
        verdict = "KILL: " + "; ".join(reasons)
    elif broken:
        top = ", ".join(f"{k} {v['count']}" for k, v in allst["by_fail_reason"].items() if not v["race"])
        verdict = (f"CHECK: {nonrace} of {n} attempts ({nonrace / n:.0%}) failed on transaction errors, not lost "
                   f"races ({top}): fix the transaction (tx.cu_limit, docs/08 section 9) before reading landing; "
                   "no landing kill and no stage-B entry until then")
    elif stage == "a" and (a_hard or (a_done and inst["raced"] >= kill["landing_attempts"])):
        short = inst["raced"] < kill["landing_attempts"]       # only possible at the hard stop
        why = []
        if short:
            why.append(f"in-class subset {inst['raced']} < {kill['landing_attempts']} attempts")
        if bar != bar:
            why.append("the unfiltered shadow EV loses at 0.25 SOL (no landing rate breaks even)")
        elif inst["landing_rate"] <= bar:
            why.append(f"in-class landing {inst['landing_rate']:.1%} (strict {inst['landing_rate_strict']:.1%}) <= "
                       f"the stage-B bar {bar:.1%}")
        if inst["median_premium"] is None or inst["median_premium"] > B_ENTRY_MAX_PREMIUM:
            why.append(f"in-class median premium {fmt_pct(inst['median_premium'], True)} above +2% or unmeasured")
        # too little data is not a negative result: a short subset that fails nothing else (or holds no attempt
        # that raced, so its landing and premium are unmeasured) is not a "stop"
        if not why:
            verdict = "PROBE-A COMPLETE: ENTER PROBE-B"
        elif short and bar == bar and (len(why) == 1 or inst["raced"] == 0):
            verdict = (f"INCONCLUSIVE: in-class subset {inst['raced']} < {kill['landing_attempts']} attempts at the "
                       f"{hard:g}-day hard stop ("
                       + ("every other stage-B condition is met" if len(why) == 1 else "no in-class attempt raced")
                       + "): too little data, not a negative result; do not enter probe-B and do not stop the "
                       "project: escalate to the owner (extend stage A, docs/research/probe-design-v2.md section 2)")
        else:
            verdict = "PROBE-A COMPLETE: DO NOT ENTER PROBE-B (" + "; ".join(why) + "): stop"
        if a_hard and not a_done:
            verdict += f" [{hard:g}-day hard stop at {n} attempts]"
    elif stage == "a" and a_done:
        verdict = (f"CONTINUE probe-A ({A_DONE_ATTEMPTS} attempts and {A_DONE_DAYS:g} days reached, but the in-class "
                   f"subset has {inst['raced']} of the {kill['landing_attempts']} attempts the stage-B read needs: "
                   f"continue to the {hard:g}-day hard stop)")
    elif stage == "b" and fills_closed >= B_DONE_LANDED:
        verdict = "PROBE-B COMPLETE: read the live gate (docs/06 section 9.5)"
    else:
        verdict = f"CONTINUE probe-{stage.upper()}"
    cap_pct = f"+{LANDED_MAX_PREMIUM_BPS / 100:g}%"
    landed_rule = (f"fill at the create slot + 0..{a.late_ok} at a measured premium <= {cap_pct} (strict: slot 0 only; "
                   f"--late-ok {a.late_ok})" if a.late_ok else
                   f"creation-slot fill at a measured premium <= {cap_pct} (--late-ok 0: strict, slot 0 only)")
    out = {
        "stage": stage, "caps": caps, "kill": kill,
        "completion": {"attempts": A_DONE_ATTEMPTS, "days": A_DONE_DAYS, "hard_stop_days": hard,
                       "in_class_attempts": kill["landing_attempts"]} if stage == "a" else {"landed": B_DONE_LANDED},
        "attempts": n, "landed_closed": allst["landed"], "landed_strict_closed": allst["landed_strict"],
        "landed_late_closed": allst["landed_late"], "landed_late_by_offset": allst["landed_late_by_offset"],
        "fills_closed": fills_closed,
        "failed_attempts": allst["failed"], "open_positions_left_out": open_left,
        "landed_rule": landed_rule, "late_ok": a.late_ok,
        "late_fills": allst["late_fills"], "slot0_fills_over_premium": allst["slot0_fills_over_premium"],
        "late_fills_over_premium": allst["late_fills_over_premium"],
        "fills_premium_unmeasured": allst["fills_premium_unmeasured"],
        "flagged_late_within_window": allst["flagged_late_within_window"],
        "landing_rate": landing, "landing_rate_strict": landing_strict, "raced_attempts": allst["raced"],
        "failed_cost_lamports_mean": fail_mean,
        "failures": {"race": allst["race_failures"], "not_race": nonrace, "by_reason": allst["by_fail_reason"],
                     "transaction_broken": broken},
        "median_premium": med_prem, "mean_premium": mean_prem, "premiums_measured": allst["premiums_measured"],
        "break_even_landing": be, "break_even_basis": f"pnl_model LEAD {B_SIZE} SOL pooled OOS, f_pay = 1",
        "ev_per_attempt": mean, "ev_90ci": [lo, hi], "ev_90ci_hour_cluster": [clo, chi],
        "ev_90ci_signer_cluster": [slo, shi] if signer else None, "signer_cluster_source": sig_src,
        "in_class": dict(inst, without_feed_bundle_row=n_unk, out_of_class=n_out,
                         source="feed_bundle.bundle_wallets >= 1" if fb else "no feed_bundle table (schema < v5)"),
        "b_entry": b_entry,
        "days_covered": span_days, "clock": clock_src, "clock_warning": clock_warn,
        "hour_cap": hour_cap, "break_even_from_shadow": be_shadow, "worst_day_net_lamports": worst_day, "daily_loss_cap_lamports": loss_cap,
        "max_hour_share_of_positive_pnl": hour_share,
        "by_variant": {str(k): {"attempts": v[0], "landed": v[1]} for k, v in by_variant.items()},
        "by_leader_class": ({str(k): {"attempts": v[0], "landed": v[1], "landed_strict": v[3],
                                      "landing_rate": v[1] / v[0] if v[0] else 0.0, "failed_cost_lamports": v[2]}
                             for k, v in by_class.items()}
                            if lc != "NULL" else "not recorded (state.db before schema v5: no leader_class column)"),
        "by_sender": dict(sorted(by_sender.items())),
        "verdict": verdict,
    }
    if a.json:
        print(json.dumps(out, indent=1, default=float))
        return 0
    lf = allst["late_fills"]
    win = f"slot+1..+{a.late_ok}"
    print(f"stage               probe-{stage.upper()}  caps {caps}")
    print(f"attempts            {n} over {span_days:.2f} days (landed {allst['landed']}, other fills "
          f"{fills_closed - allst['landed']}, failed {allst['failed']}, open left out {open_left})")
    late_ok_part = ""
    if a.late_ok:
        lbo = allst["landed_late_by_offset"]
        late_ok_part = (f"; landed late ({win}) {allst['landed_late']}"
                        + (" (" + ", ".join(f"{k} {v}" for k, v in lbo.items()) + ")" if lbo else "")
                        + (f", burn flag set on {allst['flagged_late_within_window']} landed fill(s)"
                           if allst["flagged_late_within_window"] else ""))
    print(f"landed rule         {out['landed_rule']}{late_ok_part}; late fills {lf['total']}"
          + (" (" + ", ".join(f"{k} {v}" for k, v in lf.items() if k != "total") + ")" if lf["total"] else "")
          + f", slot-0 above +2% {allst['slot0_fills_over_premium']}"
          + (f", {win} above +2% {allst['late_fills_over_premium']}" if a.late_ok else "")
          + f", premium unmeasured {allst['fills_premium_unmeasured']}: none of them count as landed")
    print(f"landing rate        {landing:.1%} of {allst['raced']} attempts that raced (strict, slot 0 only: "
          f"{landing_strict:.1%})  break-even {be:.1%} ({out['break_even_basis']})")
    fr = allst["by_fail_reason"]
    print("failures            lost races: " + (", ".join(f"{k} {v['count']}" for k, v in fr.items() if v["race"]) or "none")
          + "; NOT races (left out of the landing rate): "
          + (", ".join(f"{k} {v['count']}" for k, v in fr.items() if not v["race"]) or "none"))
    if broken:
        print(f"!!! WARNING         {nonrace} of {n} attempts failed on transaction errors: fix the transaction first")
    print(f"failed cost         {fail_mean:.0f} lamports per failed attempt")
    print(f"median premium      {fmt_pct(med_prem, True)}  (mean {fmt_pct(mean_prem, True)}, {allst['premiums_measured']} "
          "measured; break-even priced at the mean)")
    if clock_warn:
        print(f"!!! WARNING         {clock_warn}")
    print(f"hour cap            max {hour_cap['max_per_hour']} attempts in a UTC hour (cap {cap_h}, {cap_src}); "
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
    ci_in = inst["ev_90ci_hour_cluster"]
    print(f"in-class subset     {inst['attempts']} attempts ({out['in_class']['source']}; out of class {n_out}, "
          f"no feed_bundle row {n_unk}): landed {inst['landed']}, landing {inst['landing_rate']:.1%} of "
          f"{inst['raced']} (strict {inst['landing_rate_strict']:.1%}), late fills {inst['late_fills']['total']}, "
          f"median premium "
          f"{fmt_pct(inst['median_premium'], True)}, EV per attempt {fmt_pct(inst['ev_per_attempt'], True)} "
          f"hour-cluster [{fmt_pct(ci_in[0], True)}, {fmt_pct(ci_in[1], True)}]")
    print(f"stage-B bar         {fmt_pct(bar)} = max(pooled LEAD break-even {be_in:.1%} [failed cost {cf_in:.5f} SOL, "
          f"mean premium {pr_in:+.2%}], unfiltered shadow break-even at 0.25 SOL {fmt_pct(be_unf)} [shadow EV "
          f"{a.unfiltered_ev:+.2%} at {a.shadow_stake:g} SOL, failed cost {cf_in:.5f} SOL]); read on the in-class "
          f"subset, needs >= {kill['landing_attempts']} in-class attempts and a median premium <= +2%")
    print(f"worst day net       {worst_day:+d} lamports (daily loss cap {loss_cap})")
    if hour_share is not None:
        print(f"largest hour        {hour_share:.0%} of the positive P&L")
    for k, v in out["by_variant"].items():
        print(f"  variant {k:3s} attempts {v['attempts']}, landed {v['landed']}")
    if isinstance(out["by_leader_class"], dict):
        for k, v in sorted(out["by_leader_class"].items()):
            print(f"  leader {k:10s} attempts {v['attempts']}, landed {v['landed']} ({v['landing_rate']:.1%}"
                  + (f"; strict {v['landed_strict']}" if a.late_ok else "")
                  + f"), failed cost {v['failed_cost_lamports']} lamports")
    for k, v in out["by_sender"].items():
        print(f"  sender {k:12s} sent {v['sent']}, accepted {v['accepted']}, send errors {v['send_errors']}, "
              f"landed {v['landed']} (fills {v['fills']}), landed and failed {v['landed_failed']}")
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
    ap.add_argument("--hour-cap", type=int,
                    help="--probe: flag UTC hours with more than N attempts (default: the --config's "
                         "risk.max_buys_per_hour, which the risk gate enforces; without it the design's 4)")
    ap.add_argument("--hard-stop-days", type=float, default=A_HARD_STOP_DAYS,
                    help="--probe: stage A's hard stop in days (default 10; the owner's longer stage under readiness "
                         "audit M4 option (i), docs/research/probe-design-v2.md section 2); at least 7")
    ap.add_argument("--shadow-ev", type=float,
                    help="--probe: the shadow run's EV per landed trade (fraction, e.g. 0.042) for the break-even "
                         "landing at the probe stake (pnl_model.break_even_at_stake)")
    ap.add_argument("--shadow-stake", type=float, default=0.5,
                    help="--probe: stake (SOL) the --shadow-ev and --unfiltered-ev were measured at")
    ap.add_argument("--unfiltered-ev", type=float, default=UNFILTERED_SHADOW_EV,
                    help="--probe: the UNFILTERED shadow EV per landed trade (fraction) that sets the stage-B bar's "
                         "second input, its break-even at 0.25 SOL (default 0.0132: the 14-hour Ohio shadow's +1.32%% "
                         "per attempt, 36%% at 0.00073 SOL; probe-design-v2.md section 4)")
    ap.add_argument("--late-ok", type=int, default=LATE_OK_DEFAULT,
                    help="--probe: a fill at create_slot + k with 1 <= k <= N counts as landed (bucket landed_late); "
                         "later fills stay late; 0 = the strict creation-slot rule (default 1: at 200 ms slots a "
                         "slot+1 fill did no worse than a same-slot one in stage A)")
    a = ap.parse_args()
    if a.hard_stop_days < A_DONE_DAYS:
        ap.error(f"--hard-stop-days must be at least {A_DONE_DAYS:g} (stage A completes at >= {A_DONE_DAYS:g} days)")
    if a.late_ok < 0:
        ap.error("--late-ok must be >= 0")
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
