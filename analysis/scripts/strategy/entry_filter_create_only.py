"""Create-only variant of the LEAD entry filter (entry_filter_v1_create_only).

The live hot loop has to decide before the bundle run behind the create streams in (`feed_bundle` is assembled
after the decision), so the 17 bundle-shape features of entry_filter_v1 are not known when it fires. This script
fits the same model (G1 rule 3: gradient-boosted trees, depth 3, P(take-profit), operator pools as training rows and
history) on the create-time features only (entry_filter.CREATE_ONLY_FEATURES: create transaction, uri, funding,
operator history as of the launch, time and create rate, plus the in-transaction buy count), with the same code path
and the same holdout protocol as entry_filter_freeze.py:

  check   fit on the predictor table only, score once on the 480 random HA / HB / HC holdout launches (the
          pre-registered fresh holdout of docs/04 section 3a): skip-bottom-tertile uplift of the full live model,
          of the create-only model, and of the full model with the bundle block blanked (what scoring v1 before the
          bundle arrives would do), with signer-clustered 90% bootstrap CIs and the paired difference;
  freeze  refit on every row and export data/reports/strategy/entry_filter_v1_create_only.json in the format of
          entry_filter_v1.json (frozen tertile cut points), refusing to overwrite without --force.

  ../.venv/bin/python -I scripts/strategy/entry_filter_create_only.py [--force]      (from analysis/)
"""
from __future__ import annotations

import hashlib
import json
import pickle
import sys
import time
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
sys.path.insert(0, str(REPO / "analysis/scripts/strategy"))
from rugsnipe_analysis.strategy import entry_filter as EF  # noqa: E402
from rugsnipe_analysis.strategy import predictor_models as M  # noqa: E402
from entry_filter_freeze import _tertile_table  # noqa: E402

OUT = REPO / "data" / "reports" / "strategy"
DOC = OUT / "entry_filter_v1_create_only.json"
PKL = OUT / "entry_filter_v1_create_only.pkl"
CO = [f for f, _ in EF.CREATE_ONLY_FEATURES]
B = 4000


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def _cuts(ptr, w, pool):
    return EF.wquant(ptr[~pool], w[~pool], 1 / 3), EF.wquant(ptr[~pool], w[~pool], 2 / 3)


def _lift(p, ret, t1, idx):
    keep = p[idx] >= t1
    if keep.sum() == 0:
        return 0.0
    return float(ret[idx][keep].mean() - ret[idx].mean())


def cluster_ci(stat, groups, seed=11):
    """90% CI of stat(index array) resampling whole signers."""
    ug, inv = np.unique(groups, return_inverse=True)
    members = [np.nonzero(inv == k)[0] for k in range(len(ug))]
    rng = np.random.default_rng(seed)
    vals = np.empty(B)
    for b in range(B):
        pick = rng.integers(0, len(ug), len(ug))
        vals[b] = stat(np.concatenate([members[k] for k in pick]))
    return [float(np.quantile(vals, 0.05)), float(np.quantile(vals, 0.95))], float(np.mean(vals <= 0))


def check(table, hold):
    hs = [r for r in hold if not r["pool"]]
    ret = np.array([r["ret_base"] for r in hs])
    tp = np.array([r["tp_hit"] for r in hs], dtype=float)
    src = np.array([r["src"] for r in hs])
    sig = np.array([r["signer"] or r["mint"] for r in hs])
    sets = {s: src == s for s in ("HA", "HB", "HC")} | {"pooled": np.ones(len(hs), bool)}
    out, scores = {}, {}
    for name, feats in (("full_v1_features", EF.FEATURES), ("create_only", CO)):
        Xt, yt, wt, pt = EF.design(table, feats)
        model, ptr = EF.fit(Xt, yt, wt)
        t1, t2 = _cuts(ptr, wt, pt)
        Xh, _, _, _ = EF.design(hs, feats)
        p = model.predict_proba(Xh)[:, 1]
        scores[name] = (p, t1)
        out[name] = dict(n_features=len(feats), t1=t1, t2=t2, scored=_tertile_table(p, ret, tp, t1, t2, sets))
        if name == "full_v1_features":
            # v1 scored before the bundle run is known: bundle block at 0 (feature_row's value for "no bundle")
            Xb = Xh.copy()
            for j, (f, g) in enumerate(EF.LIVE_FEATURES):
                if g == "bundle":
                    Xb[:, j] = 0.0
            pb = model.predict_proba(Xb)[:, 1]
            scores["full_bundle_blanked"] = (pb, t1)
            out["full_bundle_blanked"] = dict(note="full model, bundle features set to 0 at scoring (fit unchanged)",
                                              t1=t1, t2=t2, scored=_tertile_table(pb, ret, tp, t1, t2, sets))
    allidx = np.arange(len(hs))
    for name, (p, t1) in scores.items():
        ci, pval = cluster_ci(lambda i, p=p, t1=t1: _lift(p, ret, t1, i), sig)
        out[name]["skip_bottom_lift_pooled"] = dict(lift=_lift(p, ret, t1, allidx), ci90_signer=ci, p_le_0=pval)
    pf, tf = scores["full_v1_features"]
    pc, tc = scores["create_only"]
    d = lambda i: _lift(pc, ret, tc, i) - _lift(pf, ret, tf, i)
    ci, pval = cluster_ci(d, sig)
    out["create_only_minus_full"] = dict(diff=d(allidx), ci90_signer=ci, p_le_0=pval,
                                         note="paired difference of the skip-bottom-tertile lifts on the same 480 launches")
    kc, kf = pc >= tc, pf >= tf
    out["overlap"] = dict(kept_both=int((kc & kf).sum()), kept_create_only_only=int((kc & ~kf).sum()),
                          kept_full_only=int((~kc & kf).sum()), dropped_both=int((~kc & ~kf).sum()))
    out["holdout"] = dict(n=len(hs), by_source={s: int((src == s).sum()) for s in ("HA", "HB", "HC")},
                          signers=int(len(np.unique(sig))))
    return out


