"""S3: operator-conditioned arming of the S1 policies, walk-forward over the 16-day window.

  .venv/bin/python -I analysis/scripts/strategy/operator_walkforward.py            # full run (~5-10 min)
  .venv/bin/python -I analysis/scripts/strategy/operator_walkforward.py --quick    # B=400, no control

Design (fixed before any arming result was computed):
  * Launches: every bundled / dev>=1 SOL launch with a tape on disk (arming.load_launches): gap-3,
    gap-3 operator pull, deep, fresh, the gap-1 s6 expanded wallets (cached first 2 s), and the S3
    pulls (operator_pull.py: A = low-ATH coins of bundling expanded wallets, B = new heavy bundlers).
  * Policies: fixed, from S1 (frozen #1-#3 and #3 at 0.25 SOL) plus three references with long
    horizons (the guru's TP +100 %, his bot's protective version, gap-3's TP +40 % recommendation).
  * Arming: keys {creator, bundle-wallet component, bundle-shape} x rules {R0, EB, GTP, G100} x
    skip13 {off, on} x cap45 {off, on} x timing {2d, cont}; plus the unarmed class baseline per policy
    x skip13 x cap45. Re-arming times t = day 2, 4, ..., 14; each launch is scored once, in the window
    that starts at the last t before it. Days are counted from 2026-09-28 08:58:50 UTC.
  * Headline set: "policy-OOS" = scored launches that were NOT in the gap-3 train split (the split
    that selected the S1 policies). p-values: normal approximation with the operator x day cluster
    bootstrap SE; Benjamini-Hochberg and Bonferroni over every configuration run.
"""
from __future__ import annotations

import json
import sys
import time
from collections import Counter
from dataclasses import replace
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
from rugsnipe_analysis.strategy import arming as A  # noqa: E402
from rugsnipe_analysis.strategy import sim  # noqa: E402
from rugsnipe_analysis.strategy.stats import bh_qvalues  # noqa: E402

OUT = REPO / "data" / "reports" / "strategy"
QUICK = "--quick" in sys.argv
BOOT = 400 if QUICK else 2000
EXP_POOL_FACTOR = 920 / 200   # gap-1: the 200 expanded wallets are a random 22 % of the 920 mid-size wallets

LEAD = [("tp", 0.15), ("ins", "bundle"), ("idle", 1), ("psl", 1, 0.15), ("slotexit", 5)]
POLICIES = [
    A.Policy("LEAD_0.5", LEAD, 0.5, "multi", 2),
    A.Policy("LEAD_0.25", LEAD, 0.25, "multi", 2),
    A.Policy("F1_0.5", [("tp", 0.20), ("idle", 3), ("bigbuy", 3.0), ("psl", 1, 0.10), ("hold", 20)], 0.5, "me<=45", 25),
    A.Policy("F2_0.5", [("tp", 0.20), ("idle", 2), ("bigbuy", 3.0), ("hold", 10)], 0.5, "me<=50", 12),
    A.Policy("GURU_TP100", [("tp", 1.0), ("hold", 900)], 0.25, "all", 900),
    A.Policy("GURU_BOT", [("tp", 1.0), ("psl", 10, 0.25), ("bigbuy", 2.0), ("idle", 10), ("hold", 900)], 0.25, "all", 900),
    A.Policy("G3_TP40", [("tp", 0.40), ("ins", "any"), ("idle", 3), ("hold", 900)], 0.25, "all", 900),
]
KEYS = ("creator", "bcomp", "shape")
TIMINGS = ("2d", "cont")


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def truncate(t, hor):
    hend = int(np.searchsorted(t.ts, t.bt + hor, side="right"))
    sl = slice(0, hend)
    return replace(t, hor=hor, hend=hend, complete=True, slot=t.slot[sl], ts=t.ts[sl], buy=t.buy[sl], sol=t.sol[sl],
                   tok=t.tok[sl], mc=t.mc[sl], ins_b=t.ins_b[sl], ins_c=t.ins_c[sl], s0tp=t.s0tp[sl], uid=t.uid[sl])


