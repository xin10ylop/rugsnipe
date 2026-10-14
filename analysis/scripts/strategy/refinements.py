"""Task G2: post-entry refinements of the LEAD micro-scalp.

  .venv/bin/python -I analysis/scripts/strategy/refinements.py select     # gap-3 TRAIN only -> frozen_refinements.json
  .venv/bin/python -I analysis/scripts/strategy/refinements.py evaluate   # frozen set once on test / deep / fresh / window
  .venv/bin/python -I analysis/scripts/strategy/refinements.py grid       # whole grid on every set (diagnostic, after freeze)

Selection: every configuration is scored on the gap-3 TRAIN split (first 60% of launches by create time,
multi-wallet bundled only, n=288) by the paired improvement over the LEAD on the same launches,
score = weighted mean(r_variant - r_LEAD) - 1.645 SE. One variant per family (scale-out, gate, flow exit,
re-entry, size ladder) is frozen: the best score in that family. The frozen file is written before any
out-of-sample number is computed. Returns are per launch, net of fees, own impact and fixed costs, as a
fraction of the 0.5 SOL stake (size ladders: SOL P&L per SOL deployed).
"""
from __future__ import annotations

import itertools
import json
import math
import multiprocessing as mp
import sys
import time
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
sys.path.insert(0, str(REPO / "analysis/scripts/strategy"))
from rugsnipe_analysis.strategy import postentry as pe, stats  # noqa: E402
from rugsnipe_analysis.strategy.tapes import drop_wallets  # noqa: E402
from refinements_data import load  # noqa: E402

OUT = REPO / "data/reports/strategy"
FROZEN = OUT / "frozen_refinements.json"
RESULTS = OUT / "refinements_results.json"
GRIDNPZ = REPO / "data/cache/strategy/refinements_grid.npz"
S0 = 0.5
BASE = dict(prem=0.011, cost=0.004, lat=1, xtx=0.002)
SIZES = (0.25, 0.5, 1.0)
OOS = ("test", "deep", "fresh", "window")


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


# ----------------------------------------------------------------------------- configuration grid
def build_configs(thr=None):
    C = [dict(fam="lead", name="LEAD", p={})]
    for x, tx, act, idle2, cap2 in itertools.product((0.0, 0.25, 0.5, 0.75), (0.10, 0.15, 0.20, 0.30), (0.0, 0.10, 0.20),
                                                      (2.0, 3.0), (10, 20, 60)):
        C.append(dict(fam="scaleout", name=f"scale x{x:g} trail{tx*100:g}@{act*100:g} idle{idle2:g} cap{cap2}",
                      p=dict(x=x, trail=(tx, act), idle2=idle2, cap2=cap2)))
    looses = ["lead"]
    for tp, tr, idle, cap in itertools.product((0.2, 0.3, 0.5, 1.0, None), (None, (0.15, 0.0), (0.2, 0.0), (0.3, 0.0)), (1.0, 2.0),
                                               (5, 10, 20, 60)):
        if tp is None and tr is None:
            continue
        looses.append(dict(tp=tp, trail=tr, idle=idle, cap=cap))
    for g, pmin, kmin, loose, fail in itertools.product((0, 1), (0.05, 0.10, 0.20), (0, 1, 2, 3, 5), looses,
                                                        ("scalp", "exit", "tp8", "tp10")):
        if loose == "lead" and fail == "scalp":
            continue  # identical to the LEAD apart from the deferred TP
        ln = "lead" if loose == "lead" else (f"tp{loose['tp']*100:g}" if loose["tp"] else "notp") + \
            (f"+trail{loose['trail'][0]*100:g}" if loose["trail"] else "") + f"+idle{loose['idle']:g}+cap{loose['cap']}"
        fp = dict(g=g, pmin=pmin, kmin=kmin, loose=loose, fail="exit" if fail == "exit" else "scalp")
        if fail.startswith("tp"):
            fp["fail_tp"] = int(fail[2:]) / 100
        C.append(dict(fam="gate", name=f"gate s+{g} p>={pmin*100:g}% k>={kmin} pass:{ln} fail:{fail}", p=fp))
    for tp, cap, ex in itertools.product((0.15, 0.20, 0.30), (5, 10), (("netsell0",), ("netsell1",), ("anysell0.3",), ("anysell0.5",),
                                                                     ("anysell1.0",), ("snipersell",), ("netsell1", "anysell0.5"))):
        C.append(dict(fam="flow", name=f"flow tp{tp*100:g} cap{cap} +{'+'.join(ex)}", p=dict(tp=tp, cap=cap, extra=ex)))
    for d, ref, W, only, rtp in itertools.product((0.10, 0.15, 0.20, 0.30), ("peak", "exit"), (3.0, 5.0), (False, True), (0.10, 0.15)):
        C.append(dict(fam="reentry", name=f"reenter dip{d*100:g}% from {ref} within {W:g}s {'after TP only' if only else 'after any non-insider exit'} re-tp{rtp*100:g}",
                      p=dict(d=d, ref=ref, W=W, only_tp=only, re_tp=rtp)))
    for feat in ("me", "nB"):
        for m in itertools.product(SIZES, repeat=3):
            if len(set(m)) == 1:
                continue  # flat sizes are references, not ladders
            C.append(dict(fam="ladder", name=f"ladder {feat} terciles -> {'/'.join(f'{x:g}' for x in m)} SOL", p=dict(feat=feat, sizes=m)))
    return C


