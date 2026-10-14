"""G1 diagnostics of the frozen P(TP) score (rule 3) on the Oct 12-14 sets: EV by score tertile (frozen
cut points), AUC with / without operator history, permutation importance. Post-hoc slicing of a frozen
score; no refitting.

  .venv/bin/python -I analysis/scripts/strategy/predictor_diag.py
"""
from __future__ import annotations

import json
import pickle
import sys
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
sys.path.insert(0, str(REPO / "analysis/scripts/strategy"))
from rugsnipe_analysis.strategy import predictor as P  # noqa: E402
from rugsnipe_analysis.strategy import predictor_models as M  # noqa: E402
from rugsnipe_analysis.strategy import stats  # noqa: E402
import predictor_wf as W  # noqa: E402


def main():
    doc = json.load(open(W.FROZEN))
    models = pickle.load(open(W.FROZEN_PKL, "rb"))
    ent = next(e for e in doc["rules"] if e["name"].endswith("tp:size_skip"))
    rows, d = W.load(ent["variant"])
    _, _, score = W.apply_rule(ent, d, models)
    mints = np.array([r["mint"] for r in rows])
    fresh_m = {t.mint for t in P.load_fresh() if t.kind == "multi"}
    win_m = {t.mint for t in P.load_window() if t.kind == "multi"}
    post = (d.bt >= M.OCT12) & ~d.pool
    sets = {"fresh": post & np.isin(mints, list(fresh_m)), "window": post & np.isin(mints, list(win_m)),
            "deep": post & (d.src == "deep"), "gap3_post": post & (d.src == "gap3"), "all_post": post,
            "dev_in_sample": (d.bt < M.OCT12) & ~d.pool}
    out = {}
    hist = d.X[:, d.names.index("op_has_hist")] > 0
    for sn, m in sets.items():
        w = d.w[m]
        res = {"n": int(m.sum())}
        for tn, lo, hi in (("low", -1, ent["t1"]), ("mid", ent["t1"], ent["t2"]), ("high", ent["t2"], 9)):
            s = m & (score >= lo) & (score < hi) if tn != "high" else m & (score >= lo)
            ws = d.w[s]
            if s.sum() < 2:
                continue
            lo_, hi_, p_ = stats.boot(d.ret["base"][s], ws)
            res[tn] = dict(n=int(s.sum()), share=float(ws.sum() / w.sum()), ev=stats.wmean(d.ret["base"][s], ws),
                           lo=lo_, hi=hi_, tp=stats.wmean(d.tp[s], ws), s0t=stats.wmean(d.s0t[s], ws),
                           busy=stats.wmean(d.busy[s][~np.isnan(d.busy[s])], ws[~np.isnan(d.busy[s])]) if (~np.isnan(d.busy[s])).any() else None,
                           ev_p036=stats.wmean(d.ret["p036"][s], ws), ev_c010=stats.wmean(d.ret["c010"][s], ws),
                           ev_s100=stats.wmean(d.ret["s100"][s], ws))
        res["auc_tp"] = M.auc(d.tp[m], score[m], w)
        res["auc_busy"] = M.auc(d.busy[m], score[m], w)
        for k, hm in (("hist", hist), ("nohist", ~hist)):
            mm = m & hm
            res[f"auc_tp_{k}"] = M.auc(d.tp[mm], score[mm], d.w[mm]) if mm.sum() > 10 else None
            res[f"n_{k}"] = int(mm.sum())
        out[sn] = res
        print(f"\n{sn}: n={res['n']} AUC tp={res['auc_tp']:.3f} busy={res['auc_busy']:.3f} | with op history n={res['n_hist']} "
              f"AUC={res['auc_tp_hist']} | without n={res['n_nohist']} AUC={res['auc_tp_nohist']}")
        for tn in ("low", "mid", "high"):
            if tn in res:
                r = res[tn]
                print(f"   {tn:4s} n={r['n']:4d} share={r['share']*100:3.0f}% EV={r['ev']*100:+6.2f}% [{r['lo']*100:+.1f},{r['hi']*100:+.1f}] "
                      f"TP={r['tp']:.2f} s0t={r['s0t']:.2f} busy={r['busy'] if r['busy'] is None else round(r['busy'],2)} "
                      f"p036={r['ev_p036']*100:+.1f} c010={r['ev_c010']*100:+.1f} 1SOL={r['ev_s100']*100:+.1f}")
    # permutation importance (AUC TP drop) on all_post, frozen model
    m = sets["all_post"]
    cj = np.array([d.names.index(n) for n in ent["features"]])
    model = models[ent["name"]]
    X = d.X[m][:, cj].copy()
    base = M.auc(d.tp[m], model.predict_proba(X)[:, 1], d.w[m])
    rng = np.random.default_rng(0)
    imp = []
    for k, n in enumerate(ent["features"]):
        drops = []
        for _ in range(5):
            Xp = X.copy()
            Xp[:, k] = rng.permutation(Xp[:, k])
            drops.append(base - M.auc(d.tp[m], model.predict_proba(Xp)[:, 1], d.w[m]))
        imp.append((float(np.mean(drops)), n))
    imp.sort(reverse=True)
    print("\npermutation importance (AUC-TP drop, all_post):")
    for v, n in imp[:20]:
        print(f"   {n:<22s} {v:+.4f}")
    out["perm_importance_all_post"] = [dict(feature=n, auc_drop=v) for v, n in imp]
    json.dump(out, open(REPO / "data/reports/strategy/predictor_diag.json", "w"), indent=1, default=float)


if __name__ == "__main__":
    main()