def truncation_check(L):
    """The expanded-wallet tapes stop at create + 2 s. Re-score the full gap-3 tapes cut to 2 s / 4 s."""
    g = [x for x in L if x.src == "gap3" and x.tape.nB >= 1]
    w = np.array([x.w for x in g])
    res = {}
    full = None
    for hor in (900, 25, 4, 2):
        tapes = [x.tape if hor == 900 else truncate(x.tape, hor) for x in g]
        F, R, trig, _ = sim.simulate_entry(tapes, "E1", sim.Scenario(0.5, 0.011, 0.004, 1), LEAD)
        r = R[np.arange(len(g)), np.argmin(F, axis=1)]
        r = np.where(trig, r, np.nan)
        m = np.isfinite(r)
        if full is None:
            full = r
        d = r - full
        res[hor] = dict(ev=float(np.sum(w[m] * r[m]) / w[m].sum()), n=int(m.sum()),
                        mean_abs_diff=float(np.nanmean(np.abs(d))), share_diff=float(np.nanmean(np.abs(d) > 1e-6)))
        log(f"truncation {hor:>3}s: EV {res[hor]['ev']:+.4f} n={res[hor]['n']} mean|diff| {res[hor]['mean_abs_diff']:.4f} share changed {res[hor]['share_diff']:.3f}")
    return res


def summarize(L, r, tp, armed, op, win, sets, boot_seed=11, control=True):
    """Stats of armed trades on each scoring set."""
    w = np.array([x.w for x in L])
    day = np.array([int(x.day) for x in L])
    pw = np.array([x.pw for x in L])
    src = np.array([x.src for x in L])
    out = {}
    for sname, smask in sets.items():
        m = armed & smask
        n = int(m.sum())
        if n < 2:
            out[sname] = dict(n=n)
            continue
        cl = np.array([f"{op[i]}|{day[i]}" for i in np.nonzero(m)[0]])
        b = A.cluster_boot(r[m], w[m], cl, B=BOOT, seed=boot_seed)
        hit = float(np.sum(w[m] * tp[m]) / np.sum(w[m]))
        bo = A.cluster_boot(r[m], w[m], np.array([str(op[i]) for i in np.nonzero(m)[0]]), B=BOOT, seed=boot_seed + 1)
        by_src = {}
        for s_ in sorted(set(src[m])):
            mm = m & (src == s_)
            by_src[s_] = dict(n=int(mm.sum()), ev=float(np.sum(w[mm] * r[mm]) / np.sum(w[mm])))
        rec = dict(n=n, ev=b["mean"], lo=b["lo"], hi=b["hi"], se=b["se"], p=b["p_norm"], p_boot=b["p_boot"],
                   clusters=b["k"], lo_op=bo["lo"], hi_op=bo["hi"], p_op=bo["p_norm"], hit=hit,
                   ops=int(len(set(op[m]))), by_src=by_src)
        out[sname] = rec
    # volume over all scored rows (days 2..16 = 14 days), independent of the performance set
    m = armed
    D = 14.0
    vol = {s: float(pw[m & (src == s)].sum() / D) for s in ("gap3", "exp", "opA", "opB", "ops")}
    out["per_day"] = dict(studied=float(vol["gap3"] + vol["exp"] + vol["opA"] + vol["opB"] + vol["ops"]),
                          pool_extrapolated=float(vol["gap3"] + EXP_POOL_FACTOR * (vol["exp"] + vol["opA"]) + vol["opB"] + vol["ops"]),
                          parts=vol)
    out["armed_ops_total"] = int(len(set(op[armed])))
    return out