def feat_of(t, feat):
    """Ladder features known at slot 0: post-bundle mcap (identical in rank to total bundle SOL, which sets
    it) or the number of non-creator bundle wallets."""
    return t.me if feat == "me" else t.nB


def run_cfg(v, cfg, sc, thr):
    f, p = cfg["fam"], cfg["p"]
    kw = dict(prem=sc["prem"], cost=sc["cost"])
    if f == "lead":
        o = pe.run_lead(v, S0, **kw)
    elif f == "scaleout":
        o = pe.run_scaleout(v, S0, xtx=sc["xtx"], **kw, **p)
    elif f == "gate":
        o = pe.run_gate(v, S0, **kw, **p)
    elif f == "flow":
        o = pe.run_flow(v, S0, **kw, **p)
    elif f == "reentry":
        o = pe.run_reentry(v, S0, **kw, **p)
    elif f == "ladder":
        x = feat_of(v.t, p["feat"])
        lo, hi = thr[p["feat"]]
        S = p["sizes"][0 if x <= lo else (1 if x <= hi else 2)]
        o = pe.run_lead(v, S, **kw)
    else:
        raise KeyError(f)
    return o.pnl, o.stake, o.reason


_G = {}


def _work(args):
    lo, hi = args
    V, C, sc, thr = _G["V"], _G["C"], _G["sc"], _G["thr"]
    P = np.zeros((len(V), hi - lo)); S = np.zeros((len(V), hi - lo))
    for j in range(lo, hi):
        for i, v in enumerate(V):
            P[i, j - lo], S[i, j - lo], _ = run_cfg(v, C[j], sc, thr)
    return lo, P, S


