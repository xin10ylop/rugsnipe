"""Task G3 quick screens, pre-registered in data/reports/strategy/screens_preregistration.json.

  wave   (b) second scalp on the first pullback after a LEAD take-profit
  hour   (c) hour-of-day and create-rate conditioning of the LEAD
  grad   (a) graduation momentum on fresh PumpSwap pools (needs screens_pull_grad.py first)

Run: .venv/bin/python -I analysis/scripts/strategy/screens.py [wave|hour|grad|all]
Writes data/reports/strategy/screens_results.json (merged per stage).
"""
from __future__ import annotations

import collections
import json
import sys
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
sys.path.insert(0, str(REPO / "analysis/scripts/strategy"))
from rugsnipe_analysis.strategy import screens as sc  # noqa: E402

OUT = REPO / "data/reports/strategy/screens_results.json"
OOS = ("test", "deep", "fresh", "window")


def _sets():
    import refinements_data as rd
    sets = rd.load()
    # pooled OOS weights: gap-3 weights rescaled to mean 1 inside each gap-3 set
    W = {}
    for k, v in sets.items():
        w = np.array([t.w for t in v], float)
        W[k] = w / w.mean() if k in ("train", "test") else np.ones(len(v))
    return sets, W


def _save(key, obj):
    res = json.load(open(OUT)) if OUT.exists() else {}
    res[key] = obj
    json.dump(res, open(OUT, "w"), indent=1, default=float)


def _fmt(b):
    return f"{b['ev']*100:+.2f}% [{b['lo']*100:+.1f}, {b['hi']*100:+.1f}] n={b['n']} p={b['p']:.3f}"


# ----------------------------------------------------------------------------- (b)
def wave():
    sets, W = _sets()
    grid = [(d, Wd) for d in (0.05, 0.10, 0.15, 0.20) for Wd in (10, 30)]
    out = {"grid": [], "primary": None}
    leads = {k: [sc.lead_trade(t) for t in v] for k, v in sets.items()}
    for d, Wd in grid:
        row = {"d": d, "W": Wd, "sets": {}}
        pooled_x, pooled_w = [], []
        for k, v in sets.items():
            xs, ws, add, info = [], [], [], collections.Counter()
            n_tp = 0
            for t, lt, w in zip(v, leads[k], W[k]):
                if lt is None:
                    continue
                n_tp += lt.reason == "tp"
                r = sc.second_wave(t, lt, d=d, W=Wd)
                if r is None:
                    add.append((0.0, w))
                    continue
                xs.append(r[0]); ws.append(w); add.append((r[0], w)); info[r[1]["reason"]] += 1
            b = sc.boot(xs, ws)
            b["n_tp"] = n_tp
            b["added_per_lead_trade"] = sc.wmean([a for a, _ in add], [w for _, w in add])
            b["exit_reasons"] = dict(info)
            row["sets"][k] = b
            if k in OOS[:3]:
                pooled_x += xs; pooled_w += ws
        row["sets"]["pooled_oos"] = sc.boot(pooled_x, pooled_w)
        out["grid"].append(row)
        print(f"d={d:.2f} W={Wd:2d}s  train {_fmt(row['sets']['train'])} | pooled {_fmt(row['sets']['pooled_oos'])} | window {_fmt(row['sets']['window'])}")
    # BH over 8 configs x 4 OOS sets (test, deep, fresh, window)
    ps, keys = [], []
    for i, row in enumerate(out["grid"]):
        for k in OOS:
            p = row["sets"][k]["p"]
            if np.isfinite(p):
                ps.append(p); keys.append((i, k))
    q = sc.bh(ps)
    for (i, k), qq in zip(keys, q):
        out["grid"][i]["sets"][k]["q"] = float(qq)
    prim = next(r for r in out["grid"] if r["d"] == 0.10 and r["W"] == 30)
    out["primary"] = prim
    # stress on the primary: slower re-entry, premium 3.6%
    st = {}
    for name, kw in (("slow", dict(slow=True)), ("prem3.6", dict(prem=0.036)), ("cost0.01", dict(cost=0.01))):
        px, pw, wx = [], [], []
        for k, v in sets.items():
            for t, lt, w in zip(v, leads[k], W[k]):
                r = sc.second_wave(t, lt, d=0.10, W=30, **kw) if lt else None
                if r is None:
                    continue
                if k in OOS[:3]:
                    px.append(r[0]); pw.append(w)
                if k == "window":
                    wx.append(r[0])
        st[name] = {"pooled_oos": sc.boot(px, pw), "window": sc.boot(wx)}
        print(f"stress {name}: pooled {_fmt(st[name]['pooled_oos'])} | window {_fmt(st[name]['window'])}")
    out["primary_stress"] = st
    # descriptive: what happens after the TP (window + pooled), price path relative to our TP fill
    _save("b_second_wave", out)


