"""S1 stage 2: focused refinement of the only family that came near break-even in the main sweep
(E1 rank-1 entry right after the bundle, very short "scalp" exits).

  .venv/bin/python -I analysis/scripts/strategy/refine.py select     # TRAIN only -> frozen_policies.json
  .venv/bin/python -I analysis/scripts/strategy/refine.py evaluate   # frozen set on test / odd / deep / fresh

Selection uses ONLY the gap-3 train split (first 60 % of launches by create time). The frozen file is
written before any out-of-sample number of the stage-2 grid is computed. The fresh holdout (light-sample
launches from 2026-10-12..14, pulled by pull_holdout.py) was never used in any selection.
"""
from __future__ import annotations

import json
import sys
import time
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
sys.path.insert(0, str(REPO / "analysis" / "scripts" / "strategy"))
from rugsnipe_analysis.strategy import sim, stats  # noqa: E402
from rugsnipe_analysis.strategy.tapes import build_tape, load_deep, load_gap3  # noqa: E402
from sweep import PER_DAY, filters_for, splits_for  # noqa: E402

OUT = REPO / "data" / "reports" / "strategy"
FROZEN = OUT / "frozen_policies.json"
ENTRY = "E1"
SIZES = (0.25, 0.5)
PREM, COST, LAT = 0.011, 0.004, 1
FILTERS = ("all", "me<=40", "me<=45", "me<=50", "multi", "dev_only")

SPECS = ([("tp", x) for x in (0.05, 0.075, 0.10, 0.15, 0.20, 0.30)]
         + [("ins", "any"), ("ins", "bundle"), ("ins", "creator"), ("snipersell",), ("anysell", 0.5), ("anysell", 1.0), ("anysell", 2.0)]
         + [("bigsell", x) for x in (0.005, 0.01, 0.02)]
         + [("idle", x) for x in (1, 1.5, 2, 3)]
         + [("bigbuy", x) for x in (1.0, 2.0, 3.0)]
         + [("psl", d, s) for d in (1, 3) for s in (0.10, 0.15)]
         + [("slotexit", k) for k in (1, 2, 3, 5, 10)] + [("hold", h) for h in (10, 20, 30, 60)])
KIND = [sp[0] for sp in SPECS]
GROUPS = {
    "flow": [i for i, sp in enumerate(SPECS) if sp[0] in ("ins", "snipersell", "anysell")],
    "bigsell": [i for i, k in enumerate(KIND) if k == "bigsell"],
    "idle": [i for i, k in enumerate(KIND) if k == "idle"],
    "bigbuy": [i for i, k in enumerate(KIND) if k == "bigbuy"],
    "psl": [i for i, k in enumerate(KIND) if k == "psl"],
}


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def load_fresh():
    p = REPO / "data/cache/strategy/holdout_trades.jsonl"
    out = []
    for line in open(p):
        r = json.loads(line)
        if not r["complete"] or not r["trades"]:
            continue
        C = {r["creator"], r["user"], r["fee_payer"]}
        t = build_tape(r["mint"], "fresh", 1.0, r["slot"], r["bt"], r["sig"], C, r["trades"], r["hor"], True)
        if t is not None:
            out.append(t)
    out.sort(key=lambda t: (t.bt, t.mint))
    return out


def filt(tapes):
    f = filters_for(tapes)
    f["multi"] = np.array([t.nB >= 1 for t in tapes])
    return {k: f[k] for k in FILTERS}


def returns(tapes, P, size, prem=PREM, cost=COST, lat=LAT):
    F, R, trig, _ = sim.simulate_entry(tapes, ENTRY, sim.Scenario(size, prem, cost, lat), SPECS)
    return sim.eval_policies(F, R, P), trig


def summarize(r, w, B=2000):
    if len(r) == 0:
        return dict(n=0)
    lo, hi, p = stats.boot(r, w, B=B)
    win = r > 0
    return dict(n=int(len(r)), ev=stats.wmean(r, w), lo=lo, hi=hi, p=p, hit=float(np.sum(w * win) / w.sum()),
                avg_win=stats.wmean(r[win], w[win]) if win.any() else None,
                avg_loss=stats.wmean(r[~win], w[~win]) if (~win).any() else None)