def matrix(tapes, C, sc=BASE, thr=None, procs=4):
    _G.update(V=[pe.TV(t, sc["lat"]) for t in tapes], C=C, sc=sc, thr=thr)
    n, m = len(tapes), len(C)
    P = np.zeros((n, m)); S = np.zeros((n, m))
    step = max(1, m // (procs * 8))
    chunks = [(i, min(m, i + step)) for i in range(0, m, step)]
    if procs > 1 and m > 8:
        with mp.get_context("fork").Pool(procs) as pool:
            for lo, Pc, Sc in pool.imap_unordered(_work, chunks):
                P[:, lo:lo + Pc.shape[1]] = Pc; S[:, lo:lo + Sc.shape[1]] = Sc
    else:
        for ch in chunks:
            lo, Pc, Sc = _work(ch)
            P[:, lo:lo + Pc.shape[1]] = Pc; S[:, lo:lo + Sc.shape[1]] = Sc
    return P, S


# ----------------------------------------------------------------------------- statistics
def wstats(x, w):
    sw = w.sum(); mu = float(w @ x / sw)
    n = len(x)
    se = float(np.sqrt((w ** 2) @ (x - mu) ** 2 * n / max(n - 1, 1)) / sw)
    return mu, se


def per_sol(P, S, w):
    return float((w @ P) / (w @ S))


def boot_idx(n, B=2000, seed=7):
    return np.random.default_rng(seed).integers(0, n, size=(B, n))


def score_cols(P, S, w, B=2000):
    """Per configuration: EV (per stake; ladders per SOL), paired diff vs LEAD (column 0), SE, LB, p."""
    n, m = P.shape
    R = P / S
    r0 = R[:, 0]
    out = []
    idx = boot_idx(n, B)
    for j in range(m):
        if np.allclose(S[:, j], S0):
            ev, _ = wstats(R[:, j], w)
            mu, se = wstats(R[:, j] - r0, w)
        else:  # ladder: per-SOL return, bootstrap SE of the per-SOL difference
            ev = per_sol(P[:, j], S[:, j], w)
            ww = w[idx]
            bj = (ww * P[idx, j]).sum(1) / (ww * S[idx, j]).sum(1)
            b0 = (ww * P[idx, 0]).sum(1) / (ww * S[idx, 0]).sum(1)
            mu = ev - per_sol(P[:, 0], S[:, 0], w)
            se = float(np.std(bj - b0))
        z = mu / se if se > 0 else 0.0
        p = float(0.5 * math.erfc(z / math.sqrt(2)))
        out.append(dict(ev=ev, diff=mu, se=se, lb=mu - 1.645 * se, p=p))
    return out


def summarize(P, S, w, j, B=2000, seed=7, ref=0):
    """EV, paired diff vs column `ref` (default the LEAD), bootstrap 90% CIs by launch."""
    n = len(w)
    if n == 0:
        return dict(n=0)
    idx = boot_idx(n, B, seed)
    ww = w[idx]
    ev = per_sol(P[:, j], S[:, j], w); ev0 = per_sol(P[:, ref], S[:, ref], w)
    bj = (ww * P[idx, j]).sum(1) / (ww * S[idx, j]).sum(1)
    b0 = (ww * P[idx, ref]).sum(1) / (ww * S[idx, ref]).sum(1)
    d = bj - b0
    sd = float(np.std(d))
    z = (ev - ev0) / sd if sd > 0 else 0.0
    R = P[:, j] / S[:, j]
    return dict(n=int(n), ev=ev, lo=float(np.quantile(bj, 0.05)), hi=float(np.quantile(bj, 0.95)),
                p_ev=float((np.sum(bj <= 0) + 1) / (B + 1)),
                lead=ev0, diff=ev - ev0, dlo=float(np.quantile(d, 0.05)), dhi=float(np.quantile(d, 0.95)),
                p_diff=float(0.5 * math.erfc(z / math.sqrt(2))),
                hit=float(w @ (R > 0) / w.sum()), sol_per_launch=float(w @ P[:, j] / w.sum()))


def thresholds(train):
    """me: train terciles; nB: fixed groups 1 / 2-3 / 4+ (the sweep's filters)."""
    return {"me": tuple(np.quantile([t.me for t in train], (1 / 3, 2 / 3)).tolist()), "nB": (1.0, 3.0)}


# ----------------------------------------------------------------------------- select / evaluate
def select():
    sets = load()
    tr = sets["train"]
    thr = thresholds(tr)
    C = build_configs()
    log("configurations", len(C), {f: sum(c["fam"] == f for c in C) for f in ("scaleout", "gate", "flow", "reentry", "ladder")})
    w = np.array([t.w for t in tr])
    P, S = matrix(tr, C, BASE, thr)
    sc = score_cols(P, S, w)
    np.savez_compressed(REPO / "data/cache/strategy/refinements_train.npz", P=P, S=S)
    qs = stats.bh_qvalues(np.array([s["p"] for s in sc[1:]]))
    for s, q in zip(sc[1:], qs):
        s["q"] = float(q)
    sc[0]["q"] = None
    R = P / S
    active = (np.abs(R - R[:, [0]]) > 1e-9).sum(0)
    rows = [dict(i=i, fam=c["fam"], name=c["name"], active=int(a), **s) for i, (c, s, a) in enumerate(zip(C, sc, active))]
    # ladders: information gain over a flat stake equal to the ladder's train-mean stake
    V = [pe.TV(t, 1) for t in tr]
    idx = boot_idx(len(tr), 2000)
    ww = w[idx]
    flat_cache = {}
    for r_ in rows:
        if r_["fam"] != "ladder":
            continue
        j = r_["i"]
        Sbar = round(float(w @ S[:, j] / w.sum()), 4)
        if Sbar not in flat_cache:
            flat_cache[Sbar] = np.array([pe.run_lead(v, Sbar, BASE["prem"], BASE["cost"]).pnl for v in V])
        Pf = flat_cache[Sbar]
        g = per_sol(P[:, j], S[:, j], w) - float(w @ Pf / (w.sum() * Sbar))
        bj = (ww * P[idx, j]).sum(1) / (ww * S[idx, j]).sum(1) - (ww * Pf[idx]).sum(1) / (ww.sum(1) * Sbar)
        se = float(np.std(bj))
        r_.update(flat_S=Sbar, gain=g, gain_se=se, gain_lb=g - 1.645 * se, gain_p=float(0.5 * math.erfc(g / se / math.sqrt(2))))
    lad = [r_ for r_ in rows if r_["fam"] == "ladder"]
    gq = stats.bh_qvalues(np.array([r_["gain_p"] for r_ in lad]))
    for r_, q in zip(lad, gq):
        r_["gain_q"] = float(q)
    frozen = []
    for fam in ("scaleout", "gate", "flow", "reentry", "ladder"):
        cand = [r_ for r_ in rows if r_["fam"] == fam and r_["active"] >= 20]
        best = max(cand, key=lambda r_: r_["gain_lb"] if fam == "ladder" else r_["lb"])
        frozen.append(dict(best, cfg=C[best["i"]]))
    fams = {}
    for fam in ("scaleout", "gate", "flow", "reentry", "ladder"):
        rr = [r for r in rows if r["fam"] == fam]
        d = np.array([r["diff"] for r in rr])
        fams[fam] = dict(n_configs=len(rr), share_diff_gt0=float(np.mean(d > 0)), median_diff=float(np.median(d)),
                         max_diff=float(d.max()), min_q=float(min(r["q"] for r in rr)), n_q_lt_10=int(sum(r["q"] < 0.10 for r in rr)),
                         n_active_ge20=int(sum(r["active"] >= 20 for r in rr)))
        if fam == "ladder":
            gg = np.array([r["gain"] for r in rr])
            fams[fam].update(share_gain_gt0=float(np.mean(gg > 0)), max_gain=float(gg.max()), min_gain_q=float(min(r["gain_q"] for r in rr)))
    doc = dict(frozen_at=time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime()),
               rule=("E1 rank-1 entry, 0.5 SOL, base scenario prem +1.1%, 0.004 SOL/round trip, 0.002 SOL per extra sell tx, "
                     "1-slot exit latency. Score = weighted mean paired improvement over the LEAD minus 1.645 SE on gap-3 TRAIN "
                     "(first 60% by create time, multi-wallet, n=288) only. Frozen: best score per family."),
               revision=("v2: v1 (frozen_refinements_v1_degenerate.json) picked a re-entry that never fires (diff 0, SE 0) and "
                         "the flat 1.0 SOL stake as the 'ladder'. Before any out-of-sample number: require >= 20 train launches "
                         "whose outcome differs from the LEAD; ladders exclude flat maps and are scored by the gain over a flat stake "
                         "of the same train-mean size; the 2nd ladder feature is the bundle wallet count (mcap is set by bundle SOL); "
                         "the gate grid adds pass-regime cap 5 / TP 20% and fail-branch TP 8% / 10%."),
               configs_tried=len(C) - 1, n_train=len(tr), ladder_thresholds=thr, lead_train=sc[0],
               frozen=frozen, families=fams,
               top20_train=sorted(rows[1:], key=lambda r: -r["lb"])[:20])
    json.dump(doc, open(FROZEN, "w"), indent=1, default=float)
    log(f"LEAD train EV {sc[0]['ev']*100:+.2f}%")
    for f in frozen:
        if f["fam"] == "ladder":
            log(f"   ladder gain over flat {f['flat_S']} SOL: {f['gain']*100:+.2f} (lb {f['gain_lb']*100:+.2f}) q={f['gain_q']:.3f}")
        log(f"FROZEN {f['fam']:8s} {f['name']:75s} train EV {f['ev']*100:+.2f}% diff {f['diff']*100:+.2f} (lb {f['lb']*100:+.2f}) q={f['q']:.3f}")
    for k, v in fams.items():
        log(k, v)


