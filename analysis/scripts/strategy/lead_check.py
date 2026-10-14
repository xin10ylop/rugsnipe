"""Post-hoc checks on the frozen lead (refine policy #3): pooled out-of-sample estimate, and a
parameter-neighbourhood sensitivity (NOT used for selection; shows whether the lead is a plateau or a spike)."""
import json
import sys
from itertools import product
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
sys.path.insert(0, str(REPO / "analysis" / "scripts" / "strategy"))
from rugsnipe_analysis.strategy import sim, stats  # noqa: E402
from rugsnipe_analysis.strategy.tapes import load_deep, load_gap3  # noqa: E402
from refine import filt, load_fresh  # noqa: E402
from sweep import PER_DAY, splits_for  # noqa: E402

doc = json.load(open(REPO / "data/reports/strategy/frozen_policies.json"))
lead = doc["policies"][2]
assert lead["exit"].startswith("tp15+ins_bundle+idle1"), lead
g3, deep, fresh = load_gap3(), load_deep(), load_fresh()
test = splits_for(g3)["test"]


def run(specs, size, fname, prem=0.011, cost=0.004, lat=1):
    out = {}
    pooled_r, pooled_w = [], []
    for nm, T, mask in (("train", g3, ~test), ("test", g3, test), ("deep", deep, None), ("fresh", fresh, None)):
        F, R, trig, _ = sim.simulate_entry(T, "E1", sim.Scenario(size, prem, cost, lat), specs)
        P = np.array([list(range(len(specs)))])
        ret = sim.eval_policies(F, R, P)[:, 0]
        w = np.array([t.w for t in T])
        m = trig & filt(T)[fname] & (mask if mask is not None else True)
        out[nm] = (stats.wmean(ret[m], w[m]), int(m.sum()))
        if nm != "train":
            # equal total weight per launch within each source: rescale gap-3 weights to mean 1
            ww = w[m] / w[m].mean()
            pooled_r.append(ret[m]); pooled_w.append(ww)
    r, w = np.concatenate(pooled_r), np.concatenate(pooled_w)
    lo, hi, p = stats.boot(r, w, B=4000)
    out["pooled_oos"] = dict(ev=stats.wmean(r, w), lo=lo, hi=hi, p=p, n=int(len(r)))
    return out


base_specs = [("tp", 0.15), ("ins", "bundle"), ("idle", 1), ("psl", 1, 0.15), ("slotexit", 5)]
res = {"lead": lead, "base": run(base_specs, 0.5, "multi")}
print("LEAD", json.dumps(res["base"]))
# size / filter sensitivity
res["size_filter"] = {}
for size, fname in product((0.1, 0.25, 0.5, 1.0), ("multi", "all", "me<=50")):
    o = run(base_specs, size, fname)
    res["size_filter"][f"{size}_{fname}"] = o
    print(f"size {size} {fname:7s} train {o['train'][0]:+.4f} test {o['test'][0]:+.4f} deep {o['deep'][0]:+.4f} fresh {o['fresh'][0]:+.4f} pooled {o['pooled_oos']['ev']:+.4f} [{o['pooled_oos']['lo']:+.4f},{o['pooled_oos']['hi']:+.4f}]")
# neighbourhood: tp x idle x slot cap x (with/without ins_bundle) x (with/without psl)
rows = []
for tp, idle, cap, ins, psl in product((0.10, 0.15, 0.20), (1, 1.5, 2), (3, 5, 10), (True, False), (True, False)):
    sp = [("tp", tp), ("idle", idle), ("slotexit", cap)] + ([("ins", "bundle")] if ins else []) + ([("psl", 1, 0.15)] if psl else [])
    o = run(sp, 0.5, "multi")
    rows.append(dict(tp=tp, idle=idle, cap=cap, ins=ins, psl=psl, **{k: (v if k == "pooled_oos" else v[0]) for k, v in o.items()}))
res["neighbourhood"] = rows
po = np.array([r["pooled_oos"]["ev"] for r in rows])
tr = np.array([r["train"] for r in rows])
print(f"neighbourhood n={len(rows)}: pooled OOS EV median {np.median(po):+.4f}, min {po.min():+.4f}, max {po.max():+.4f}, share>0 {np.mean(po>0):.2f}; train median {np.median(tr):+.4f}")
# trades/day and SOL/day at the class level (upper bound: rank 1 on every qualifying launch)
w = np.array([t.w for t in g3])
m = filt(g3)["multi"]
res["class_per_day"] = float(w[m].sum() * PER_DAY)
json.dump(res, open(REPO / "data/reports/strategy/lead_check.json", "w"), indent=1, default=float)