def control_stats(L, r, armed, win, scorable, mask, seed=5):
    m = armed & mask
    if m.sum() < 5:
        return None
    ctrl, nm = A.matched_control(L, np.nan_to_num(r), m, win, scorable & mask, R=200, seed=seed)
    if not len(ctrl):
        return None
    w = np.array([x.w for x in L])
    ev = float(np.sum(w[m] * r[m]) / np.sum(w[m]))
    return dict(control_ev=float(ctrl.mean()), control_lo=float(np.quantile(ctrl, 0.05)),
                control_hi=float(np.quantile(ctrl, 0.95)), matched=int(nm),
                p_armed_le_control=float((np.sum(ctrl >= ev) + 1) / (len(ctrl) + 1)), armed_minus_control=ev - float(ctrl.mean()))


PRIMARY = ("LEAD_0.5", "creator/EB/skip13/cap45/2d")   # pre-declared (task spec: EB rule, skip13, cap45)


def stress(L, P, armed, mask):
    """Re-price the armed trades (arming decisions fixed) under worse fills, costs, latency and fewer
    snipers. Returns a dict of EV per scenario on armed & mask."""
    from rugsnipe_analysis.strategy.tapes import drop_wallets
    idx = np.nonzero(armed & mask)[0]
    w = np.array([L[i].w for i in idx])
    tapes = [L[i].tape for i in idx]
    out = {}

    def ev(tp_, prem=P.prem, cost=P.cost, lat=P.lat):
        F, R, trig, _ = sim.simulate_entry(tp_, "E1", sim.Scenario(P.size, prem, cost, lat), P.specs)
        r = R[np.arange(len(tp_)), np.argmin(F, axis=1)]
        r = np.where(trig, r, np.nan)
        m = np.isfinite(r)
        return float(np.sum(w[m] * r[m]) / w[m].sum()), int(m.sum())
    for prem in (0.0, 0.011, 0.02, 0.03, 0.036, 0.05, 0.08, 0.108):
        for cost in (0.004, 0.01):
            out[f"prem{prem}_cost{cost}"] = ev(tapes, prem, cost)
    for lat in (0, 2, 4):
        out[f"lat{lat}"] = ev(tapes, lat=lat)
    rng = np.random.default_rng(3)
    for frac in (0.25, 0.5, 1.0):
        vals = []
        for _ in range(5 if frac < 1 else 1):
            tt = []
            for t in tapes:
                u = sorted(set(t.uid[t.s0tp].tolist()))
                k = int(round(frac * len(u)))
                drop = set(rng.choice(u, size=k, replace=False).tolist()) if (u and k) else set()
                tt.append(drop_wallets(t, drop))
            vals.append(ev(tt)[0])
        out[f"drop{frac}"] = (float(np.mean(vals)), len(tapes))
    return out


def mechanism(L, P, r, tp, reason, armed, ctrl_mask):
    """Descriptive: what differs between armed launches and the rest (same scoring set)."""
    rows = {}
    for nm, m in (("armed", armed), ("not_armed", ctrl_mask & ~armed)):
        idx = np.nonzero(m)[0]
        if not len(idx):
            continue
        w = np.array([L[i].w for i in idx])
        f = lambda v: float(np.sum(w * v) / w.sum())  # noqa: E731
        rows[nm] = dict(n=int(len(idx)), ev=f(r[idx]), tp_rate=f(tp[idx].astype(float)),
                        s0_third=f(np.array([L[i].tape.s0_third for i in idx], float)),
                        me=f(np.array([L[i].me for i in idx])), nB=f(np.array([L[i].tape.nB for i in idx], float)),
                        dev_sol=f(np.array([L[i].tape.dev_sol for i in idx])),
                        bundle_sell_le3=f(np.array([(L[i].fbs is not None and L[i].fbs <= 3) for i in idx], float)),
                        reasons={k: f(np.array([reason[i] == k for i in idx], float)) for k in sorted(set(reason[idx]))})
    return rows


