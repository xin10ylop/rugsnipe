"""Pre-send gate models: the demand classifier and the P(TP) entry filter refit with the funding block masked.

  .venv/bin/python -I analysis/scripts/strategy/presend_freeze.py [--force]

The live gate scores BEFORE the send, when the signer's funding is unknown: it fills the funding block (features
38..47, ``filter.rs`` FUNDING / N_FUNDING) with ``funding_features(&Funding::default())``. ``Funding::default()`` is
all ``None``, so that is ten zeros (no class one-hot, ``l1p(None) = 0`` for n_prior / amount / lag, ``fund_round``
false). Every training row gets exactly those ten values, so no tree splits on them.

Writes data/reports/strategy/demand_v1_presend.json (target s0_third >= 2) and entry_filter_v2_presend.json (target
tp_hit), both in the entry_filter_v1 schema, fit on every row with cut points from the entry-filter protocol
(weighted 1/3 and 2/3 quantiles of the in-sample predictions on the random-sample rows). Before that, a dev fit of each
on the earlier 70 % of the table (15-minute embargo) is scored on the later 30 % and the HA / HB / HC holdout:
AUC, and on the test + holdout random rows the lift table take all / skip below t1 / take only >= t2 (dev cuts),
with creator-cluster bootstrap CIs (demand_freeze.cluster_ci / lift_ci).
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
import demand_freeze as DF  # noqa: E402
from rugsnipe_analysis.strategy import entry_filter as EF  # noqa: E402
from rugsnipe_analysis.strategy import predictor_models as M  # noqa: E402

FUNDING, N_FUNDING = 38, 10                       # filter.rs
PRESEND_FUNDING = [0.0] * N_FUNDING               # funding_features(&Funding::default())
SP_CHECK = None                                   # parity rows go next to the model only on request (--check DIR)

TARGETS = {
    "demand_v1_presend": dict(target=f"s0_third>={DF.K}", y=DF.label,
                              what=("LEAD demand classifier, pre-send: P(s0_third >= 2) (>= 2 outside buyers behind "
                                    "the bundle in the create slot = live shadow.tp_behind >= 2 for a first fill); "
                                    "funding block masked to funding_features(&Funding::default())")),
    "entry_filter_v2_presend": dict(target=EF.TARGET, y=lambda rows: np.array([float(r["tp_hit"]) for r in rows]),
                                    what=("G1 rule 3 P(take-profit) of the LEAD (entry_filter_v1 pipeline), pre-send: "
                                          "funding block masked to funding_features(&Funding::default())")),
}


def masked_design(rows):
    X, _, w, pool = EF.design(rows)
    X[:, FUNDING:FUNDING + N_FUNDING] = PRESEND_FUNDING
    return X, w, pool


def cut_table(name, p, ret, cl, t1, t2) -> dict:
    sol = ret * DF.SIZE
    out = {}
    print(f"  [{name}] {'rule':<18}{'kept':>6}{'share':>7}{'mean ret':>10}{'95% CI (mean)':>20}{'total SOL':>11}"
          f"{'95% CI (total)':>20}{'d mean vs all':>15}{'95% CI':>18}")
    for rule, keep in (("take all", np.ones(len(p), bool)), ("skip below t1", p >= t1), ("take only >= t2", p >= t2)):
        c, ct = DF.cluster_ci(ret[keep], cl[keep], 3), DF.cluster_ci(sol[keep], cl[keep], 3)
        lr = DF.lift_ci(ret, keep, cl)
        lt = DF.lift_ci(sol, keep, cl)
        out[rule] = dict(n=int(keep.sum()), share=float(keep.mean()), mean_ret=c["mean"], mean_ci=c["mean_ci"],
                         total_sol=ct["total"], total_ci=ct["total_ci"], lift_mean=lr["mean"], lift_mean_ci=lr["mean_ci"],
                         lift_total_sol=lt["total"], lift_total_ci=lt["total_ci"])
        print(f"  {'':<{len(name) + 3}}{rule:<18}{int(keep.sum()):>6}{keep.mean() * 100:>6.0f}%{c['mean'] * 100:>+9.2f}%"
              f"   [{c['mean_ci'][0] * 100:+6.2f}, {c['mean_ci'][1] * 100:+6.2f}]{ct['total']:>+11.3f}   "
              f"[{ct['total_ci'][0]:+7.3f}, {ct['total_ci'][1]:+7.3f}]{lr['mean'] * 100:>+14.2f}%  "
              f"[{lr['mean_ci'][0] * 100:+5.2f}, {lr['mean_ci'][1] * 100:+5.2f}]")
    return out


def main():
    outs = {n: EF.OUT / f"{n}.json" for n in TARGETS}
    if any(p.exists() for p in outs.values()) and "--force" not in sys.argv:
        raise SystemExit(f"{[str(p) for p in outs.values() if p.exists()]} exist; refusing to overwrite (--force)")
    import sklearn
    ef_doc = EF.load_doc()
    assert ef_doc["features"] == EF.FEATURES and len(EF.FEATURES) == 60
    assert all(ef_doc["feature_groups"][f] == "funding" for f in EF.FEATURES[FUNDING:FUNDING + N_FUNDING])
    assert sum(g == "funding" for g in ef_doc["feature_groups"].values()) == N_FUNDING
    table, hold = EF.table_rows(), EF.holdout_rows()
    table.sort(key=lambda r: (r["bt"], r["mint"]))
    split = int(np.quantile([r["bt"] for r in table], DF.TRAIN_FRAC))
    tr = [r for r in table if r["bt"] < split - DF.EMBARGO]
    te = [r for r in table if r["bt"] >= split]
    oos = te + hold
    Xtr, wtr, pooltr = masked_design(tr)
    Xo, _, poolo = masked_design(oos)
    rnd_o = ~poolo
    ret_o = np.array([r["ret_s025"] for r in oos])
    cl_o = np.array([r.get("creator") or r.get("signer") or r["mint"] for r in oos])
    is_te = np.arange(len(oos)) < len(te)
    log = DF.log
    log(f"split {split}: train {len(tr)}, test {len(te)}, holdout {len(hold)}; funding cols "
        f"{EF.FEATURES[FUNDING]}..{EF.FEATURES[FUNDING + N_FUNDING - 1]} := {PRESEND_FUNDING}")
    rows_all = sorted(table + hold, key=lambda r: (r["bt"], r["mint"]))
    Xa, wa, poola = masked_design(rows_all)
    summary = {}
    for name, spec in TARGETS.items():
        ytr, yo = spec["y"](tr), spec["y"](oos)
        model, ptr = EF.fit(Xtr, ytr, wtr)
        r_tr = ~pooltr
        t1, t2 = EF.wquant(ptr[r_tr], wtr[r_tr], 1 / 3), EF.wquant(ptr[r_tr], wtr[r_tr], 2 / 3)
        po = model.predict_proba(Xo)[:, 1]
        # reference: the same dev fit without the mask
        Xtr_u, Xo_u = EF.design(tr)[0], EF.design(oos)[0]
        mu, _ = EF.fit(Xtr_u, ytr, wtr)
        pu = mu.predict_proba(Xo_u)[:, 1]
        dem_o, tp_o = DF.label(oos), np.array([float(r["tp_hit"]) for r in oos])
        aucs = {"test random": M.auc(yo[is_te & rnd_o], po[is_te & rnd_o]),
                "holdout random": M.auc(yo[~is_te & rnd_o], po[~is_te & rnd_o]),
                "test+holdout random": M.auc(yo[rnd_o], po[rnd_o]),
                "test+holdout random, unmasked": M.auc(yo[rnd_o], pu[rnd_o]),
                "same score vs s0_third>=2": M.auc(dem_o[rnd_o], po[rnd_o]),
                "same score vs tp_hit": M.auc(tp_o[rnd_o], po[rnd_o])}
        print(f"\n== {name} (target {spec['target']}); dev cuts t1 {t1:.6f} t2 {t2:.6f}")
        print("  AUC: " + ", ".join(f"{k} {v:.3f}" for k, v in aucs.items()))
        lift = cut_table("test+holdout random", po[rnd_o], ret_o[rnd_o], cl_o[rnd_o], t1, t2)
        lift_te = cut_table("test random", po[is_te & rnd_o], ret_o[is_te & rnd_o], cl_o[is_te & rnd_o], t1, t2)
        lift_h = cut_table("holdout random", po[~is_te & rnd_o], ret_o[~is_te & rnd_o], cl_o[~is_te & rnd_o], t1, t2)
        # final: every row, masked
        ya = spec["y"](rows_all)
        fm, pf = EF.fit(Xa, ya, wa)
        rr = ~poola
        f1, f2 = EF.wquant(pf[rr], wa[rr], 1 / 3), EF.wquant(pf[rr], wa[rr], 2 / 3)
        mdl = EF.export_gbt(fm)
        nsplit_f = sum(1 for t in mdl["trees"] for n in t if not n["leaf"] and FUNDING <= n["feature"] < FUNDING + N_FUNDING)
        assert nsplit_f == 0, f"{name}: {nsplit_f} splits on the masked funding block"
        blob = pickle.dumps(fm)
        src_counts = {}
        for r in rows_all:
            k = "pool" if r["pool"] else r["src"]
            src_counts[k] = src_counts.get(k, 0) + 1
        doc = dict(
            name=name, frozen_at=time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime()), what=spec["what"],
            code="analysis/scripts/strategy/presend_freeze.py",
            features=EF.FEATURES, feature_groups=ef_doc["feature_groups"], dropped=ef_doc["dropped"],
            defaults=ef_doc["defaults"], scaler=None, model=mdl,
            tertiles=dict(t1=f1, t2=f2, protocol=("weighted 1/3 and 2/3 quantiles of the in-sample predictions on the "
                                                  "random-sample rows (entry_filter_v1 protocol)")),
            hyper=dict(kind=EF.KIND, target=spec["target"], history=EF.HIST, max_depth=3, learning_rate=0.05,
                       max_iter=150, min_samples_leaf=20, l2_regularization=1.0, random_state=0,
                       sklearn=sklearn.__version__),
            training=dict(n=len(rows_all), n_random=int(rr.sum()), n_pool=int(poola.sum()), by_source=src_counts,
                          label_rate_random=float(np.sum(wa[rr] * ya[rr]) / wa[rr].sum()),
                          first_bt=int(rows_all[0]["bt"]), last_bt=int(rows_all[-1]["bt"])),
            constants=ef_doc["constants"], pkl_sha256=hashlib.sha256(blob).hexdigest(),
            presend=dict(masked_features=EF.FEATURES[FUNDING:FUNDING + N_FUNDING], masked_values=PRESEND_FUNDING,
                         source="crates/rugsnipe/src/filter.rs funding_features(&Funding::default())",
                         dev=dict(split_bt=split, embargo_s=DF.EMBARGO, n_train=len(tr), n_test=len(te),
                                  n_holdout=len(hold), cuts=dict(t1=t1, t2=t2), auc=aucs,
                                  lift={"test+holdout random": lift, "test random": lift_te,
                                        "holdout random": lift_h})),
        )
        outs[name].write_text(json.dumps(doc, indent=1))
        # parity: pure-Python scorer of the export vs scikit-learn
        pick = np.random.default_rng(20261023).choice(len(rows_all), DF.N_CHECK, replace=False)
        ps = fm.predict_proba(Xa[pick])[:, 1]
        worst = max(abs(EF.score_doc(doc, [float(v) for v in Xa[i]]) - s) for i, s in zip(pick, ps))
        if worst > 1e-9:
            raise SystemExit(f"{name}: export parity failed ({worst})")
        if "--check" in sys.argv:
            d = Path(sys.argv[sys.argv.index("--check") + 1])
            (d / f"{name}_check.json").write_text(json.dumps(dict(model=name, rows=[
                dict(x=[float(v) for v in Xa[i]], score=float(s)) for i, s in zip(pick, ps)])))
        log(f"{name}: {len(mdl['trees'])} trees, 0 funding splits, final cuts t1 {f1:.6f} t2 {f2:.6f}, "
            f"parity {worst:.1e} -> {outs[name]}")
        summary[name] = dict(t1=f1, t2=f2)


if __name__ == "__main__":
    main()