def scen_sets(sets, sc, drop=0.0, seed=11):
    out = {}
    for nm, T in sets.items():
        if drop > 0:
            rng = np.random.default_rng(seed)
            T2 = []
            for t in T:
                th = sorted(set(t.uid[t.s0tp].tolist()))
                T2.append(drop_wallets(t, {u for u in th if rng.random() < drop}))
            T = [t for t in T2 if t.pb < t.hend]
        out[nm] = T
    return out


def eval_cfgs(sets, C, sc, thr, B=2000, pairs=()):
    """Summaries of every config vs the LEAD (column 0); `pairs` = extra (name, j, ref) comparisons."""
    res = {}

    def summ(P, S, w):
        d = [summarize(P, S, w, j, B) for j in range(len(C))]
        for nm_, j, ref in pairs:
            d[j][f"vs_{nm_}"] = summarize(P, S, w, j, B, ref=ref)
        return d

    pool = []
    for nm, T in sets.items():
        w = np.array([t.w for t in T])
        P, S = matrix(T, C, sc, thr, procs=1)
        res[nm] = summ(P, S, w)
        if nm == "window":
            for mode in ("full_1page", "full", "prefix60"):
                m = np.array([getattr(t, "mode", None) == mode for t in T])
                if m.any():
                    res[f"window_{mode}"] = summ(P[m], S[m], w[m])
        if nm in ("test", "deep", "fresh"):
            pool.append((P, S, w / w.mean()))
    if pool:
        P = np.concatenate([x[0] for x in pool]); S = np.concatenate([x[1] for x in pool])
        w = np.concatenate([x[2] for x in pool])
        res["pooled_oos"] = summ(P, S, w)
    return res


