"""Break-even premium x cost grid and per-day stability for the frozen lead (pooled out-of-sample sets)."""
import json
import sys
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
sys.path.insert(0, str(REPO / "analysis" / "scripts" / "strategy"))
from rugsnipe_analysis.strategy import sim, stats  # noqa: E402
from rugsnipe_analysis.strategy.tapes import load_deep, load_gap3  # noqa: E402
from refine import filt, load_fresh  # noqa: E402
from sweep import splits_for  # noqa: E402

specs = [("tp", 0.15), ("ins", "bundle"), ("idle", 1), ("psl", 1, 0.15), ("slotexit", 5)]
g3, deep, fresh = load_gap3(), load_deep(), load_fresh()
test = splits_for(g3)["test"]
P = np.array([list(range(len(specs)))])


def pooled(prem, cost, size=0.5, sets=("test", "deep", "fresh"), full=False):
    rs, ws, days = [], [], []
    for nm, T, mask in (("all_g3", g3, np.ones(len(g3), bool)), ("test", g3, test), ("deep", deep, None), ("fresh", fresh, None)):
        if nm not in sets:
            continue
        F, R, trig, _ = sim.simulate_entry(T, "E1", sim.Scenario(size, prem, cost, 1), specs)
        ret = sim.eval_policies(F, R, P)[:, 0]
        w = np.array([t.w for t in T])
        m = trig & filt(T)["multi"] & (mask if mask is not None else True)
        rs.append(ret[m]); ws.append(w[m] / w[m].mean()); days.append(np.array([t.bt // 86400 for t in T])[m])
    r, w, d = np.concatenate(rs), np.concatenate(ws), np.concatenate(days)
    if full:
        return r, w, d
    return stats.wmean(r, w)


grid = {}
for cost in (0.004, 0.01, 0.02):
    for prem in (0.0, 0.01, 0.02, 0.03, 0.04, 0.05, 0.06, 0.08, 0.108):
        grid[f"{prem}_{cost}"] = pooled(prem, cost)
        print(f"cost {cost} prem {prem:.3f} pooled OOS EV {grid[f'{prem}_{cost}']:+.4f}")
# per-day stability over every launch day (gap-3 all 16 days + deep + fresh; overlapping days merged)
r, w, d = pooled(0.011, 0.004, sets=("all_g3", "deep", "fresh"), full=True)
per_day = {}
for day in sorted(set(d.tolist())):
    m = d == day
    per_day[str(day)] = dict(n=int(m.sum()), ev=stats.wmean(r[m], w[m]))
pos = sum(1 for v in per_day.values() if v["ev"] > 0)
print("days", len(per_day), "positive", pos, {k: (v["n"], round(v["ev"], 3)) for k, v in per_day.items()})
json.dump(dict(grid=grid, per_day=per_day), open(REPO / "data/reports/strategy/lead_breakeven.json", "w"), indent=1)