# ----------------------------------------------------------------------------- (c)
def hour():
    sets, W = _sets()
    cr = sc.load_create_index(REPO)
    rows = {}
    for k, v in sets.items():
        rr = []
        for t, w in zip(v, W[k]):
            lt = sc.lead_trade(t)
            if lt is None:
                continue
            rr.append(dict(ret=lt.ret, w=float(w), hour=int((t.bt // 3600) % 24), r60=cr.per_hour(t.bt, 3600),
                           r10=cr.per_hour(t.bt, 600), busy=getattr(t, "mode", None) == "full", bt=t.bt))
        rows[k] = rr
    out = {"per_hour": {}, "tests": [], "claim_check": {}}
    # descriptive per 3-hour block for every set
    for k, rr in rows.items():
        blk = collections.defaultdict(list)
        for r in rr:
            blk[r["hour"] // 3 * 3].append(r)
        out["per_hour"][k] = {f"{h:02d}-{h+3:02d}": sc.boot([r["ret"] for r in g], [r["w"] for r in g]) for h, g in sorted(blk.items())}
    # claim as stated: 18-21 UTC on test + fresh (the sets that formed it)
    cl = [r for k in ("test", "fresh") for r in rows[k] if r["hour"] in (18, 19, 20)]
    out["claim_check"]["test+fresh 18-21"] = sc.boot([r["ret"] for r in cl], [r["w"] for r in cl])
    print("claim (test+fresh, 18-21 UTC):", _fmt(out["claim_check"]["test+fresh 18-21"]))
    tests = []
    # H1: hour 18-21 vs rest on train and deep
    for k in ("train", "deep"):
        a = [r for r in rows[k] if r["hour"] in (18, 19, 20)]
        b = [r for r in rows[k] if r["hour"] not in (18, 19, 20)]
        tests.append(("H1_hour18-21", k, sc.boot_diff([r["ret"] for r in a], [r["w"] for r in a], [r["ret"] for r in b], [r["w"] for r in b])))
    # H2/H3: create rate split at the gap-3-train median
    for key, H in (("r60", "H2_rate60"), ("r10", "H3_rate10")):
        thr = float(np.nanmedian([r[key] for r in rows["train"]]))
        out[f"threshold_{key}_per_hour"] = thr
        for k in ("test", "deep", "fresh", "window"):
            a = [r for r in rows[k] if np.isfinite(r[key]) and r[key] >= thr]
            b = [r for r in rows[k] if np.isfinite(r[key]) and r[key] < thr]
            tests.append((H, k, sc.boot_diff([r["ret"] for r in a], [r["w"] for r in a], [r["ret"] for r in b], [r["w"] for r in b])))
        wm = float(np.nanmedian([r[key] for r in rows["window"]]))
        a = [r for r in rows["window"] if r[key] >= wm]
        b = [r for r in rows["window"] if r[key] < wm]
        tests.append((H + "_withinwindow", "window", sc.boot_diff([r["ret"] for r in a], None, [r["ret"] for r in b], None)))
        # in-sample look on train, for the record
        a = [r for r in rows["train"] if r[key] >= thr]
        b = [r for r in rows["train"] if r[key] < thr]
        out[f"train_insample_{key}"] = sc.boot_diff([r["ret"] for r in a], [r["w"] for r in a], [r["ret"] for r in b], [r["w"] for r in b])
    tests = [x for x in tests if np.isfinite(x[2]["p"]) or print("untestable:", x[0], x[1], x[2]["n1"], x[2]["n0"])]
    q = sc.bh([t[2]["p"] for t in tests])
    for (H, k, d), qq in zip(tests, q):
        d["q"] = float(qq)
        out["tests"].append(dict(test=H, set=k, **d))
        print(f"{H:24s} {k:7s} high {d['ev1']*100:+.2f}% (n={d['n1']}) low {d['ev0']*100:+.2f}% (n={d['n0']}) diff {d['d']*100:+.2f} [{d['lo']*100:+.1f},{d['hi']*100:+.1f}] p={d['p']:.3f} q={qq:.2f}")
    # busy share by create rate on the window (does the rate predict busy?)
    wr = rows["window"]
    med = float(np.median([r["r60"] for r in wr]))
    out["window_busy_share"] = {"rate60_high": float(np.mean([r["busy"] for r in wr if r["r60"] >= med])),
                                "rate60_low": float(np.mean([r["busy"] for r in wr if r["r60"] < med]))}
    print("window busy share by rate60 half:", out["window_busy_share"])
    _save("c_hour_rate", out)
    return rows


if __name__ == "__main__":
    what = sys.argv[1] if len(sys.argv) > 1 else "all"
    if what in ("wave", "all"):
        wave()
    if what in ("hour", "all"):
        hour()
    if what in ("grad", "all"):
        import screens_grad
        screens_grad.main()
