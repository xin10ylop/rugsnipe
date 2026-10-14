"""H1: re-freeze G1 rule 3 for live use and export it (analysis/rugsnipe_analysis/strategy/entry_filter.py).

  .venv/bin/python -I analysis/scripts/strategy/entry_filter_freeze.py freeze [--force]   # fit + export
  .venv/bin/python -I analysis/scripts/strategy/entry_filter_freeze.py fixtures           # Rust extraction fixtures
  .venv/bin/python -I -m rugsnipe_analysis.selftest_entry_filter                          # exporter test

freeze writes data/reports/strategy/entry_filter_v1.json (the model the hot path loads: features, scaler, trees,
tertile cut points, constants), entry_filter_v1.pkl (the scikit-learn object, sha256 in the JSON) and
entry_filter_v1_check.json (200 rows: feature vector + scikit-learn score, for the Python and Rust parity tests).
fixtures writes entry_filter_v1_fixtures.json: real creation slots and create transactions with the features the
Python pipeline computes from them, so the Rust feature extraction is checked against the training definitions.
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
from rugsnipe_analysis import labels as L  # noqa: E402
from rugsnipe_analysis.strategy import entry_filter as EF  # noqa: E402
from rugsnipe_analysis.strategy import predictor as P  # noqa: E402
from rugsnipe_analysis.strategy import predictor_models as M  # noqa: E402
from rugsnipe_analysis.strategy.tapes import SPLIT_SLOT, build_tape  # noqa: E402

N_CHECK = 200


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def _tertile_table(p, ret, tp, t1, t2, sets):
    out = {}
    for name, m in sets.items():
        if m.sum() < 10:
            continue
        lo, mid, hi = m & (p < t1), m & (p >= t1) & (p < t2), m & (p >= t2)
        keep = m & (p >= t1)
        out[name] = dict(
            n=int(m.sum()), auc_tp=M.auc(tp[m], p[m]), ev_all=float(ret[m].mean()),
            skip_bottom=dict(keep=float(keep.sum() / m.sum()), ev_kept=float(ret[keep].mean()),
                             lift=float(ret[keep].mean() - ret[m].mean())),
            tertiles={k: dict(share=float(v.sum() / m.sum()), ev=float(ret[v].mean()) if v.any() else None,
                              tp=float(tp[v].mean()) if v.any() else None)
                      for k, v in (("low", lo), ("mid", mid), ("high", hi))})
    return out


def pre_freeze_check(table: list[dict], hold: list[dict]) -> dict:
    """The live feature set fitted on the table only and scored once on the holdout rows (HA / HB / HC random).
    The holdouts were already used by the angles critic, so this is a consistency check of the reduced feature set,
    not a clean out-of-sample test. Also: the same score with the funding block blanked (a live bot knows the
    signer's funding only when its funding watcher saw it)."""
    Xt, yt, wt, pt = EF.design(table)
    hs = [r for r in hold if not r["pool"]]
    Xh, yh, _, _ = EF.design(hs)
    model, ptr = EF.fit(Xt, yt, wt)
    t1, t2 = EF.wquant(ptr[~pt], wt[~pt], 1 / 3), EF.wquant(ptr[~pt], wt[~pt], 2 / 3)
    ret = np.array([r["ret_base"] for r in hs])
    src = np.array([r["src"] for r in hs])
    sets = {s: src == s for s in ("HA", "HB", "HC")} | {"pooled": np.ones(len(hs), bool)}
    p = model.predict_proba(Xh)[:, 1]
    Xb = Xh.copy()
    for j, (f, g) in enumerate(EF.LIVE_FEATURES):
        if g == "funding":
            Xb[:, j] = 0.0
    pb = model.predict_proba(Xb)[:, 1]
    return dict(note=pre_freeze_check.__doc__.split("\n\n")[0].replace("\n    ", " "), t1=t1, t2=t2,
                scored=_tertile_table(p, ret, yh, t1, t2, sets),
                funding_blanked=_tertile_table(pb, ret, yh, t1, t2, sets))


def freeze():
    if EF.DOC.exists() and "--force" not in sys.argv:
        raise SystemExit(f"{EF.DOC} exists; refusing to overwrite a frozen model (--force)")
    import sklearn
    table, hold = EF.table_rows(), EF.holdout_rows()
    log("rows: table", len(table), "holdout", len(hold))
    chk = pre_freeze_check(table, hold)
    for k in ("scored", "funding_blanked"):
        b = chk[k]["pooled"]
        log(f"pre-freeze check ({k}): AUC {b['auc_tp']:.3f} keep {b['skip_bottom']['keep']*100:.0f}% "
            f"EV kept {b['skip_bottom']['ev_kept']*100:+.2f} vs all {b['ev_all']*100:+.2f} | "
            + " ".join(f"{s}: lift {chk[k][s]['skip_bottom']['lift']*100:+.2f}" for s in ("HA", "HB", "HC")))
    rows = sorted(table + hold, key=lambda r: (r["bt"], r["mint"]))
    X, y, w, pool = EF.design(rows)
    model, ptr = EF.fit(X, y, w)
    rnd = ~pool
    t1, t2 = EF.wquant(ptr[rnd], w[rnd], 1 / 3), EF.wquant(ptr[rnd], w[rnd], 2 / 3)
    blob = pickle.dumps(model)
    EF.PKL.write_bytes(blob)
    mdl = EF.export_gbt(model)
    src_counts = {}
    for r in rows:
        k = ("pool" if r["pool"] else r["src"])
        src_counts[k] = src_counts.get(k, 0) + 1
    doc = dict(
        name="entry_filter_v1",
        frozen_at=time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime()),
        what=("G1 rule 3 (gradient-boosted trees, depth 3, P(take-profit) of the LEAD), re-frozen on every row known "
              "now without the metadata JSON and cu_used; rule: skip if score < t1 (bottom training tertile)"),
        code="analysis/rugsnipe_analysis/strategy/entry_filter.py, analysis/scripts/strategy/entry_filter_freeze.py",
        features=EF.FEATURES,
        feature_groups={f: g for f, g in EF.LIVE_FEATURES},
        dropped=EF.DROPPED,
        defaults=EF.DEFAULTS,
        scaler=None,
        model=mdl,
        tertiles=dict(t1=t1, t2=t2, protocol=("weighted 1/3 and 2/3 quantiles of the in-sample predictions on the "
                                              "random-sample rows (operator-pool and HA-armed rows excluded), as in "
                                              "G1's freeze (predictor_wf.freeze)")),
        hyper=dict(kind=EF.KIND, target=EF.TARGET, history=EF.HIST, max_depth=3, learning_rate=0.05, max_iter=150,
                   min_samples_leaf=20, l2_regularization=1.0, random_state=0, sklearn=sklearn.__version__),
        training=dict(n=len(rows), n_random=int(rnd.sum()), n_pool=int(pool.sum()), by_source=src_counts,
                      tp_rate_random=float(np.sum(w[rnd] * y[rnd]) / w[rnd].sum()), first_bt=int(rows[0]["bt"]),
                      last_bt=int(rows[-1]["bt"])),
        constants=dict(
            uri_hosts=list(M.URI_HOSTS), uri_host_feature=["host_j7tracker", "host_ipfsio", "host_uxento",
                                                           "host_mypinata", "host_rapidlaunch"],
            mev_tip_accounts=sorted(lb.address for lb in L.all_labels() if lb.category == "mev"),
            cex_funders=sorted(lb.address for lb in L.all_labels() if lb.category == "cex"),
            swap_funders=sorted(lb.address for lb in L.all_labels() if lb.category in ("swap_service", "bridge")),
            compute_budget_program=P.COMPUTE_BUDGET, base_fee_per_signature=5000,
            system_transfer_types=["transfer", "transferWithSeed", "createAccount", "createAccountWithSeed"],
            split_slot=SPLIT_SLOT, supply_tokens=1e9, op_outcome_delay_s=10,
            fund_classes=list(M.FUND_CLS),
        ),
        pkl_sha256=hashlib.sha256(blob).hexdigest(),
        pre_freeze_check=chk,
        in_sample=_tertile_table(ptr, np.array([r["ret_base"] for r in rows]), y, t1, t2,
                                 {"random rows": rnd}),
    )
    EF.DOC.write_text(json.dumps(doc, indent=1))
    log("model", len(mdl["trees"]), "trees, t1", round(t1, 4), "t2", round(t2, 4), "->", EF.DOC)
    # 200 parity rows: seeded draw over the random-sample rows
    rng = np.random.default_rng(20261014)
    pick = np.sort(rng.choice(np.nonzero(rnd)[0], N_CHECK, replace=False))
    p = model.predict_proba(X[pick])[:, 1]
    chk_rows = [dict(mint=rows[i]["mint"], src=rows[i]["src"], x=[float(v) for v in X[i]], score=float(s),
                     tertile=EF.tertile(doc, float(s))) for i, s in zip(pick, p)]
    EF.CHECK.write_text(json.dumps(dict(model=doc["name"], doc_sha256=hashlib.sha256(EF.DOC.read_bytes()).hexdigest(),
                                        features=EF.FEATURES, rows=chk_rows), indent=0))
    log("check rows", len(chk_rows), "->", EF.CHECK)


