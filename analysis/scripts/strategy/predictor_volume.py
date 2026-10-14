"""G1: volume and SOL/day of the frozen P(TP) score (rule 3) and of post-hoc readings of the same frozen
score (keep top tertile; sizes capped at 0.5 SOL), on the Oct 12-14 sets. Population-scaled to 3,750
multi-wallet bundled launches/day; failed attempts burn 0.003 SOL each.

  .venv/bin/python -I analysis/scripts/strategy/predictor_volume.py
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
from rugsnipe_analysis.strategy import predictor_models as M  # noqa: E402
import predictor_wf as W  # noqa: E402

LANDS = (0.1, 0.2, 0.5)


def main():
    doc = json.load(open(W.FROZEN))
    models = pickle.load(open(W.FROZEN_PKL, "rb"))
    ent = next(e for e in doc["rules"] if e["name"].endswith("tp:size_skip"))
    rows, d = W.load(ent["variant"])
    _, _, p = W.apply_rule(ent, d, models)
    hi, mid = p >= ent["t2"], (p >= ent["t1"]) & (p < ent["t2"])
    pols = {
        "flat 0.5 SOL, every launch": np.full(len(p), 0.5),
        "rule 3 frozen: skip low, 0.5 mid, 1.0 high": np.where(hi, 1.0, np.where(mid, 0.5, 0.0)),
        "post-hoc: skip low, 0.25 mid, 0.5 high": np.where(hi, 0.5, np.where(mid, 0.25, 0.0)),
        "post-hoc: 0.5 SOL on the high tertile only": np.where(hi, 0.5, 0.0),
        "post-hoc: 0.5 SOL on mid + high": np.where(hi | mid, 0.5, 0.0),
    }
    out = {}
    for sn, m in (("window", (d.src == "window") & ~d.pool), ("all_post", (d.bt >= M.OCT12) & ~d.pool)):
        w = d.w[m]
        res = {}
        for name, sz in pols.items():
            s = sz[m]
            r = np.select([s >= 1.0, s >= 0.5, s >= 0.25], [d.ret["s100"][m], d.ret["base"][m], d.ret["s025"][m]], 0.0)
            att = float(np.sum(w * (s > 0)) / w.sum())
            pnl = float(np.sum(w * s * r) / w.sum())                 # SOL per qualifying launch, if landed
            stake = float(np.sum(w * s) / w.sum())
            ev_trade = float(np.sum(w * (s > 0) * r) / np.sum(w * (s > 0)))
            day = {f"land{int(L*100)}": M.POP_PER_DAY * (L * pnl - (1 - L) * M.FAIL_COST * att) for L in LANDS}
            res[name] = dict(attempt_share=att, trades_per_day_at_100pct=M.POP_PER_DAY * att, sol_per_launch=pnl,
                             ev_per_trade=ev_trade, ev_per_sol=pnl / stake, **day)
            print(f"{sn:8s} {name:<44s} attempt={att*100:4.0f}% trades/day(100% land)={M.POP_PER_DAY*att:5.0f} EV/trade={ev_trade*100:+5.2f}% "
                  f"EV/SOL={pnl/stake*100:+5.2f}% SOL/day@10/20/50%={day['land10']:+6.1f} {day['land20']:+6.1f} {day['land50']:+6.1f}")
        out[sn] = res
    json.dump(out, open(REPO / "data/reports/strategy/predictor_volume.json", "w"), indent=1)


if __name__ == "__main__":
    main()
