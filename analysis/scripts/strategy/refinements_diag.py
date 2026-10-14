"""Task G2 diagnostics (run AFTER the freeze; nothing here selects anything).

  1. follow-through k (third-party buys behind us by the end of slot se / se+1) as a busy-launch detector
     on the window (AUC vs the API-mode label) and LEAD EV by k on every OOS set
  2. ceilings: perfect exit within slot+5/+20 (oracle), perfect bail on quiet launches (oracle on the label),
     perfect skip of quiet launches (pre-entry oracle)
  3. price drift after the LEAD's TP fill (why runners lose)
  4. the re-entry leg on its own
  5. size vs sniper drop: how much extra sniper attrition erases the 1.0 SOL fixed-cost gain
Run: .venv/bin/python -I analysis/scripts/strategy/refinements_diag.py
"""
from __future__ import annotations

import json
import math
import sys
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
sys.path.insert(0, str(REPO / "analysis/scripts/strategy"))
from rugsnipe_analysis.strategy import postentry as pe, sim  # noqa: E402
from refinements import scen_sets, BASE  # noqa: E402
from refinements_data import load  # noqa: E402

OOS = ("test", "deep", "fresh", "window")
out = {}


def wm(x, w, m=None):
    m = np.ones(len(x), bool) if m is None else m
    return float(np.sum(x[m] * w[m]) / w[m].sum()) if m.any() else float("nan")


def auc(score, label):
    pos, neg = score[label], score[~label]
    if not len(pos) or not len(neg):
        return float("nan")
    gt = (pos[:, None] > neg[None, :]).sum() + 0.5 * (pos[:, None] == neg[None, :]).sum()
    return float(gt / (len(pos) * len(neg)))


sets = load()
# ---------------------------------------------------------------- 1. follow-through
out["followthrough"] = {}
for nm in OOS + ("train",):
    T = sets[nm]
    V = [pe.TV(t) for t in T]
    w = np.array([t.w for t in T])
    r = np.array([pe.run_lead(v).pnl / 0.5 for v in V])
    rec = {}
    for g in (0, 1):
        k = np.array([v.third_buys_by(v.se + g) for v in V])
        pr = np.array([v.mc[v.eos_idx(v.se + g)] / v.base - 1 for v in V])
        b = {}
        for lo, hi in ((0, 0), (1, 1), (2, 2), (3, 4), (5, 999)):
            m = (k >= lo) & (k <= hi)
            b[f"{lo}-{hi}"] = dict(n=int(m.sum()), share=float(w[m].sum() / w.sum()), ev=wm(r, w, m), hit=wm((r > 0).astype(float), w, m))
        rec[f"slot+{g}"] = b
        if nm == "window":
            busy = np.array([t.mode != "full_1page" for t in T])
            rec[f"auc_k_slot+{g}"] = auc(k.astype(float), busy)
            rec[f"auc_price_slot+{g}"] = auc(pr, busy)
            rec[f"busy_share_by_k_slot+{g}"] = {kk: float(busy[(k >= lo) & (k <= hi)].mean()) if ((k >= lo) & (k <= hi)).any() else None
                                               for kk, (lo, hi) in {"0": (0, 0), "1": (1, 1), "2": (2, 2), "3-4": (3, 4), "5+": (5, 999)}.items()}
    out["followthrough"][nm] = rec
    print(nm, {g: {kk: f"{v['ev']*100:+.1f}%/{v['share']:.2f}" for kk, v in rec[g].items()} for g in ("slot+0", "slot+1")})
    if nm == "window":
        print("  window AUC busy vs quiet: k@s0 %.3f k@s1 %.3f price@s0 %.3f price@s1 %.3f" % tuple(rec[x] for x in ("auc_k_slot+0", "auc_k_slot+1", "auc_price_slot+0", "auc_price_slot+1")))
        print("  busy share by k@slot+1:", rec["busy_share_by_k_slot+1"])

# ---------------------------------------------------------------- 2. ceilings
out["ceilings"] = {}
for nm in OOS:
    T = sets[nm]
    V = [pe.TV(t) for t in T]
    w = np.array([t.w for t in T])
    q = [sim.tokens_for(0.5, v.base) / 1.011 for v in V]
    lead = np.array([pe.run_lead(v).pnl / 0.5 for v in V])
    rec = dict(lead=wm(lead, w))
    for H in (5, 20):   # best end-of-slot fill price among slots se+1 .. se+H (exit decided with hindsight)
        best = []
        for v, qq in zip(V, q):
            vals = [pe.sell_value(float(v.mc[v.eos_idx(s)]), qq, qq) for s in range(v.se + 1, v.se + H + 1)]
            best.append((max(vals) - 0.5 - 0.004) / 0.5)
        rec[f"oracle_exit_by_slot+{H}"] = wm(np.array(best), w)
    immediate = np.array([(pe.sell_value(v.fill_mc(v.se), qq, qq) - 0.5 - 0.004) / 0.5 for v, qq in zip(V, q)])
    rec["exit_at_landing+1"] = wm(immediate, w)
    if nm == "window":
        busy = np.array([t.mode != "full_1page" for t in T])
        bail = np.where(busy, lead, immediate)
        rec["oracle_bail_quiet"] = wm(bail, w)
        rec["oracle_bail_quiet_quiet_only"] = wm(immediate, w, ~busy)
        rec["lead_quiet"] = wm(lead, w, ~busy)
        rec["oracle_skip_quiet_per_trade"] = wm(lead, w, busy)
        rec["oracle_skip_quiet_trade_share"] = float(busy.mean())
    out["ceilings"][nm] = rec
    print("ceiling", nm, {k: (round(v * 100, 2) if isinstance(v, float) else v) for k, v in rec.items()})