def evaluate():
    doc = json.load(open(FROZEN))
    sets = load()
    thr = {k: tuple(v) for k, v in doc["ladder_thresholds"].items()}
    lead = build_configs()[0]
    C = [lead] + [f["cfg"] for f in doc["frozen"]]
    lad = next(f for f in doc["frozen"] if f["fam"] == "ladder")
    Sf = lad["flat_S"]
    C += [dict(fam="ladder", name="flat 0.25 SOL (reference)", p=dict(feat="me", sizes=[0.25] * 3)),
          dict(fam="ladder", name="flat 1.0 SOL (reference)", p=dict(feat="me", sizes=[1.0] * 3)),
          dict(fam="ladder", name=f"flat {Sf:g} SOL = ladder's train-mean stake (reference)", p=dict(feat="me", sizes=[Sf] * 3))]
    pairs = [("flat_matched", 1 + [f["fam"] for f in doc["frozen"]].index("ladder"), len(C) - 1)]
    for c in C:
        if c["fam"] == "gate" and isinstance(c["p"]["loose"], dict) and c["p"]["loose"].get("trail"):
            c["p"]["loose"]["trail"] = tuple(c["p"]["loose"]["trail"])
        if c["fam"] == "scaleout":
            c["p"]["trail"] = tuple(c["p"]["trail"])
        if c["fam"] == "flow":
            c["p"]["extra"] = tuple(c["p"]["extra"])
        if c["fam"] == "ladder":
            c["p"]["sizes"] = tuple(c["p"]["sizes"])
    allsets = {k: sets[k] for k in ("train",) + OOS}
    out = dict(frozen=doc["frozen"], names=[c["name"] for c in C], sets={k: len(v) for k, v in allsets.items()})
    log("base scenario")
    out["base"] = eval_cfgs(allsets, C, BASE, thr, pairs=pairs)
    # BH over 5 frozen variants x 4 OOS sets (improvement over the LEAD > 0, and EV > 0)
    keys, pd_, pe_ = [], [], []
    for j in range(1, 1 + len(doc["frozen"])):
        for s in OOS:
            r = out["base"][s][j]
            keys.append((j, s)); pd_.append(r["p_diff"]); pe_.append(r["p_ev"])
    qd = stats.bh_qvalues(np.array(pd_)); qe = stats.bh_qvalues(np.array(pe_))
    out["bh_oos"] = [dict(variant=C[j]["name"], set=s, p_diff=p1, q_diff=float(a), p_ev=p2, q_ev=float(b))
                     for (j, s), p1, a, p2, b in zip(keys, pd_, qd, pe_, qe)]
    for j, c in enumerate(C):
        b = out["base"]
        log(f"{c['fam']:8s} " + " | ".join(f"{s} {b[s][j]['ev']*100:+.2f} (d {b[s][j]['diff']*100:+.2f} [{b[s][j]['dlo']*100:+.1f},{b[s][j]['dhi']*100:+.1f}])"
                                          for s in ("train",) + OOS + ("pooled_oos",)))
    # latency 0..4
    out["latency"] = {}
    for lat in (0, 1, 2, 3, 4):
        sc = dict(BASE, lat=lat)
        out["latency"][lat] = eval_cfgs({k: allsets[k] for k in OOS}, C, sc, thr, B=500, pairs=pairs)
        log("lat", lat, " ".join(f"{c['fam']}:{out['latency'][lat]['pooled_oos'][j]['ev']*100:+.2f}/{out['latency'][lat]['window'][j]['ev']*100:+.2f}" for j, c in enumerate(C)))
    # cost / premium stress
    out["stress"] = {}
    for key, sc in (("cost0.01", dict(BASE, cost=0.01, xtx=0.005)), ("prem0.036", dict(BASE, prem=0.036)),
                    ("xtx0.004", dict(BASE, xtx=0.004))):
        out["stress"][key] = eval_cfgs({k: allsets[k] for k in OOS}, C, sc, thr, B=500, pairs=pairs)
        log(key, " ".join(f"{c['fam']}:{out['stress'][key]['pooled_oos'][j]['ev']*100:+.2f}/{out['stress'][key]['window'][j]['ev']*100:+.2f}" for j, c in enumerate(C)))
    # sniper-drop counterfactual
    out["sniper_drop"] = {}
    for f in (0.25, 0.5):
        ss = scen_sets({k: allsets[k] for k in OOS}, BASE, drop=f)
        out["sniper_drop"][f] = eval_cfgs(ss, C, BASE, thr, B=500, pairs=pairs)
        log("drop", f, " ".join(f"{c['fam']}:{out['sniper_drop'][f]['pooled_oos'][j]['ev']*100:+.2f}/{out['sniper_drop'][f]['window'][j]['ev']*100:+.2f}" for j, c in enumerate(C)))
    json.dump(out, open(RESULTS, "w"), indent=1, default=float)
    log("written", RESULTS)


def grid():
    """Whole grid on every set (diagnostic only; run after the freeze)."""
    doc = json.load(open(FROZEN))
    sets = load()
    thr = {k: tuple(v) for k, v in doc["ladder_thresholds"].items()}
    C = build_configs()
    res = {}
    for nm in OOS:
        T = sets[nm]
        w = np.array([t.w for t in T])
        P, S = matrix(T, C, BASE, thr)
        res[nm] = dict(P=P, S=S, w=w)
        log(nm, "done")
    np.savez_compressed(GRIDNPZ, **{f"{nm}_{k}": v for nm, d in res.items() for k, v in d.items()},
                        window_mode=np.array([getattr(t, "mode", "") for t in sets["window"]]))
    log("written", GRIDNPZ)


if __name__ == "__main__":
    {"select": select, "evaluate": evaluate, "grid": grid}[sys.argv[1]]()
