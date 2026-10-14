"""G4 skeptic: pre-registration for the second fresh holdout (written before any holdout feature or outcome).

Refits G1's two best walk-forward selection configurations on the WHOLE predictor table (every row known
before the holdouts) and freezes them with their training-quantile thresholds, next to G1's own frozen
rule 3 (unchanged), G2's frozen refinements (unchanged) and the evaluation protocol.

  .venv/bin/python -I analysis/scripts/strategy/angles_prereg.py
Writes data/reports/strategy/angles_prereg.json and angles_prereg_models.pkl (sha256 in the json).
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
from rugsnipe_analysis.strategy import predictor_models as M  # noqa: E402
import predictor_wf as W  # noqa: E402

OUT = REPO / "data" / "reports" / "strategy"
PREREG = OUT / "angles_prereg.json"
PKL = OUT / "angles_prereg_models.pkl"
RETRAINED = ["mdl:rand:all:gbt_3:tp:keep0.2", "mdl:pool:all:lr_0.5:ret:keep0.4"]


def fit_all(name):
    _, variant, fs, kind, target, pol = name.split(":")
    rows, d = W.load(variant)
    tr = np.ones(len(rows), bool) if variant == "pool" else ~d.pool
    rnd = tr & ~d.pool
    cj = W.cols(d, fs)
    y, mt = W.target_y(d, target)
    trm = tr & ~np.isnan(y)
    _, ptr, model = M.fit_predict(kind, mt, d.X[trm][:, cj], y[trm], d.w[trm], d.X[trm][:1, cj])
    k = float(pol[4:])
    thr = W.wquant(ptr[rnd[trm]], d.w[rnd], 1 - k)
    ent = dict(name=name, type="model", variant=variant, fs=fs, kind=kind, target=target, policy=pol,
               features=[d.names[i] for i in cj], n_train=int(trm.sum()), n_train_random=int(rnd.sum()),
               train_last_bt=int(d.bt[trm].max()), threshold=thr)
    return ent, model


def main():
    if PREREG.exists():
        raise SystemExit(f"{PREREG} exists; refusing to overwrite a pre-registration")
    ents, models = [], {}
    for name in RETRAINED:
        e, m = fit_all(name)
        ents.append(e)
        models[name] = m
        print(name, {k: v for k, v in e.items() if k != "features"})
    blob = pickle.dumps(models)
    PKL.write_bytes(blob)
    g1 = json.load(open(W.FROZEN))
    g2 = json.load(open(OUT / "frozen_refinements.json"))
    doc = dict(
        written_at=time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime()),
        status=("Written before any feature, score or outcome of either holdout set was computed. HA's aggregate LEAD EV "
                "(+3.2%, n=222) is public in docs/04; nothing per launch or per rule had been looked at. HB's tapes had "
                "not been pulled (only the create index, create decodes and the chain-only multi-wallet prescreen)."),
        sets=dict(
            HA=("critic holdout (data/cache/strategy_holdout, Oct 14 08:43-13:43 UTC): random-group multi-wallet "
                "launches with a complete tape; never used by G1/G2/G3"),
            HB=("new dense block (data/cache/strategy_holdout2): every eligible create (non-mayhem, SOL-quoted, single "
                "create, < 84.9 SOL in the create tx, mint in no earlier cache) from anchor-45 min backwards, newest first, "
                "multi-wallet by the chain-only prescreen (getSignaturesForAddress + slot-0 txs, tapes._raw_bundle's rule), "
                "tapes from swap-api (25 s x1 page, then 4 s x1, then 4 s x4), until 500 swap-api requests (retries "
                "included) are spent; final multi-wallet status from build_tape"),
            pooled="HA + HB, weight 1 per launch"),
        primary=[
            dict(id="G1-R3-high", what="G1 frozen rule 3 P(TP) (predictor_frozen_models.pkl, unchanged), keep score >= t2 (top tertile)",
                 test="EV(kept) - EV(all) on the same launches"),
            dict(id="G1-R3-skip", what="G1 frozen rule 3, keep score >= t1 (skip the bottom tertile; G1's probe recommendation)",
                 test="EV(kept) - EV(all)"),
            dict(id="G1-RT-gbt3-tp-top20", what="mdl:rand:all:gbt_3:tp:keep0.2 refitted on the whole table (frozen here)",
                 test="EV(kept) - EV(all)"),
            dict(id="G1-RT-ridge-ret-top40", what="mdl:pool:all:lr_0.5:ret:keep0.4 refitted on the whole table (frozen here)",
                 test="EV(kept) - EV(all)"),
            dict(id="G2-gate", what=g2["frozen"][1]["name"], test="paired EV(variant) - EV(LEAD)"),
            dict(id="G2-reentry", what="frozen re-entry of frozen_refinements.json", test="paired EV(variant) - EV(LEAD)"),
            dict(id="G2-ladder", what="frozen ladder of frozen_refinements.json", test="SOL per SOL deployed minus a flat stake of the same mean size"),
            dict(id="G2-k0-bail", what="G2's pre-registration note: no third-party buy behind us by the end of slot 0 -> sell at the end of slot+1 (fire slot 0, 1-slot latency); else LEAD",
                 test="paired EV(variant) - EV(LEAD)"),
        ],
        secondary=["LEAD itself (EV > 0)", "G2 scale-out and flow (frozen)", "sell at the end of slot+1 always",
                   "G1 frozen rules 1 (tx index <= 295) and 2 (n10 GBT top 40%)", "G1 univariate uri host = meta.uxento.io",
                   "HB only: the two retrained configs refitted again on table + HA (walk-forward step, same code)",
                   "stress of every rule: +3.6% premium, 0.01 SOL, 2-slot exit latency, rank 2, gap-2 median-bid-priced cost"],
        g3="G3 has no positive screen (graduation, second wave, hour/create rate all lost); nothing to carry.",
        statistics=("Pooled HA+HB. One-sided p from a bootstrap over signers (clusters), 4000 draws; BH over the 8 primary "
                    "tests. A rule 'raises the edge beyond the LEAD on fresh data' only if its pooled lift > 0 with BH q < 0.10 "
                    "and the lift is >= 0 on both HA and HB. Per set and per stress reported regardless."),
        volume=("trades/day = qualifying launches/day x kept share x landing rate; qualifying launches/day both 3,750 "
                "(docs/04) and 6,181 (G3 pnl_model); SOL/day from pnl_model at L1 = 20%, c_fail 0.003, f_pay 0.8, reach 0.93"),
        g1_frozen=dict(file="predictor_frozen.json", frozen_at=g1["frozen_at"], models_sha256=g1["models_sha256"]),
        g2_frozen=dict(file="frozen_refinements.json", frozen_at=g2["frozen_at"]),
        retrained=ents, models_sha256=hashlib.sha256(blob).hexdigest(),
    )
    json.dump(doc, open(PREREG, "w"), indent=1)
    print("written", PREREG, doc["written_at"], doc["models_sha256"])


if __name__ == "__main__":
    main()