def main():
    table, hold = EF.table_rows(), EF.holdout_rows()
    log("rows: table", len(table), "holdout", len(hold), "create-only features", len(CO))
    chk = check(table, hold)
    for k in ("full_v1_features", "create_only", "full_bundle_blanked"):
        b = chk[k]["scored"]["pooled"]
        L = chk[k]["skip_bottom_lift_pooled"]
        log(f"{k:20s} AUC {b['auc_tp']:.3f} keep {b['skip_bottom']['keep']*100:.0f}% EV kept "
            f"{b['skip_bottom']['ev_kept']*100:+.2f} vs all {b['ev_all']*100:+.2f} lift {L['lift']*100:+.2f} "
            f"[{L['ci90_signer'][0]*100:+.2f}, {L['ci90_signer'][1]*100:+.2f}] | "
            + " ".join(f"{s}: {chk[k]['scored'][s]['skip_bottom']['lift']*100:+.2f}" for s in ("HA", "HB", "HC")))
    dd = chk["create_only_minus_full"]
    log(f"create-only minus full: {dd['diff']*100:+.2f} [{dd['ci90_signer'][0]*100:+.2f}, {dd['ci90_signer'][1]*100:+.2f}]")
    if DOC.exists() and "--force" not in sys.argv:
        log(f"{DOC} exists; not refreezing (--force). Check numbers above are not written.")
        return
    import sklearn
    rows = sorted(table + hold, key=lambda r: (r["bt"], r["mint"]))
    X, y, w, pool = EF.design(rows, CO)
    model, ptr = EF.fit(X, y, w)
    rnd = ~pool
    t1, t2 = _cuts(ptr, w, pool)
    blob = pickle.dumps(model)
    PKL.write_bytes(blob)
    v1 = EF.load_doc()
    doc = dict(
        name="entry_filter_v1_create_only",
        frozen_at=time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime()),
        what=("entry_filter_v1 (G1 rule 3, gradient-boosted trees depth 3, P(take-profit) of the LEAD) re-fitted on the "
              "create-time features only: no bundle-run features (they stream in after the decision); rule: skip if "
              "score < t1 (bottom training tertile)"),
        code="analysis/rugsnipe_analysis/strategy/entry_filter.py, analysis/scripts/strategy/entry_filter_create_only.py",
        features=CO,
        feature_groups={f: g for f, g in EF.CREATE_ONLY_FEATURES},
        dropped=dict(EF.DROPPED, **{"bundle run (nB .. tok_pct_insiders, 17 features)":
                                    "the creation slot's bundle run behind the create streams in after the decision "
                                    "(feed_bundle); only the create transaction is known when the hot loop fires"}),
        new_features={"intx_n_buys": "number of pump.fun buy instructions inside the create transaction (0 when none); "
                                     "predictor.create_tx_features"},
        defaults=EF.DEFAULTS,
        scaler=None,
        model=EF.export_gbt(model),
        tertiles=dict(t1=t1, t2=t2, protocol=v1["tertiles"]["protocol"]),
        hyper=dict(v1["hyper"], sklearn=sklearn.__version__),
        training=dict(n=len(rows), n_random=int(rnd.sum()), n_pool=int(pool.sum()),
                      tp_rate_random=float(np.sum(w[rnd] * y[rnd]) / w[rnd].sum()), first_bt=int(rows[0]["bt"]),
                      last_bt=int(rows[-1]["bt"])),
        constants=v1["constants"],
        pkl_sha256=hashlib.sha256(blob).hexdigest(),
        pre_freeze_check=chk,
        in_sample=_tertile_table(ptr, np.array([r["ret_base"] for r in rows]), y, t1, t2, {"random rows": rnd}),
    )
    DOC.write_text(json.dumps(doc, indent=1))
    log("model", len(doc["model"]["trees"]), "trees, t1", round(t1, 4), "t2", round(t2, 4), "->", DOC)


if __name__ == "__main__":
    main()