def persistence(L, r, key, min_n=10, perms=2000, seed=9):
    """Split-half persistence of operator EV: for operators with >= min_n scored launches, compare the
    weighted mean of their first half (by time) with their second half. Spearman rho, Pearson r, and a
    permutation p (second-half means shuffled across operators)."""
    op = A.op_ids(L, key, np.ones(len(L), bool))
    w = np.array([x.w for x in L])
    bt = np.array([x.bt for x in L])
    groups = {}
    for i in np.nonzero(np.isfinite(r))[0]:
        groups.setdefault(op[i], []).append(i)
    a, b, ns = [], [], []
    for o, idx in groups.items():
        if len(idx) < min_n or o.startswith("b1:") or o == "s:none":
            continue
        idx = sorted(idx, key=lambda i: bt[i])
        h = len(idx) // 2
        i1, i2 = np.array(idx[:h]), np.array(idx[h:])
        a.append(np.sum(w[i1] * r[i1]) / w[i1].sum())
        b.append(np.sum(w[i2] * r[i2]) / w[i2].sum())
        ns.append(len(idx))
    a, b = np.array(a), np.array(b)
    if len(a) < 5:
        return dict(k=int(len(a)))
    rank = lambda v: np.argsort(np.argsort(v))  # noqa: E731
    rho = float(np.corrcoef(rank(a), rank(b))[0, 1])
    pr = float(np.corrcoef(a, b)[0, 1])
    rng = np.random.default_rng(seed)
    null = np.array([np.corrcoef(rank(a), rank(rng.permutation(b)))[0, 1] for _ in range(perms)])
    # top-tercile on the first half -> second half
    top = a >= np.quantile(a, 2 / 3)
    bot = a <= np.quantile(a, 1 / 3)
    return dict(k=int(len(a)), n_launches=int(np.sum(ns)), spearman=rho, pearson=pr,
                p_perm=float((np.sum(null >= rho) + 1) / (perms + 1)),
                first_top=float(a[top].mean()), second_top=float(b[top].mean()),
                first_bottom=float(a[bot].mean()), second_bottom=float(b[bot].mean()))


def lift_by_source(L, r, tp, armed, op, scored):
    """Armed vs not-armed EV inside each source group (same scoring set), operator-clustered CIs."""
    src = np.array([x.src for x in L])
    w = np.array([x.w for x in L])
    groups = {"random sample (gap-3 test, deep, fresh)": ["gap3", "deep", "fresh"],
              "expanded bundlers (exp + opA, complete)": ["exp", "opA"],
              "new heavy bundlers (opB)": ["opB"], "gap-3 operator pull (ops)": ["ops"]}
    out = {}
    for g, ss in groups.items():
        rec = {}
        for nm, m in (("armed", armed), ("not_armed", ~armed)):
            mm = m & scored & np.isin(src, ss)
            if mm.sum() < 2:
                rec[nm] = dict(n=int(mm.sum()))
                continue
            b = A.cluster_boot(r[mm], w[mm], np.array([str(op[i]) for i in np.nonzero(mm)[0]]), B=BOOT, seed=21)
            rec[nm] = dict(n=int(mm.sum()), ev=b["mean"], lo=b["lo"], hi=b["hi"], ops=int(len(set(op[mm]))),
                           hit=float(np.sum(w[mm] * tp[mm]) / w[mm].sum()))
        out[g] = rec
    return out