# ---------------------------------------------------------------- 3. drift after TP
out["post_tp_drift"] = {}
for nm in OOS:
    T = sets[nm]
    rows = []
    for t in T:
        v = pe.TV(t)
        o = pe.run_lead(v)
        if o.reason != "tp":
            continue
        f = o.extra["f"]
        m0 = v.fill_mc(f)
        rows.append([v.mc[v.eos_idx(f + 1 + h)] / m0 - 1 for h in (2, 5, 10, 20)] +
                    [float(v.mc[v.jfrom_slot(f + 2):v.eos_idx(f + 21) + 1].max(initial=m0)) / m0 - 1])
    a = np.array(rows)
    out["post_tp_drift"][nm] = dict(n=len(a), mean_at=[float(x) for x in a[:, :4].mean(0)], median_at=[float(x) for x in np.median(a[:, :4], 0)],
                                    share_up_at10=float((a[:, 2] > 0).mean()), median_max20=float(np.median(a[:, 4])))
    print("post-TP drift", nm, out["post_tp_drift"][nm])

# ---------------------------------------------------------------- 4. re-entry leg alone (the frozen config)
fz = json.load(open(REPO / "data/reports/strategy/frozen_refinements.json"))
re_cfg = next(f for f in fz["frozen"] if f["fam"] == "reentry")["cfg"]["p"]
out["reentry_leg"] = {}
for nm in OOS:
    legs = []
    for t in sets[nm]:
        o = pe.run_reentry(pe.TV(t), **re_cfg)
        if o.extra.get("re"):
            legs.append(o.extra["re_pnl"] / 0.25)
    legs = np.array(legs)
    out["reentry_leg"][nm] = dict(n=len(legs), share=len(legs) / len(sets[nm]), ev_leg=float(legs.mean()) if len(legs) else None,
                                  hit=float((legs > 0).mean()) if len(legs) else None)
    print("re-entry leg", nm, out["reentry_leg"][nm])

# ---------------------------------------------------------------- 5. size vs sniper drop (average over 5 seeds)
out["size_drop"] = {}
for f in (0.0, 0.05, 0.10, 0.15, 0.25, 0.5):
    rec = {}
    for S in (0.25, 0.5, 1.0):
        evs = {}
        for nm in ("test", "deep", "fresh", "window"):
            vals = []
            for seed in ((11,) if f == 0 else (11, 12, 13, 14, 15)):
                T = scen_sets({nm: sets[nm]}, BASE, drop=f, seed=seed)[nm]
                w = np.array([t.w for t in T])
                r = np.array([pe.run_lead(pe.TV(t), S).pnl / S for t in T])
                vals.append(wm(r, w))
            evs[nm] = float(np.mean(vals))
        rec[str(S)] = evs
    out["size_drop"][str(f)] = rec
    print("drop", f, {S: {k: round(v * 100, 2) for k, v in e.items()} for S, e in rec.items()})

json.dump(out, open(REPO / "data/reports/strategy/refinements_diag.json", "w"), indent=1, default=float)
print("written")


# ---------------------------------------------------------------- 6. pure time exits and bail-by-k (diagnostic)
def time_exit(v, j, S=0.5, prem=0.011, cost=0.004):
    q = sim.tokens_for(S, v.base) / (1 + prem)
    return (pe.sell_value(v.fill_mc(v.se + j), q, q) - S - cost) / S


out["time_exits"] = {}
for nm in ("train",) + OOS:
    T = sets[nm]
    V = [pe.TV(t) for t in T]
    w = np.array([t.w for t in T])
    lead = np.array([pe.run_lead(v).pnl / 0.5 for v in V])
    k0 = np.array([v.third_buys_by(v.se) for v in V])
    rec = dict(lead=wm(lead, w))
    for j in (0, 1, 2, 3, 5):
        te = np.array([time_exit(v, j) for v in V])
        rec[f"sell_fire_slot+{j}"] = wm(te, w)
        if j == 0:
            rec["k0=0: lead"] = wm(lead, w, k0 == 0)
            rec["k0=0: sell_fire_slot+0"] = wm(te, w, k0 == 0)
            rec["k0>=1: lead"] = wm(lead, w, k0 >= 1)
            rec["k0>=1: sell_fire_slot+0"] = wm(te, w, k0 >= 1)
            if nm == "window":
                busy = np.array([t.mode != "full_1page" for t in T])
                rec["quiet: lead"] = wm(lead, w, ~busy); rec["quiet: sell_fire_slot+0"] = wm(te, w, ~busy)
                rec["busy: lead"] = wm(lead, w, busy); rec["busy: sell_fire_slot+0"] = wm(te, w, busy)
    out["time_exits"][nm] = rec
    print("time exits", nm, {k: round(v * 100, 2) for k, v in rec.items()})
json.dump(out, open(REPO / "data/reports/strategy/refinements_diag.json", "w"), indent=1, default=float)
