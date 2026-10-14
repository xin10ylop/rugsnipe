"""Descriptive breakdown of a frozen E1 scalp policy on gap-3 (all launches): which exit component
fires, the hold time, return per exit reason, and where the gross edge comes from."""
import json
import sys
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
sys.path.insert(0, str(REPO / "analysis" / "scripts" / "strategy"))
from rugsnipe_analysis.strategy import sim  # noqa: E402
from rugsnipe_analysis.strategy.tapes import load_gap3  # noqa: E402
from refine import SPECS, filt  # noqa: E402

doc = json.load(open(REPO / "data/reports/strategy/frozen_policies.json"))
tapes = load_gap3()
w = np.array([t.w for t in tapes])
out = []
for pol in doc["policies"][:3]:
    sc = sim.Scenario(pol["size"], 0.011, 0.004, 1)
    F, R, trig, _ = sim.simulate_entry(tapes, "E1", sc, SPECS)
    row = np.array(pol["prow"])
    row = row[row < len(SPECS)]
    m = trig & filt(tapes)[pol["filter"]]
    Fp, Rp = F[m][:, row], R[m][:, row]
    am = Fp.argmin(1)
    ret = Rp[np.arange(len(am)), am]
    ww = w[m]
    hold_slots = Fp[np.arange(len(am)), am] - np.array([t.S0 for t, mm in zip(tapes, m) if mm])
    ms = np.array([t.ms_slot for t, mm in zip(tapes, m) if mm])
    hold_s = (hold_slots + 1) * ms / 1000
    rec = dict(policy=pol["exit"], size=pol["size"], filter=pol["filter"], n=int(m.sum()),
               ev=float(np.sum(ret * ww) / ww.sum()), hold_s_median=float(np.median(hold_s)),
               hold_s_p90=float(np.quantile(hold_s, 0.9)), by_reason={})
    for k, ci in enumerate(row):
        sel = am == k
        if sel.any():
            rec["by_reason"][sim.spec_name(SPECS[ci])] = dict(
                share=float(ww[sel].sum() / ww.sum()), ev=float(np.sum(ret[sel] * ww[sel]) / ww[sel].sum()),
                contrib=float(np.sum(ret[sel] * ww[sel]) / ww.sum()))
    # gross decomposition at the same exits: fees+fixed cost vs price move
    S = pol["size"]
    rec["cost_drag"] = dict(fees_round_trip=0.0247, fixed=0.004 / S, premium=0.011 / 1.011)
    out.append(rec)
    print(json.dumps(rec, indent=1))
json.dump(out, open(REPO / "data/reports/strategy/mechanism.json", "w"), indent=1)