def main():
    T = time.time()
    L = A.load_launches()
    n = len(L)
    src = np.array([x.src for x in L])
    g3tr = np.array([x.g3_train for x in L])
    day = np.array([x.day for x in L])
    res = dict(n_launches=n, by_src=dict(Counter(src)), multi_by_src=dict(Counter(src[[x.tape.nB >= 1 for x in L]])))
    res["truncation"] = truncation_check(L)

    sets = {
        "policy_oos": (~g3tr) & (day >= 2),
        "all": day >= 2,
        "never_selected": np.isin(src, ["deep", "fresh", "exp", "opA", "opB", "ops"]) & (day >= 2),
        "no_expanded": (~g3tr) & (day >= 2) & ~np.isin(src, ["exp", "opA"]),
    }
    # ------------------------------------------------------------------ policies, unconditional
    PR = {}
    res["unconditional"] = {}
    for P in POLICIES:
        r, tp, reason = A.policy_returns(L, P)
        PR[P.name] = (r, tp, reason)
        w = np.array([x.w for x in L])
        u = {}
        for s in ("gap3", "ops", "deep", "fresh", "exp", "opA", "opB"):
            m = np.isfinite(r) & (src == s) & ~g3tr
            if m.sum():
                u[s] = dict(n=int(m.sum()), ev=float(np.sum(w[m] * r[m]) / w[m].sum()),
                            hit=float(np.sum(w[m] * tp[m]) / w[m].sum()))
        res["unconditional"][P.name] = u
        log(P.name, " ".join(f"{s}:{v['ev']:+.3f}(n={v['n']})" for s, v in u.items()))

    # ------------------------------------------------------------------ the grid
    configs = []
    for P in POLICIES:
        for skip in (False, True):
            for cap in (False, True):
                configs.append((P, A.WFConfig("none", "none", skip, cap, "2d")))
                for key in KEYS:
                    for rule in A.RULES:
                        for timing in TIMINGS:
                            configs.append((P, A.WFConfig(key, rule, skip, cap, timing)))
    log(f"{len(configs)} configurations")
    rows = []
    for ci, (P, cfg) in enumerate(configs):
        r, tp, _ = PR[P.name]
        wins = A.walk_forward(L, r, tp, cfg)
        armed, op, win = A.flatten(L, wins)
        st = summarize(L, r, tp, armed, op, win, sets)
        row = dict(policy=P.name, cfg=cfg.name, key=cfg.key, rule=cfg.rule, skip13=cfg.skip13, cap45=cfg.cap45,
                   timing=cfg.timing, **{k: v for k, v in st.items()})
        row["_armed"] = armed
        row["_op"] = op
        row["_win"] = win
        rows.append(row)
        if ci % 50 == 0:
            log(ci, P.name, cfg.name, {k: (round(v.get("ev", float("nan")), 4), v.get("n")) for k, v in st.items() if k in sets})
    # multiple testing over every configuration (policy-OOS set, H1: EV > 0)
    p = np.array([row["policy_oos"].get("p", 1.0) if row["policy_oos"].get("n", 0) >= 10 else 1.0 for row in rows])
    p = np.nan_to_num(p, nan=1.0)
    q = bh_qvalues(p)
    for row, pi, qi in zip(rows, p, q):
        row["p_oos"] = float(pi)
        row["q_bh"] = float(qi)
        row["bonf"] = float(min(1.0, pi * len(rows)))
    res["n_configs"] = len(rows)
    # matched random control for every configuration with >= 20 policy-OOS armed trades
    log("matched controls")
    for row in rows:
        P = next(P for P in POLICIES if P.name == row["policy"])
        r = PR[P.name][0]
        scor = np.isfinite(r) & ((np.array([x.me for x in L]) <= 45) if row["cap45"] else True)
        if row["policy_oos"].get("n", 0) >= 20 and row["key"] != "none":
            row["control"] = control_stats(L, r, row["_armed"], row["_win"], scor, sets["policy_oos"])
    # stale 7-day arming for the main rule on the lead
    log("stale 7-day windows")
    stale = []
    for key in KEYS:
        for rule in A.RULES:
            P = POLICIES[0]
            r, tp, _ = PR[P.name]
            cfg = A.WFConfig(key, rule, True, True, "2d", horizon=7.0)
            for W in A.walk_forward(L, r, tp, cfg):
                idx = W["idx"][W["armed"]]
                idx = idx[sets["policy_oos"][idx]]
                w = np.array([L[i].w for i in idx])
                ev = float(np.sum(w * r[idx]) / w.sum()) if len(idx) else float("nan")
                stale.append(dict(key=key, rule=rule, t=W["t"], n=int(len(idx)), ev=ev, ops=int(len(set(W["op"][W["armed"]])))))
    res["stale7"] = stale
    # per-window detail for the best configurations
    clean = [{k: v for k, v in row.items() if not k.startswith("_")} for row in rows]
    res["rows"] = clean
    order = sorted(range(len(rows)), key=lambda i: rows[i]["p_oos"])
    res["top"] = [clean[i] for i in order[:25]]
    detail = {}
    for i in order[:6] + [i for i, row in enumerate(rows) if row["policy"] == "LEAD_0.5" and row["key"] == "none"]:
        row = rows[i]
        r = PR[row["policy"]][0]
        win = row["_win"]
        per = []
        for k in sorted(set(win[win >= 0])):
            m = row["_armed"] & (win == k) & sets["policy_oos"]
            w = np.array([x.w for x in L])
            per.append(dict(win=int(k), n=int(m.sum()), ev=float(np.sum(w[m] * r[m]) / w[m].sum()) if m.sum() else None))
        detail[f"{row['policy']}|{row['cfg']}"] = per
    res["per_window"] = detail
    # operator persistence of the lead's EV (split-half, all scorable launches)
    res["persistence"] = {}
    for pol in ("LEAD_0.5", "F2_0.5", "GURU_TP100"):
        r = PR[pol][0]
        for key in KEYS:
            res["persistence"][f"{pol}|{key}"] = persistence(L, r, key)
            log("persistence", pol, key, res["persistence"][f"{pol}|{key}"])
    # EB family parameters per re-arming time for the primary
    P0 = POLICIES[0]
    res["eb_params"] = {}
    for key in KEYS:
        wins = A.walk_forward(L, PR[P0.name][0], PR[P0.name][1], A.WFConfig(key, "EB", True, True, "2d"))
        res["eb_params"][key] = [dict(t=W["t"], **(W["par"] or {}), armed=int(W["armed"].sum()), test=int(len(W["idx"]))) for W in wins]
    # primary configuration and the best rows: stress, mechanism, operator list
    res["primary"] = {}
    focus = [PRIMARY, ("LEAD_0.5", "creator/EB/skip13/-/2d"), ("LEAD_0.5", "bcomp/EB/skip13/-/2d"),
             ("LEAD_0.5", "creator/G100/skip13/-/2d"), ("LEAD_0.5", "creator/GTP/skip13/-/2d")] + \
        [(rows[i]["policy"], rows[i]["cfg"]) for i in order[:3]]
    for pol, cfgname in dict.fromkeys(focus):
        row = next(rw for rw in rows if rw["policy"] == pol and rw["cfg"] == cfgname)
        P = next(P for P in POLICIES if P.name == pol)
        r, tp, reason = PR[pol]
        me = np.array([x.me for x in L])
        scor = np.isfinite(r) & ((me <= 45) if row["cap45"] else True)
        log("stress", pol, cfgname)
        ops = Counter(str(o) for o in row["_op"][row["_armed"] & sets["policy_oos"]])
        res["primary"][f"{pol}|{cfgname}"] = dict(
            row={k: v for k, v in row.items() if not k.startswith("_")},
            stress=stress(L, P, row["_armed"], sets["policy_oos"]),
            mechanism=mechanism(L, P, r, tp, reason, row["_armed"] & sets["policy_oos"], scor & sets["policy_oos"] & (row["_win"] >= 0)),
            lift=lift_by_source(L, r, tp, row["_armed"], row["_op"], scor & sets["policy_oos"] & (row["_win"] >= 0)),
            operators=ops.most_common(40))
    res["runtime_s"] = time.time() - T
    json.dump(res, open(OUT / "operator_walkforward.json", "w"), indent=1, default=float)
    log("done", round(time.time() - T), "s")


if __name__ == "__main__":
    main()