def fixtures():
    """Real creation slots (HB tapes) and create transactions (HB rpc cache) with the features the Python pipeline
    computes from them."""
    import angles_eval as AE
    rc = AE._MultiCache()
    rows = [r for r in EF.holdout_rows() if r["src"] == "HB"]
    by_mint = {r["mint"]: r for r in rows}
    bundle, ctx = [], []
    names = list(P.bundle_features.__code__.co_varnames)  # noqa: F841  (documentation only)
    for line in open(AE.HB / "tapes.jsonl"):
        r = json.loads(line)
        if r["mint"] not in by_mint or not r["complete"] or not r["trades"]:
            continue
        C = {r["creator"], r["user"], r["fee_payer"]}
        t = build_tape(r["mint"], "HB", 1.0, r["slot"], r["bt"], r["sig"], C, r["trades"], r["hor"], True)
        if t is None or t.kind != "multi":
            continue
        bf = P.bundle_features(t)
        s0 = [x for x in r["trades"] if x[0] == r["slot"]]
        bundle.append(dict(
            mint=r["mint"], creator_set=sorted(C),
            trades=[[x[1], x[9] == r["sig"], x[4], bool(x[5]), int(round(x[6] * 1e9)), int(round(x[7] * 1e6))]
                    for x in s0],
            me=float(t.me), expected={k: float(bf[k]) for k in EF.FEATURES if k in bf}))
        if len(bundle) >= 60:
            break
    for r in rows:
        hit = rc.get(f"tx:{r['sig']}")
        tx = hit.get("v") if hit else None
        if not tx:
            continue
        f = M.feature_row(r, EF.HIST)
        exp = {k: float(f[k]) for k, g in EF.LIVE_FEATURES if g in ("create_tx", "uri")}
        ctx.append(dict(sig=r["sig"], signer=r["signer"], creator=r["creator_arg"], tx_index=r.get("tx_index"),
                        tx=tx, expected=exp))
        if len(ctx) >= 24:
            break
    EF.FIXTURES.write_text(json.dumps(dict(model="entry_filter_v1", bundle=bundle, create_tx=ctx)))
    log("fixtures: bundle", len(bundle), "create tx", len(ctx), "->", EF.FIXTURES)


if __name__ == "__main__":
    {"freeze": freeze, "fixtures": fixtures}[sys.argv[1]]()
