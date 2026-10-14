"""Family-level out-of-sample summary of the WHOLE G2 grid (diagnostic, after the freeze; selects nothing).

For every configuration: train paired diff vs the LEAD, pooled-OOS diff (gap-3 test + deep + fresh, gap-3
weights rescaled to mean 1) and window diff (quiet / busy split). Per family and sub-family: how many configs
beat the LEAD out of sample, the rank correlation train -> OOS, and the best OOS config (a winner's-curse
illustration, not a recommendation). Ladders compare SOL P&L per SOL deployed.
"""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis/scripts/strategy"))
from refinements import build_configs  # noqa: E402

C = build_configs()
tr = np.load(REPO / "data/cache/strategy/refinements_train.npz")
g = np.load(REPO / "data/cache/strategy/refinements_grid.npz")


def diffs(P, S, w):
    ev = (w @ P) / (w @ S)
    return ev - ev[0], ev


wtr = None
from refinements_data import load  # noqa: E402
sets = load()
W = {k: np.array([t.w for t in v]) for k, v in sets.items()}
d_tr, ev_tr = diffs(tr["P"], tr["S"], W["train"])
Pp = np.concatenate([g[f"{s}_P"] for s in ("test", "deep", "fresh")])
Sp = np.concatenate([g[f"{s}_S"] for s in ("test", "deep", "fresh")])
wp = np.concatenate([g[f"{s}_w"] / g[f"{s}_w"].mean() for s in ("test", "deep", "fresh")])
d_po, ev_po = diffs(Pp, Sp, wp)
mode = g["window_mode"]
d_w, ev_w = diffs(g["window_P"], g["window_S"], g["window_w"])
q = mode == "full_1page"
d_wq, _ = diffs(g["window_P"][q], g["window_S"][q], g["window_w"][q])
d_wb, _ = diffs(g["window_P"][~q], g["window_S"][~q], g["window_w"][~q])


def spearman(a, b):
    ra, rb = np.argsort(np.argsort(a)), np.argsort(np.argsort(b))
    return float(np.corrcoef(ra, rb)[0, 1]) if len(a) > 2 else float("nan")


def summ(idx):
    idx = np.array(idx)
    b = idx[np.argmax(d_po[idx])]
    return dict(n=len(idx), train_share_gt0=float(np.mean(d_tr[idx] > 0)), pooled_share_gt0=float(np.mean(d_po[idx] > 0)),
                window_share_gt0=float(np.mean(d_w[idx] > 0)), median_train=float(np.median(d_tr[idx])),
                median_pooled=float(np.median(d_po[idx])), median_window=float(np.median(d_w[idx])),
                median_window_quiet=float(np.median(d_wq[idx])), median_window_busy=float(np.median(d_wb[idx])),
                spearman_train_pooled=spearman(d_tr[idx], d_po[idx]), spearman_pooled_window=spearman(d_po[idx], d_w[idx]),
                best_pooled=dict(name=C[b]["name"], pooled=float(d_po[b]), window=float(d_w[b]), train=float(d_tr[b])),
                share_both_pooled_and_window_gt0=float(np.mean((d_po[idx] > 0) & (d_w[idx] > 0))))


out = {}
fam = lambda f: [i for i, c in enumerate(C) if c["fam"] == f]
for f in ("scaleout", "gate", "flow", "reentry", "ladder"):
    out[f] = summ(fam(f))
sub = {}
for x in (0.0, 0.25, 0.5, 0.75):
    sub[f"scaleout x={x}"] = [i for i in fam("scaleout") if C[i]["p"]["x"] == x]
for cap in (10, 20, 60):
    sub[f"scaleout cap{cap}"] = [i for i in fam("scaleout") if C[i]["p"]["cap2"] == cap]
G = fam("gate")
sub["gate g=0"] = [i for i in G if C[i]["p"]["g"] == 0]
sub["gate g=1"] = [i for i in G if C[i]["p"]["g"] == 1]
sub["gate pass:lead fail:exit (bail)"] = [i for i in G if C[i]["p"]["loose"] == "lead" and C[i]["p"]["fail"] == "exit"]
sub["gate pass:lead fail:exit g=0"] = [i for i in sub["gate pass:lead fail:exit (bail)"] if C[i]["p"]["g"] == 0]
sub["gate fail:exit (any pass)"] = [i for i in G if C[i]["p"]["fail"] == "exit"]
sub["gate fail:tight TP (8/10%)"] = [i for i in G if "fail_tp" in C[i]["p"]]
sub["gate fail:scalp, loose pass"] = [i for i in G if C[i]["p"]["fail"] == "scalp" and "fail_tp" not in C[i]["p"] and C[i]["p"]["loose"] != "lead"]
for tp in (0.2, 0.3, 0.5, 1.0, None):
    sub[f"gate loose tp={tp}"] = [i for i in G if isinstance(C[i]["p"]["loose"], dict) and C[i]["p"]["loose"]["tp"] == tp]
for cap in (5, 10, 20, 60):
    sub[f"gate loose cap{cap}"] = [i for i in G if isinstance(C[i]["p"]["loose"], dict) and C[i]["p"]["loose"]["cap"] == cap]
for ex in ("netsell0", "netsell1", "anysell0.3", "anysell0.5", "anysell1.0", "snipersell"):
    sub[f"flow +{ex}"] = [i for i in fam("flow") if C[i]["p"]["extra"] == (ex,)]
for d in (0.10, 0.15, 0.20, 0.30):
    sub[f"reentry dip{int(d*100)}"] = [i for i in fam("reentry") if C[i]["p"]["d"] == d]
for ft in ("me", "nB"):
    sub[f"ladder {ft}"] = [i for i in fam("ladder") if C[i]["p"]["feat"] == ft]
out["sub"] = {k: summ(v) for k, v in sub.items() if v}
out["lead"] = dict(train=float(ev_tr[0]), pooled=float(ev_po[0]), window=float(ev_w[0]))
json.dump(out, open(REPO / "data/reports/strategy/refinements_grid_summary.json", "w"), indent=1)
for k in ("scaleout", "gate", "flow", "reentry", "ladder"):
    v = out[k]
    print(f"{k:9s} n={v['n']:5d} train>0 {v['train_share_gt0']:.2f} pooled>0 {v['pooled_share_gt0']:.2f} window>0 {v['window_share_gt0']:.2f} both {v['share_both_pooled_and_window_gt0']:.2f} "
          f"med d train {v['median_train']*100:+.2f} pooled {v['median_pooled']*100:+.2f} window {v['median_window']*100:+.2f} (q {v['median_window_quiet']*100:+.2f} b {v['median_window_busy']*100:+.2f}) rho {v['spearman_train_pooled']:+.2f}/{v['spearman_pooled_window']:+.2f}")
    print("           best pooled:", v["best_pooled"])
for k, v in out["sub"].items():
    print(f"  {k:36s} n={v['n']:5d} pooled>0 {v['pooled_share_gt0']:.2f} window>0 {v['window_share_gt0']:.2f} med d train {v['median_train']*100:+.2f} pooled {v['median_pooled']*100:+.2f} window {v['median_window']*100:+.2f} (q {v['median_window_quiet']*100:+.2f} b {v['median_window_busy']*100:+.2f}) best {v['best_pooled']['pooled']*100:+.2f}/{v['best_pooled']['window']*100:+.2f}")
