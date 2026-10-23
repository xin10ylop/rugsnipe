"""Demand classifier for the LEAD: P(>= 2 outside buyers land behind us in the create slot), exported in the
entry-filter JSON schema so ``crates/rugsnipe/src/filter.rs`` (``Model::from_json``) loads it unchanged.

  .venv/bin/python -I analysis/scripts/strategy/demand_freeze.py [--force]

Why: live LEAD fills at 0.25 SOL are decided by how many third-party buyers land behind us in the create slot
(``shadow.tp_behind``); fills with tp_behind >= 2 average positive, fills with nobody behind lose the round trip.

Label: ``s0_third >= 2`` (``rugsnipe_analysis.strategy.tapes.build_tape``): the number of distinct wallets that buy in
the creation slot and are neither in the creator set (creator / signer / fee payer) nor bundle wallets (the gap-3
run behind the create). The run is contiguous from the create, so every such buyer lands after the last bundle trade
``pb``, which is where the dry run puts the LEAD (``sim_at(t, t.pb, t.S0)``): in the dry run the LEAD is first by
construction (tp_ahead = 0) and ``s0_third`` is exactly what ``e1.rs`` / ``real.rs`` count as ``tp_behind`` (distinct
non-insider buyers after our index in the create slot). One difference: e1.rs counts a known sniper inside the run as
ahead; the Python tape folds it into the bundle.

Same machinery as entry_filter_freeze.py: ``EF.design`` (the 60 ``EF.FEATURES``, ``feature_row`` with history
``op_all``), ``EF.fit`` (HistGradientBoosting depth 3, lr 0.05, 150 iterations, min leaf 20, l2 1.0, sample weight
``w``), ``EF.export_gbt``, tertiles by ``EF.wquant`` on the random-sample rows. Only the target changes.

Protocol: dev model fit on the earlier 70 % of the predictor table (by block time, 15-minute embargo), scored on the
later 30 % (test) and on the HA / HB / HC holdout rows (all later than the table). The lift tables use the dev model
and the dev tertile cuts. The exported model (demand_v1.json) is refit on every row (table + holdout), with cut points
from its in-sample random-row predictions (the entry_filter_v1 protocol); both cut sets are stored under ``demand``.
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
from rugsnipe_analysis.strategy import entry_filter as EF  # noqa: E402
from rugsnipe_analysis.strategy import predictor_models as M  # noqa: E402

DOC = EF.OUT / "demand_v1.json"
PKL = EF.OUT / "demand_v1.pkl"
CHECK = EF.OUT / "demand_v1_check.json"
K = 2                 # label: s0_third >= K
TRAIN_FRAC = 0.7
EMBARGO = 900
SIZE = 0.25           # ret_s025: LEAD return (fraction of a 0.25 SOL buy, net of costs)
B = 2000
N_CHECK = 200


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def label(rows: list[dict]) -> np.ndarray:
    return np.array([float((r.get("s0_third") or 0) >= K) for r in rows])


def cluster_ci(x: np.ndarray, cl: np.ndarray, seed: int = 0) -> dict:
    """Cluster bootstrap (resample creators with replacement): 95 % CI of the mean and of the total."""
    if len(x) == 0:
        return dict(mean=float("nan"), total=0.0, mean_ci=(float("nan"),) * 2, total_ci=(float("nan"),) * 2)
    u, inv = np.unique(cl, return_inverse=True)
    sums = np.bincount(inv, weights=x, minlength=len(u))
    cnts = np.bincount(inv, minlength=len(u)).astype(float)
    ii = np.random.default_rng(seed).integers(0, len(u), (B, len(u)))
    s, c = sums[ii].sum(1), cnts[ii].sum(1)
    m = s / np.maximum(c, 1)
    return dict(mean=float(x.mean()), total=float(x.sum()),
                mean_ci=tuple(float(v) for v in np.quantile(m, [0.025, 0.975])),
                total_ci=tuple(float(v) for v in np.quantile(s, [0.025, 0.975])))


def lift_ci(x: np.ndarray, keep: np.ndarray, cl: np.ndarray, seed: int = 1) -> dict:
    """Paired cluster bootstrap of skip-bottom minus take-all: mean per fill and total."""
    u, inv = np.unique(cl, return_inverse=True)
    sa = np.bincount(inv, weights=x, minlength=len(u))
    ca = np.bincount(inv, minlength=len(u)).astype(float)
    sk = np.bincount(inv, weights=x * keep, minlength=len(u))
    ck = np.bincount(inv, weights=keep.astype(float), minlength=len(u))
    ii = np.random.default_rng(seed).integers(0, len(u), (B, len(u)))
    dm = sk[ii].sum(1) / np.maximum(ck[ii].sum(1), 1) - sa[ii].sum(1) / np.maximum(ca[ii].sum(1), 1)
    dt = sk[ii].sum(1) - sa[ii].sum(1)
    pt = float(x[keep].mean() - x.mean()) if keep.any() else float("nan")
    return dict(mean=pt, mean_ci=tuple(float(v) for v in np.quantile(dm, [0.025, 0.975])),
                total=float(x[keep].sum() - x.sum()), total_ci=tuple(float(v) for v in np.quantile(dt, [0.025, 0.975])),
                p_total_le0=float(np.mean(dt <= 0)))


def lift_table(name: str, p, y, ret, cl, t1, t2) -> dict:
    """Per dev-tertile: n, label share, mean ret_s025 per fill (CI), total SOL at 0.25 (CI); skip-bottom vs all."""
    sol = ret * SIZE
    tert = np.where(p < t1, 0, np.where(p < t2, 1, 2))
    out = dict(n=int(len(p)), auc=M.auc(y, p), tertiles={})
    print(f"\n[{name}] n={len(p)}  AUC(label)={out['auc']:.3f}  label share={y.mean()*100:.1f}%")
    print(f"  {'tertile':<12}{'n':>5}{'share':>7}{'lbl=1':>7}{'mean ret':>10}{'95% CI (mean)':>20}"
          f"{'total SOL':>11}{'95% CI (total)':>20}")
    for k, nm in enumerate(("low", "mid", "high")):
        m = tert == k
        c = cluster_ci(ret[m], cl[m], seed=k)
        ct = cluster_ci(sol[m], cl[m], seed=k)
        out["tertiles"][nm] = dict(n=int(m.sum()), share=float(m.mean()), label=float(y[m].mean()) if m.any() else None,
                                   mean_ret=c["mean"], mean_ci=c["mean_ci"], total_sol=ct["total"],
                                   total_ci=ct["total_ci"])
        print(f"  {nm:<12}{int(m.sum()):>5}{m.mean()*100:>6.0f}%{(y[m].mean()*100 if m.any() else float('nan')):>6.0f}%"
              f"{c['mean']*100:>+9.2f}%   [{c['mean_ci'][0]*100:+6.2f}, {c['mean_ci'][1]*100:+6.2f}]"
              f"{ct['total']:>+11.3f}   [{ct['total_ci'][0]:+7.3f}, {ct['total_ci'][1]:+7.3f}]")
    keep = tert >= 1
    ca, ck = cluster_ci(ret, cl, 7), cluster_ci(ret[keep], cl[keep], 8)
    ta, tk = cluster_ci(sol, cl, 7), cluster_ci(sol[keep], cl[keep], 8)
    lm = lift_ci(sol, keep, cl)
    lr = lift_ci(ret, keep, cl)
    out["take_all"] = dict(n=int(len(ret)), mean_ret=ca["mean"], mean_ci=ca["mean_ci"], total_sol=ta["total"],
                           total_ci=ta["total_ci"])
    out["skip_bottom"] = dict(n=int(keep.sum()), mean_ret=ck["mean"], mean_ci=ck["mean_ci"], total_sol=tk["total"],
                              total_ci=tk["total_ci"], lift_mean=lr["mean"], lift_mean_ci=lr["mean_ci"],
                              lift_total_sol=lm["total"], lift_total_ci=lm["total_ci"], p_lift_total_le0=lm["p_total_le0"])
    for nm, c, t in (("take all", ca, ta), ("skip bottom", ck, tk)):
        nn = len(ret) if nm == "take all" else int(keep.sum())
        print(f"  {nm:<12}{nn:>5}{'':>14}{c['mean']*100:>+9.2f}%   [{c['mean_ci'][0]*100:+6.2f}, "
              f"{c['mean_ci'][1]*100:+6.2f}]{t['total']:>+11.3f}   [{t['total_ci'][0]:+7.3f}, {t['total_ci'][1]:+7.3f}]")
    print(f"  lift (skip bottom - all): mean/fill {lr['mean']*100:+.2f}% [{lr['mean_ci'][0]*100:+.2f}, "
          f"{lr['mean_ci'][1]*100:+.2f}]  total {lm['total']:+.3f} SOL [{lm['total_ci'][0]:+.3f}, {lm['total_ci'][1]:+.3f}]"
          f"  P(total lift <= 0) = {lm['p_total_le0']:.3f}")
    return out


def main():
    if DOC.exists() and "--force" not in sys.argv:
        raise SystemExit(f"{DOC} exists; refusing to overwrite (--force)")
    import sklearn
    ef_doc = EF.load_doc()
    assert ef_doc["features"] == EF.FEATURES and len(EF.FEATURES) == 60
    table, hold = EF.table_rows(), EF.holdout_rows()
    table.sort(key=lambda r: (r["bt"], r["mint"]))
    log("rows: table", len(table), "holdout", len(hold))
    split = int(np.quantile([r["bt"] for r in table], TRAIN_FRAC))
    tr = [r for r in table if r["bt"] < split - EMBARGO]
    te = [r for r in table if r["bt"] >= split]
    log(f"time split at bt {split} ({time.strftime('%Y-%m-%d %H:%M', time.gmtime(split))} UTC): train {len(tr)} "
        f"(embargo {EMBARGO}s), test {len(te)}, holdout {len(hold)} "
        f"({time.strftime('%m-%d %H:%M', time.gmtime(min(r['bt'] for r in hold)))}..."
        f"{time.strftime('%m-%d %H:%M', time.gmtime(max(r['bt'] for r in hold)))} UTC)")

    Xtr, _, wtr, ptr_pool = EF.design(tr)
    ytr = label(tr)
    model, p_tr = EF.fit(Xtr, ytr, wtr)
    rnd = ~ptr_pool
    t1, t2 = EF.wquant(p_tr[rnd], wtr[rnd], 1 / 3), EF.wquant(p_tr[rnd], wtr[rnd], 2 / 3)
    log(f"dev tertile cuts (train random rows, weighted): t1 {t1:.6f}  t2 {t2:.6f}")

    def score(rows):
        X, _, w, pool = EF.design(rows)
        return model.predict_proba(X)[:, 1], label(rows), w, pool

    p_te, y_te, w_te, pool_te = score(te)
    p_h, y_h, w_h, pool_h = score(hold)
    src_h = np.array([r["src"] for r in hold])
    aucs = {"train (in-sample, all)": M.auc(ytr, p_tr), "train (in-sample, random)": M.auc(ytr[rnd], p_tr[rnd]),
            "test (all)": M.auc(y_te, p_te), "test (random)": M.auc(y_te[~pool_te], p_te[~pool_te]),
            "holdout (all)": M.auc(y_h, p_h), "holdout (random)": M.auc(y_h[~pool_h], p_h[~pool_h])}
    for s in ("HA", "HB", "HC"):
        m = src_h == s
        aucs[f"holdout {s}"] = M.auc(y_h[m], p_h[m])
    aucs["test (random, weighted)"] = M.auc(y_te[~pool_te], p_te[~pool_te], w_te[~pool_te])
    print("\nAUC of P(s0_third >= 2):")
    for k, v in aucs.items():
        print(f"  {k:<28}{v:.3f}")
    # reference: AUC of the entry filter's own feature 'me' and of bsol alone on the OOS rows (does the GBT beat one feature?)
    oos = te + hold
    p_o, y_o = np.concatenate([p_te, p_h]), np.concatenate([y_te, y_h])
    pool_o = np.concatenate([pool_te, pool_h])
    X_o = EF.design(oos)[0]
    ref = {f: M.auc(y_o, X_o[:, EF.FEATURES.index(f)]) for f in ("me", "bsol", "nB", "create_rate60", "op_s0t")}
    print("  single-feature AUC on test+holdout (reference): " + ", ".join(f"{k} {v:.3f}" for k, v in ref.items()))

    ret_o = np.array([r["ret_s025"] for r in oos])
    cl_o = np.array([r.get("creator") or r.get("signer") or r["mint"] for r in oos])
    print(f"\nLIFT (dev model, dev cuts t1={t1:.6f} t2={t2:.6f}); ret = ret_s025 (LEAD at 0.25 SOL, net), dry-run LEAD "
          "enters right after the bundle (nobody ahead by construction); CIs: creator-cluster bootstrap, B=2000")
    lifts = {}
    sets = {"test random": (np.arange(len(oos)) < len(te)) & ~pool_o,
            "holdout random": (np.arange(len(oos)) >= len(te)) & ~pool_o,
            "test+holdout random": ~pool_o,
            "test+holdout all (incl. operator pools / HA armed)": np.ones(len(oos), bool)}
    for nm, m in sets.items():
        lifts[nm] = lift_table(nm, p_o[m], y_o[m], ret_o[m], cl_o[m], t1, t2)
    # label -> return link on the same rows (sanity: is the label the right target?)
    for k in (0, 1, 2):
        mm = ~pool_o & ((np.array([r["s0_third"] or 0 for r in oos]) >= 2) if k == 2 else
                        (np.array([r["s0_third"] or 0 for r in oos]) == k))
        print(f"  [sanity] OOS random rows with s0_third {'>=2' if k == 2 else '==' + str(k)}: n {mm.sum()}, "
              f"mean ret_s025 {ret_o[mm].mean()*100:+.2f}%")

    # ---------------------------------------------------------------- final model: every row
    rows = sorted(table + hold, key=lambda r: (r["bt"], r["mint"]))
    X, _, w, pool = EF.design(rows)
    y = label(rows)
    fmodel, pf = EF.fit(X, y, w)
    rr = ~pool
    f1, f2 = EF.wquant(pf[rr], w[rr], 1 / 3), EF.wquant(pf[rr], w[rr], 2 / 3)
    log(f"final model: {len(rows)} rows, in-sample AUC {M.auc(y, pf):.3f}; cuts t1 {f1:.6f} t2 {f2:.6f}")
    # where the final cuts put the dev model's OOS scores is not comparable (different model); report the in-sample
    # tertile shares of the final model on the latest 626 rows (holdout period) for drift reference
    blob = pickle.dumps(fmodel)
    PKL.write_bytes(blob)
    mdl = EF.export_gbt(fmodel)
    src_counts = {}
    for r in rows:
        k = "pool" if r["pool"] else r["src"]
        src_counts[k] = src_counts.get(k, 0) + 1
    doc = dict(
        name="demand_v1",
        frozen_at=time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime()),
        what=("LEAD demand classifier: P(s0_third >= 2), i.e. at least two distinct outside (non-creator, non-bundle) "
              "wallets buy in the create slot behind the bundle = live shadow.tp_behind >= 2 for a fill that is first. "
              "Same 60 live features and model family as entry_filter_v1; rule under evaluation: skip if score < t1"),
        code="analysis/scripts/strategy/demand_freeze.py, analysis/scripts/strategy/demand_eval.py",
        features=EF.FEATURES,
        feature_groups=ef_doc["feature_groups"],
        dropped=ef_doc["dropped"],
        defaults=ef_doc["defaults"],
        scaler=None,
        model=mdl,
        tertiles=dict(t1=f1, t2=f2, protocol=("weighted 1/3 and 2/3 quantiles of the final model's in-sample "
                                              "predictions on the random-sample rows (entry_filter_v1 protocol)")),
        hyper=dict(kind=EF.KIND, target=f"s0_third>={K}", history=EF.HIST, max_depth=3, learning_rate=0.05,
                   max_iter=150, min_samples_leaf=20, l2_regularization=1.0, random_state=0, sklearn=sklearn.__version__),
        training=dict(n=len(rows), n_random=int(rr.sum()), n_pool=int(pool.sum()), by_source=src_counts,
                      label_rate_random=float(np.sum(w[rr] * y[rr]) / w[rr].sum()), first_bt=int(rows[0]["bt"]),
                      last_bt=int(rows[-1]["bt"])),
        constants=ef_doc["constants"],
        pkl_sha256=hashlib.sha256(blob).hexdigest(),
        demand=dict(
            label=f"s0_third >= {K} (tapes.build_tape: distinct slot-0 buyers outside the creator set and the bundle)",
            cuts_final=dict(t1=f1, t2=f2),
            dev=dict(split_bt=split, embargo_s=EMBARGO, n_train=len(tr), n_test=len(te), n_holdout=len(hold),
                     cuts=dict(t1=t1, t2=t2), auc=aucs, single_feature_auc_oos=ref, lift=lifts),
        ),
    )
    DOC.write_text(json.dumps(doc, indent=1))
    log("model", len(mdl["trees"]), "trees ->", DOC)
    # parity rows: python score_doc vs scikit-learn (and for filter_bench: cargo run --example filter_bench -- doc check)
    rng = np.random.default_rng(20261023)
    pick = np.sort(rng.choice(len(rows), N_CHECK, replace=False))
    ps = fmodel.predict_proba(X[pick])[:, 1]
    chk = [dict(mint=rows[i]["mint"], src=rows[i]["src"], x=[float(v) for v in X[i]], score=float(s))
           for i, s in zip(pick, ps)]
    worst = max(abs(EF.score_doc(doc, c["x"]) - c["score"]) for c in chk)
    log(f"parity: max |score_doc - sklearn| over {N_CHECK} rows = {worst:.3e}")
    if worst > 1e-9:
        raise SystemExit("export parity failed")
    CHECK.write_text(json.dumps(dict(model=doc["name"], doc_sha256=hashlib.sha256(DOC.read_bytes()).hexdigest(),
                                     features=EF.FEATURES, rows=chk), indent=0))
    log("check rows ->", CHECK)


if __name__ == "__main__":
    main()
