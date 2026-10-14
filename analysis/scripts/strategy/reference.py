"""Fixed, a-priori reference policies (not selected): the guru's rules and gap-3's best combo,
on gap-3 train/test/even/odd and the deep holdout, at the base scenario."""
import json
import sys
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
sys.path.insert(0, str(REPO / "analysis" / "scripts" / "strategy"))
from rugsnipe_analysis.strategy import sim, stats  # noqa: E402
from rugsnipe_analysis.strategy.tapes import load_deep, load_gap3  # noqa: E402
from sweep import BASE, filters_for, splits_for  # noqa: E402

REFS = [
    ("guru: TP +100%, no stop (hold to 900 s)", "E1", ["tp100", "hold900"]),
    ("guru bot: TP +100% + phantom SL 10 s -25% + sell on big buy 2 SOL + no-activity 10 s", "E1", ["tp100", "psl10s-25", "bigbuy2", "idle10", "hold900"]),
    ("guru: buy on dev/insider sell, TP +100%", "INS", ["tp100", "hold900"]),
    ("guru: snipe delay (end of slot+1), TP +100%", "E2", ["tp100", "hold900"]),
    ("gap-3 best: TP +100% + idle 3 s + sell>=2% supply", "E1", ["tp100", "idle3", "bigsell2", "hold900"]),
    ("gap-3 rec: TP +40% + insider sell + idle 3 s", "E1", ["tp40", "ins_any", "idle3", "hold900"]),
]
tapes, deep = load_gap3(), load_deep()
out = []
for name, ent, comps in REFS:
    row = [sim.CIDX[c] for c in comps]
    row = np.array([row + [sim.DUMMY] * (5 - len(row))])
    rec = dict(name=name, entry=ent, exit="+".join(comps))
    for tag, T, spl in (("gap3", tapes, splits_for(tapes)), ("deep", deep, {"all": np.ones(len(deep), bool)})):
        w = np.array([t.w for t in T])
        F, R, trig, _ = sim.simulate_entry(T, ent, BASE)
        ret = sim.eval_policies(F, R, row)[:, 0]
        for s, m in spl.items():
            mm = m & trig
            lo, hi, p = stats.boot(ret[mm], w[mm], B=1000)
            rec[f"{tag}_{s}"] = dict(n=int(mm.sum()), ev=stats.wmean(ret[mm], w[mm]), lo=lo, hi=hi,
                                     hit=float(np.sum(w[mm] * (ret[mm] > 0)) / w[mm].sum()))
    out.append(rec)
    g = rec
    print(f"{name[:70]:70s} train {g['gap3_train']['ev']:+.3f} test {g['gap3_test']['ev']:+.3f} [{g['gap3_test']['lo']:+.3f},{g['gap3_test']['hi']:+.3f}] deep {g['deep_all']['ev']:+.3f} hit {g['gap3_all']['hit']:.2f}")
json.dump(out, open(REPO / "data/reports/strategy/reference.json", "w"), indent=1)