def select():
    tapes = load_gap3()
    w = np.array([t.w for t in tapes])
    F_ = filt(tapes); S_ = splits_for(tapes)
    P = sim.enumerate_policies(3, SPECS, GROUPS, "tp", ("slotexit", "hold"))
    log("stage-2 exit policies", len(P), "x filters", len(FILTERS), "x sizes", len(SIZES))
    cands = []
    n_cfg = 0
    for size in SIZES:
        ret, trig = returns(tapes, P, size)
        for fn in FILTERS:
            m = trig & F_[fn] & S_["train"]
            rr, ww = np.nan_to_num(ret[m]), w[m]
            sw = ww.sum()
            mu = (ww @ rr) / sw
            var = (ww ** 2) @ (rr - mu) ** 2
            se = np.sqrt(var * len(ww) / (len(ww) - 1)) / sw
            lb = mu - 1.645 * se
            n_cfg += len(P)
            for j in np.argsort(-lb)[:50]:
                cands.append(dict(size=size, filter=fn, prow=P[j].tolist(), exit=sim.policy_name(P[j], SPECS),
                                  train_ev=float(mu[j]), train_lb=float(lb[j]), n_train=int(m.sum())))
    cands.sort(key=lambda c: -c["train_lb"])
    frozen, seen = [], set()
    for c in cands:                                   # top 5 by train lower bound, one per (size, filter)
        key = (c["size"], c["filter"])
        if key in seen or c["n_train"] < 100:
            continue
        seen.add(key)
        frozen.append(c)
        if len(frozen) == 5:
            break
    doc = dict(frozen_at=time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime()), rule=(
        "E1 rank-1 entry; base scenario prem +1.1%, 0.004 SOL/round trip, 1-slot exit latency; "
        "score = train weighted mean - 1.645 SE on gap-3 TRAIN (first 60% by create time) only; "
        "n_train >= 100; top 5, at most one per (size, filter)"),
        configs_searched=n_cfg, n_exit_policies=int(len(P)), specs=[list(s) for s in SPECS], policies=frozen,
        top_train_candidates=cands[:30])
    json.dump(doc, open(FROZEN, "w"), indent=1)
    for c in frozen:
        log(f"FROZEN size={c['size']} {c['filter']:8s} {c['exit']:50s} train {c['train_ev']:+.4f} lb {c['train_lb']:+.4f} n={c['n_train']}")
    log("configs searched", n_cfg)


def evaluate():
    doc = json.load(open(FROZEN))
    sets = {"gap3": load_gap3(), "deep": load_deep(), "fresh": load_fresh()}
    log({k: len(v) for k, v in sets.items()})
    S3 = splits_for(sets["gap3"])
    res = dict(frozen=doc, sets={k: len(v) for k, v in sets.items()}, per_policy=[])
    stress = [(PREM, COST, LAT), (0.0, COST, LAT), (0.036, COST, LAT), (0.108, COST, LAT), (PREM, 0.01, LAT),
              (0.036, 0.01, LAT), (PREM, COST, 0), (PREM, COST, 2), (PREM, COST, 4)]
    for pol in doc["policies"]:
        P = np.array([pol["prow"]], dtype=np.int32)
        rec = dict(pol)
        rec["eval"] = {}
        for (pr, co, la) in stress:
            key = f"p{pr}_c{co}_l{la}"
            ev = {}
            for nm, tapes in sets.items():
                w = np.array([t.w for t in tapes])
                ret, trig = returns(tapes, P, pol["size"], pr, co, la)
                ret = ret[:, 0]
                fm = filt(tapes)[pol["filter"]] & trig
                if nm == "gap3":
                    for sp in ("train", "test", "even", "odd", "all"):
                        m = fm & S3[sp]
                        ev[f"gap3_{sp}"] = summarize(ret[m], w[m], B=2000 if key.startswith(f"p{PREM}_c{COST}_l{LAT}") else 300)
                    for reg in ("pre", "post"):
                        m = fm & np.array([t.regime == reg for t in tapes])
                        ev[f"gap3_{reg}"] = summarize(ret[m], w[m], B=300)
                    ev["per_day"] = float(w[fm].sum() * PER_DAY)
                else:
                    ev[nm] = summarize(ret[fm], w[fm])
                    if nm == "fresh":
                        ev["fresh_ret"] = ret[fm].tolist()
                    if nm == "deep":
                        ev["deep_ret"] = ret[fm].tolist()
            # pooled out-of-sample: gap-3 test + deep + fresh (equal launch weight within each source)
            rec["eval"][key] = ev
        res["per_policy"].append(rec)
        b = rec["eval"][f"p{PREM}_c{COST}_l{LAT}"]
        log(f"{pol['size']} {pol['filter']:8s} {pol['exit']:45s} train {b['gap3_train']['ev']:+.4f} test {b['gap3_test']['ev']:+.4f} "
            f"[{b['gap3_test']['lo']:+.3f},{b['gap3_test']['hi']:+.3f}] deep {b['deep'].get('ev', float('nan')):+.4f} (n={b['deep']['n']}) "
            f"fresh {b['fresh'].get('ev', float('nan')):+.4f} [{b['fresh'].get('lo', float('nan')):+.3f},{b['fresh'].get('hi', float('nan')):+.3f}] (n={b['fresh']['n']})")
    # BH over (5 frozen policies) x (3 OOS sets), bootstrap p for EV > 0
    keys, ps = [], []
    for i, rec in enumerate(res["per_policy"]):
        b = rec["eval"][f"p{PREM}_c{COST}_l{LAT}"]
        for s in ("gap3_test", "deep", "fresh"):
            keys.append((i, s)); ps.append(b[s].get("p", 1.0) if b[s].get("n", 0) else 1.0)
    qs = stats.bh_qvalues(np.array(ps))
    res["bh"] = [dict(policy=i, set=s, p=float(p), q=float(q)) for (i, s), p, q in zip(keys, ps, qs)]
    json.dump(res, open(OUT / "refine_results.json", "w"), indent=1, default=float)
    log("written", OUT / "refine_results.json")


if __name__ == "__main__":
    {"select": select, "evaluate": evaluate}[sys.argv[1]]()
