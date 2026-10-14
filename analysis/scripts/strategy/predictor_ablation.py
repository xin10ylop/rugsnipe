"""G1 ablation in the full walk-forward: which feature groups carry the P(TP) selection? Same folds as
predictor_wf.py full (train before each fold minus 15 min, test on random-sample rows), GBT depth 3 and
logistic C=0.05, target = TP hit; keep top 20 % / 40 % by train-quantile threshold. Per-set breakdown.

  .venv/bin/python -I analysis/scripts/strategy/predictor_ablation.py
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
sys.path.insert(0, str(REPO / "analysis/scripts/strategy"))
from rugsnipe_analysis.strategy import predictor_models as M  # noqa: E402
from rugsnipe_analysis.strategy import stats  # noqa: E402
import predictor_wf as W  # noqa: E402

GROUPS = {
    "bundle": lambda n: n in ("nB", "bsol", "dev_sol", "tot_sol", "me", "dev_share", "b_n_tx", "b_max", "b_min", "b_mean",
                              "b_cv", "b_round", "b_tok_eq", "b_desc", "n_trades_to_pb", "dev_round", "tok_pct_insiders"),
    "createtx": lambda n: n in ("tx_index", "tx_index_log", "jito_tip", "tip_sol", "prio_sol", "prio_log", "cu_used",
                                "cu_limit_set", "cu_limit", "cu_price_log", "n_top_ix", "tx_v0", "n_signers", "creator_ne_signer", "payer_ne_signer"),
    "text_uri": lambda n: n.startswith("host_") or n in ("name_len", "sym_len", "name_words", "name_caps", "sym_caps", "emoji",
                                                         "non_ascii", "cjk", "sym_ticker", "sym_dollar", "name_eq_sym", "name_digits", "uri_ipfs"),
    "meta": lambda n: n.startswith("m_") or n == "meta_ok",
    "funding": lambda n: n.startswith("fund_"),
    "operator": lambda n: n.startswith("op_"),
    "time_market": lambda n: n in ("hour_sin", "hour_cos", "weekend", "regime_post", "create_rate60", "create_rate600"),
}


def run(d, rows, cj, kind, train_ok, test_ok):
    n = len(d.bt)
    p = np.full(n, np.nan)
    thr = {k: np.full(n, np.inf) for k in (0.2, 0.4)}
    for a, b in zip(M.FULL_EDGES[:-1], M.FULL_EDGES[1:]):
        tr = (d.bt < a - M.EMBARGO) & train_ok
        te = (d.bt >= a) & (d.bt < b) & test_ok
        if (tr & ~d.pool).sum() < 60 or te.sum() == 0:
            continue
        pte, ptr, _ = M.fit_predict(kind, "tp", d.X[tr][:, cj], d.tp[tr], d.w[tr], d.X[te][:, cj])
        p[te] = pte
        rnd = ~d.pool[tr]
        for k in thr:
            thr[k][te] = W.wquant(ptr[rnd], d.w[tr][rnd], 1 - k)
    return p, thr


def main():
    out = {}
    for variant in ("rand", "pool"):
        rows, d = W.load(variant)
        train_ok = np.ones(len(rows), bool) if variant == "pool" else ~d.pool
        test_ok = ~d.pool
        names = d.names
        specs = {"all": [i for i in range(len(names))]}
        for g, f in GROUPS.items():
            specs[f"only_{g}"] = [i for i, nm in enumerate(names) if f(nm)]
            specs[f"all_minus_{g}"] = [i for i, nm in enumerate(names) if not f(nm)]
        for sname, cj in specs.items():
            for kind in ("gbt_3", "lr_0.05"):
                p, thr = run(d, rows, np.array(cj), kind, train_ok, test_ok)
                key = f"{variant}:{sname}:{kind}"
                res = {}
                for setn, m in (("post", (d.bt >= M.OCT12) & ~np.isnan(p)), ("window", (d.src == "window") & ~np.isnan(p)),
                                ("fresh_deep_g3", (d.bt >= M.OCT12) & (d.src != "window") & ~np.isnan(p)),
                                ("dev", (d.bt < M.OCT12) & ~np.isnan(p))):
                    w = d.w[m]
                    r = d.ret["base"][m]
                    e = dict(n=int(m.sum()), auc_tp=M.auc(d.tp[m], p[m], w), ev_all=stats.wmean(r, w))
                    for k in (0.2, 0.4):
                        s = p[m] >= thr[k][m]
                        st = M.sel_stats(r, w, s, 1.0)
                        e[f"keep{k}"] = dict(frac=st.get("frac"), ev=st["ev"], lift=st.get("lift"), p=st.get("p_lift"))
                    res[setn] = e
                out[key] = res
                po, wi, fr = res["post"], res["window"], res["fresh_deep_g3"]
                print(f"{key:<36s} AUC post={po['auc_tp']:.3f} win={wi['auc_tp']:.3f} oth={fr['auc_tp']:.3f} | keep20 EV post={po['keep0.2']['ev']*100:+.1f}% "
                      f"win={wi['keep0.2']['ev']*100:+.1f}% oth={fr['keep0.2']['ev']*100:+.1f}% | keep40 post={po['keep0.4']['ev']*100:+.1f}% "
                      f"(all {po['ev_all']*100:+.1f}%)", flush=True)
    json.dump(out, open(REPO / "data/reports/strategy/predictor_ablation.json", "w"), indent=1, default=float)


if __name__ == "__main__":
    main()
