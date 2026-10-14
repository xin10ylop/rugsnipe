"""G1 step 2: walk-forward selection on the predictor table (docs/research/strategy-predictor.md).

  .venv/bin/python -I analysis/scripts/strategy/predictor_wf.py dev       # pre-Oct-12 folds only (selection)
  .venv/bin/python -I analysis/scripts/strategy/predictor_wf.py freeze    # fit the 3 chosen rules on pre-Oct-12
  .venv/bin/python -I analysis/scripts/strategy/predictor_wf.py holdout   # frozen rules, once, on Oct 12-14
  .venv/bin/python -I analysis/scripts/strategy/predictor_wf.py full      # same grid, walk-forward through Oct 14

Two variants of the training data:
  rand: random-sample rows only (gap-3, deep, fresh, window); operator history from those rows only.
  pool: + the operator-pool launches of strategy-operators.md as extra TRAINING rows and as operator
        history; tests are always on random-sample rows.
Outputs under data/reports/strategy/predictor_*.json.
"""
from __future__ import annotations

import hashlib
import json
import math
import pickle
import sys
import time
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
from rugsnipe_analysis.strategy import predictor_models as M  # noqa: E402
from rugsnipe_analysis.strategy import stats  # noqa: E402

OUT = REPO / "data" / "reports" / "strategy"
FROZEN = OUT / "predictor_frozen.json"
FROZEN_PKL = OUT / "predictor_frozen_models.pkl"
KEEP = (0.8, 0.6, 0.4, 0.2)                 # share of launches kept (train-quantile threshold)
MODELS = ("lr_0.05", "lr_0.5", "gbt_2", "gbt_3")
TARGETS = ("busy", "tp", "ret", "s0t", "n10")
UNI_Q = (0.25, 0.5, 0.75)
VARIANTS = {"rand": "op_tab", "pool": "op_all"}

FEATURE_SETS = {
    "all": None,
    "core": ["nB", "bsol", "dev_sol", "me", "dev_share", "b_cv", "b_round", "b_tok_eq", "tx_index_log", "jito_tip",
             "prio_log", "cu_limit_set", "host_j7tracker", "host_ipfsio", "host_uxento", "host_mypinata",
             "host_rapidlaunch", "host_other", "fund_serial", "fund_mother_fresh", "fund_cex_fresh",
             "fund_n_prior_log", "op_g1_prior_log", "create_rate60", "regime_post", "m_n_social", "sym_ticker"],
}


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def wquant(x, w, q):
    o = np.argsort(x, kind="mergesort")
    cw = np.cumsum(w[o]) / w.sum()
    return float(x[o][min(len(x) - 1, int(np.searchsorted(cw, q)))])


def cols(d, fs):
    names = FEATURE_SETS[fs]
    if names is None:
        return np.arange(len(d.names))
    return np.array([d.names.index(n) for n in names])


def target_y(d, target):
    if target == "busy":
        return d.busy, "busy"
    if target == "tp":
        return d.tp, "tp"
    if target == "s0t":                     # followers: third-party slot-0 buyers (log1p); a label only
        return np.log1p(d.s0t), "ret"
    if target == "n10":                     # trades in the first 10 s (log1p); a label only
        return np.log1p(d.n10), "ret"
    return d.ret["base"], "ret"


def uni_rule_sel(x, thr=None, dirn="ge", lo=None, hi=None, xmin=None, xmax=None):
    if dirn == "band":
        return (x >= lo) & (x <= hi)
    if dirn == "ge":
        return x > thr if thr == xmin else x >= thr
    return x < thr if thr == xmax else x <= thr


def uni_threshold(x, w, q, dirn):
    if dirn == "band":
        lo_q, hi_q = q
        return dict(lo=wquant(x, w, lo_q), hi=wquant(x, w, hi_q))
    return dict(thr=wquant(x, w, q), xmin=float(x.min()), xmax=float(x.max()))


def run_grid(d, train_ok, test_ok, edges, variant):
    """Walk-forward: folds by time; train = before fold - embargo & train_ok; test = in fold & test_ok."""
    F = []
    for a, b in zip(edges[:-1], edges[1:]):
        tr = (d.bt < a - M.EMBARGO) & train_ok
        te = (d.bt >= a) & (d.bt < b) & test_ok
        if (tr & ~d.pool).sum() >= 60 and te.sum() > 0:
            F.append((tr, te))
    n = len(d.bt)
    te_all = np.zeros(n, bool)
    for _, te in F:
        te_all |= te
    configs, preds = {}, {}
    # --- univariate rules (rand: every feature; pool: only the operator-history features it changes)
    for j, fname in enumerate(d.names):
        if variant == "pool" and not fname.startswith("op_"):
            continue
        x = d.X[:, j]
        if np.std(x[train_ok]) == 0:
            continue
        specs = [(q, dirn) for q in UNI_Q for dirn in ("ge", "le")]
        if len(np.unique(x)) > 4:
            specs += [((0.25, 0.75), "band"), ((0.1, 0.9), "band")]
        for q, dirn in specs:
            sel = np.zeros(n, bool)
            for tr, te in F:
                th = uni_threshold(x[tr], d.w[tr], q, dirn)
                sel[te] = uni_rule_sel(x[te], dirn=dirn, **th)
            qn = f"{q[0]}-{q[1]}" if dirn == "band" else f"q{q}"
            configs[f"{variant}:uni:{fname}:{dirn}:{qn}"] = dict(sel=sel, fam="uni")
    # --- models
    for fs in FEATURE_SETS:
        cj = cols(d, fs)
        for kind in MODELS:
            for target in TARGETS:
                y, mt = target_y(d, target)
                p = np.full(n, np.nan)
                thrs = {k: np.zeros(n) for k in KEEP}
                ter = {k: np.zeros(n) for k in (1, 2)}
                for tr, te in F:
                    trm = tr & ~np.isnan(y)
                    pte, ptr, _ = M.fit_predict(kind, mt, d.X[trm][:, cj], y[trm], d.w[trm], d.X[te][:, cj])
                    p[te] = pte
                    rnd = trm & ~d.pool              # thresholds from the random-sample training rows
                    ptr_r = ptr[rnd[trm]]
                    wtr = d.w[rnd]
                    for k in KEEP:
                        thrs[k][te] = wquant(ptr_r, wtr, 1 - k)
                    ter[1][te] = wquant(ptr_r, wtr, 1 / 3)
                    ter[2][te] = wquant(ptr_r, wtr, 2 / 3)
                key = f"{variant}:{fs}:{kind}:{target}"
                preds[key] = p
                for k in KEEP:
                    configs[f"mdl:{key}:keep{k}"] = dict(sel=te_all & (p >= thrs[k]), fam=f"{kind}:{target}")
                size = np.where(p >= ter[2], 1.0, np.where(p >= ter[1], 0.5, 0.25))
                configs[f"mdl:{key}:size"] = dict(sel=te_all.copy(), size=size, fam=f"{kind}:{target}")
                size2 = np.where(p >= ter[2], 1.0, np.where(p >= ter[1], 0.5, 0.0))
                configs[f"mdl:{key}:size_skip"] = dict(sel=te_all & (size2 > 0), size=size2, fam=f"{kind}:{target}")
    return te_all, configs, preds


def size_stats(d, te, sz, w):
    ret = d.ret["base"][te]
    r_by = {1.0: d.ret["s100"][te], 0.5: ret, 0.25: d.ret["s025"][te], 0.0: 0 * ret}
    r_sz = np.select([sz >= 1.0, sz >= 0.5, sz >= 0.25], [r_by[1.0], r_by[0.5], r_by[0.25]], 0.0)
    pnl = sz * r_sz
    # control: the same size mix assigned at random (isolates the prediction from the fixed-cost
    # amortisation of bigger tickets)
    ctrl = np.zeros_like(pnl)
    for s_ in (1.0, 0.5, 0.25, 0.0):
        share = float(np.sum(w * (sz == s_)) / w.sum())
        ctrl += share * s_ * r_by[s_]
    diff = pnl - ctrl
    mu = float(np.sum(w * diff) / w.sum())
    sd = math.sqrt(np.sum(w * (diff - mu) ** 2) / w.sum())
    neff = w.sum() ** 2 / np.sum(w ** 2)
    p = float(stats.norm_sf(mu / (sd / math.sqrt(neff)))) if sd > 0 else float("nan")
    k = sz > 0
    return dict(kind="size", sol_per_launch=float(np.sum(w * pnl) / w.sum()), staked_per_launch=float(np.sum(w * sz) / w.sum()),
                ctrl_sol_per_launch=float(np.sum(w * ctrl) / w.sum()), flat_sol_per_launch=0.5 * stats.wmean(ret, w),
                ev_per_sol=float(np.sum(w[k] * pnl[k]) / np.sum(w[k] * sz[k])) if k.any() else float("nan"),
                gain_vs_ctrl=mu, p_lift=p, frac=float(np.sum(w[k]) / w.sum()), n=int(k.sum()))


def evaluate(d, te_all, configs, preds, label):
    w = d.w[te_all]
    ret = d.ret["base"][te_all]
    days = (d.bt[te_all].max() - d.bt[te_all].min()) / 86400
    base = M.sel_stats(ret, w, np.ones(te_all.sum(), bool), days)
    res = []
    for name, c in configs.items():
        sel = c["sel"][te_all]
        if "size" in c:
            s = size_stats(d, te_all, c["size"][te_all], w)
            s.update(name=name, fam=c["fam"])
            res.append(s)
            continue
        s = M.sel_stats(ret, w, sel, days)
        s.update(name=name, fam=c["fam"], kind="select")
        for sc in ("p036", "c010"):
            rr = d.ret[sc][te_all]
            s[f"ev_{sc}"] = stats.wmean(rr[sel], w[sel]) if sel.sum() else float("nan")
        bz = d.busy[te_all]
        okb = ~np.isnan(bz)
        s["busy_kept"] = stats.wmean(bz[sel & okb], w[sel & okb]) if (sel & okb).sum() else float("nan")
        s["busy_dropped"] = stats.wmean(bz[~sel & okb], w[~sel & okb]) if (~sel & okb).sum() else float("nan")
        res.append(s)
    ps = np.array([r["p_lift"] if r.get("p_lift") == r.get("p_lift") and r.get("p_lift") is not None else 1.0 for r in res])
    q = stats.bh_qvalues(ps)
    pp = np.array([r["p_pos"] if r.get("p_pos") == r.get("p_pos") and r.get("p_pos") is not None else 1.0 for r in res])
    qp = stats.bh_qvalues(pp)
    for r, qq, qpp in zip(res, q, qp):
        r["q_lift"] = float(qq)
        r["q_pos"] = float(qpp)
    aucs = {}
    for key, p in preds.items():
        m = te_all & ~np.isnan(p)
        aucs[key] = dict(auc_busy=M.auc(d.busy[m], p[m], d.w[m]), auc_tp=M.auc(d.tp[m], p[m], d.w[m]),
                         auc_pos=M.auc((d.ret["base"][m] > 0).astype(float), p[m], d.w[m]),
                         auc_s0t2=M.auc((d.s0t[m] >= 2).astype(float), p[m], d.w[m]))
    return dict(label=label, base=base, n_test=int(te_all.sum()), days=days, n_configs=len(res), results=res, aucs=aucs)


def fmt_sel(r):
    return (f"{r['name']:<50s} n={r['n']:4d} keep={r.get('frac', 0)*100:4.0f}% EV={r['ev']*100:+6.2f}% "
            f"lift={r.get('lift', 0)*100:+5.2f} [{r.get('lo', 0)*100:+5.1f},{r.get('hi', 0)*100:+5.1f}] "
            f"p={r.get('p_lift', 1):.3f} q={r['q_lift']:.2f} hit={r.get('hit', 0):.2f} busy {r.get('busy_kept', 0):.2f}/{r.get('busy_dropped', 0):.2f} "
            f"p036={r.get('ev_p036', 0)*100:+.1f}")


def report(out, top=25):
    sel = [r for r in out["results"] if r["kind"] == "select" and r["n"] >= 30 and r.get("frac", 0) >= 0.15]
    print(f"\n[{out['label']}] configs {out['n_configs']}, test launches {out['n_test']}, base EV {out['base']['ev']*100:+.2f}%")
    print(f"Top {top} selection configs by lower bound (keep >= 15 %):")
    for r in sorted(sel, key=lambda r: -r["lo"])[:top]:
        print(" ", fmt_sel(r))
    print("Top 12 by lift p:")
    for r in sorted(sel, key=lambda r: r["p_lift"])[:12]:
        print(" ", fmt_sel(r))
    sz = [r for r in out["results"] if r["kind"] == "size"]
    print("Sizing (SOL per launch; ctrl = same size mix at random):")
    for r in sorted(sz, key=lambda r: r["p_lift"])[:8]:
        print(f"  {r['name']:<46s} sol/launch={r['sol_per_launch']*1000:+.2f}m ctrl={r['ctrl_sol_per_launch']*1000:+.2f}m "
              f"flat0.5={r['flat_sol_per_launch']*1000:+.2f}m gain={r['gain_vs_ctrl']*1000:+.2f}m p={r['p_lift']:.3f} q={r['q_lift']:.2f} "
              f"ev/SOL={r['ev_per_sol']*100:+.2f}%")


def load(variant):
    rows = M.load_table()
    d = M.make_data(rows, VARIANTS[variant])
    return rows, d


def grid(edges, test_mask_fn, label):
    allres = []
    allaucs = {}
    outs = {}
    for variant in VARIANTS:
        rows, d = load(variant)
        train_ok = np.ones(len(rows), bool) if variant == "pool" else ~d.pool
        test_ok = ~d.pool & test_mask_fn(d)
        te_all, configs, preds = run_grid(d, train_ok, test_ok, edges, variant)
        out = evaluate(d, te_all, configs, preds, f"{label}:{variant}")
        outs[variant] = (d, te_all, configs, preds)
        allres += out["results"]
        allaucs.update(out["aucs"])
        base = out["base"]
        log(variant, "configs", out["n_configs"], "test", out["n_test"])
    # BH across both variants together
    ps = np.array([r["p_lift"] if r.get("p_lift") == r.get("p_lift") and r.get("p_lift") is not None else 1.0 for r in allres])
    q = stats.bh_qvalues(ps)
    for r, qq in zip(allres, q):
        r["q_lift"] = float(qq)
    return dict(label=label, base=base, n_test=out["n_test"], n_configs=len(allres), results=allres, aucs=allaucs), outs


def dev():
    out, _ = grid(M.DEV_EDGES, lambda d: d.bt < M.OCT12, "dev")
    report(out, 30)
    print("\nModel AUCs (dev OOS):")
    for k, v in sorted(out["aucs"].items(), key=lambda kv: -kv[1]["auc_tp"])[:20]:
        print(f"  {k:<32s} busy={v['auc_busy']:.3f} tp={v['auc_tp']:.3f} pos={v['auc_pos']:.3f} s0t>=2={v['auc_s0t2']:.3f}")
    json.dump(out, open(OUT / "predictor_dev.json", "w"), indent=1, default=float)


def full():
    out, outs = grid(M.FULL_EDGES, lambda d: np.ones(len(d.bt), bool), "full")
    report(out, 30)
    # same configs, OOS restricted to the Oct 12-14 folds
    post_res = []
    for variant, (d, te_all, configs, preds) in outs.items():
        post = te_all & (d.bt >= M.OCT12)
        o = evaluate(d, post, configs, preds, f"post:{variant}")
        post_res += o["results"]
        out.setdefault("aucs_post", {}).update(o["aucs"])
    ps = np.array([r["p_lift"] if r.get("p_lift") == r.get("p_lift") and r.get("p_lift") is not None else 1.0 for r in post_res])
    for r, qq in zip(post_res, stats.bh_qvalues(ps)):
        r["q_lift"] = float(qq)
    out["post"] = dict(results=post_res, base=o["base"], n_test=o["n_test"], label="full_post_oct12", n_configs=len(post_res))
    report(dict(out["post"], label="full, OOS folds of Oct 12-14 only"), 20)
    print("\nModel AUCs (full OOS):")
    for k, v in sorted(out["aucs"].items(), key=lambda kv: -kv[1]["auc_tp"])[:24]:
        print(f"  {k:<32s} busy={v['auc_busy']:.3f} tp={v['auc_tp']:.3f} pos={v['auc_pos']:.3f} s0t>=2={v['auc_s0t2']:.3f}")
    json.dump(out, open(OUT / "predictor_full.json", "w"), indent=1, default=float)


# ----------------------------------------------------------------------------- freeze / holdout
def freeze(choice: list[str]):
    """Fit each chosen config on ALL pre-Oct-12 training rows; store thresholds and fitted models."""
    frozen, models = [], {}
    for name in choice:
        parts = name.split(":")
        if parts[1] == "uni":
            variant, _, fname, dirn, qn = parts
        else:
            _, variant, fs, kind, target, pol = parts
        rows, d = load(variant)
        tr = (d.bt < M.OCT12 - M.EMBARGO) & (np.ones(len(rows), bool) if variant == "pool" else ~d.pool)
        rnd = tr & ~d.pool
        if parts[1] == "uni":
            j = d.names.index(fname)
            q = tuple(float(v) for v in qn.split("-")) if dirn == "band" else float(qn[1:])
            th = uni_threshold(d.X[rnd, j], d.w[rnd], q, dirn)
            frozen.append(dict(name=name, type="uni", variant=variant, feature=fname, dirn=dirn, **th,
                               train_keep=float(stats.wmean(uni_rule_sel(d.X[rnd, j], dirn=dirn, **th).astype(float), d.w[rnd]))))
            continue
        cj = cols(d, fs)
        y, mt = target_y(d, target)
        trm = tr & ~np.isnan(y)
        _, ptr, model = M.fit_predict(kind, mt, d.X[trm][:, cj], y[trm], d.w[trm], d.X[trm][:1, cj])
        ptr_r = ptr[rnd[trm]]
        wtr = d.w[rnd]
        ent = dict(name=name, type="model", variant=variant, fs=fs, kind=kind, target=target, policy=pol,
                   features=[d.names[i] for i in cj], n_train=int(trm.sum()), n_train_random=int(rnd.sum()))
        if pol.startswith("keep"):
            k = float(pol[4:])
            ent["threshold"] = wquant(ptr_r, wtr, 1 - k)
        else:
            ent["t1"], ent["t2"] = wquant(ptr_r, wtr, 1 / 3), wquant(ptr_r, wtr, 2 / 3)
        models[name] = model
        frozen.append(ent)
    blob = pickle.dumps(models)
    with open(FROZEN_PKL, "wb") as f:
        f.write(blob)
    doc = dict(frozen_at=time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime()), data="pre-Oct-12 rows only",
               models_sha256=hashlib.sha256(blob).hexdigest(), rules=frozen)
    json.dump(doc, open(FROZEN, "w"), indent=1)
    print(json.dumps(doc, indent=1))


def apply_rule(ent, d, models):
    if ent["type"] == "uni":
        j = d.names.index(ent["feature"])
        th = {k: ent[k] for k in ("thr", "xmin", "xmax", "lo", "hi") if k in ent}
        sel = uni_rule_sel(d.X[:, j], dirn=ent["dirn"], **th)
        return sel, None, d.X[:, j]
    cj = np.array([d.names.index(n) for n in ent["features"]])
    m = models[ent["name"]]
    X = d.X[:, cj]
    if ent["kind"].startswith("lr"):
        sc, lr = m
        A = np.clip(sc.transform(X), -5, 5)
        p = lr.predict(A) if ent["target"] in ("ret", "s0t", "n10") else lr.predict_proba(A)[:, 1]
    elif ent["target"] in ("busy", "tp"):
        p = m.predict_proba(X)[:, 1]
    else:
        p = m.predict(X)
    if ent["policy"].startswith("keep"):
        return p >= ent["threshold"], None, p
    sz = np.where(p >= ent["t2"], 1.0, np.where(p >= ent["t1"], 0.5, 0.25 if ent["policy"] == "size" else 0.0))
    return sz > 0, sz, p


def holdout():
    from rugsnipe_analysis.strategy import predictor as P
    doc = json.load(open(FROZEN))
    models = pickle.load(open(FROZEN_PKL, "rb"))
    assert hashlib.sha256(open(FROZEN_PKL, "rb").read()).hexdigest() == doc["models_sha256"]
    fresh_m = {t.mint for t in P.load_fresh() if t.kind == "multi"}
    win_m = {t.mint for t in P.load_window() if t.kind == "multi"}
    out = dict(frozen=doc, sets={})
    for ent in doc["rules"]:
        rows, d = load(ent["variant"])
        sel, sz, score = apply_rule(ent, d, models)
        mints = np.array([r["mint"] for r in rows])
        post = (d.bt >= M.OCT12) & ~d.pool
        sets = {"fresh": post & np.isin(mints, list(fresh_m)), "window": post & np.isin(mints, list(win_m)),
                "deep": post & (d.src == "deep"), "gap3_post": post & (d.src == "gap3"), "all_post": post}
        res = {}
        for sname, m in sets.items():
            if m.sum() == 0:
                continue
            w = d.w[m]
            days = 1.0
            if sz is not None and ent["policy"].startswith("size"):
                s = size_stats(d, m, sz[m], w)
            else:
                s = M.sel_stats(d.ret["base"][m], w, sel[m], days)
                for sc in ("p036", "c010", "lat2"):
                    s[f"ev_{sc}"] = stats.wmean(d.ret[sc][m][sel[m]], w[sel[m]]) if sel[m].sum() else float("nan")
                    s[f"ev_all_{sc}"] = stats.wmean(d.ret[sc][m], w)
                bz = d.busy[m]
                ok = ~np.isnan(bz)
                s["busy_kept"] = stats.wmean(bz[sel[m] & ok], w[sel[m] & ok]) if (sel[m] & ok).any() else None
                s["busy_dropped"] = stats.wmean(bz[~sel[m] & ok], w[~sel[m] & ok]) if (~sel[m] & ok).any() else None
                s["ev_dropped"] = stats.wmean(d.ret["base"][m][~sel[m]], w[~sel[m]]) if (~sel[m]).any() else None
                lo, hi, pboot = stats.boot(d.ret["base"][m][sel[m]], w[sel[m]])
                s["boot_lo"], s["boot_hi"], s["boot_p"] = lo, hi, pboot
                # bootstrap of the lift (selected minus all), resampling launches
                rng = np.random.default_rng(3)
                rr, ss = d.ret["base"][m], sel[m]
                lifts = []
                for _ in range(2000):
                    ix = rng.integers(0, len(rr), len(rr))
                    if ss[ix].sum() < 2:
                        continue
                    lifts.append(stats.wmean(rr[ix][ss[ix]], w[ix][ss[ix]]) - stats.wmean(rr[ix], w[ix]))
                s["lift_lo"], s["lift_hi"] = (float(np.quantile(lifts, 0.05)), float(np.quantile(lifts, 0.95)))
                s["lift_p"] = float((np.sum(np.array(lifts) <= 0) + 1) / (len(lifts) + 1))
                s["tp_kept"] = stats.wmean(d.tp[m][sel[m]], w[sel[m]]) if sel[m].any() else None
                s["tp_all"] = stats.wmean(d.tp[m], w)
            s["auc_busy"] = M.auc(d.busy[m], score[m], w)
            s["auc_tp"] = M.auc(d.tp[m], score[m], w)
            s["auc_pos"] = M.auc((d.ret["base"][m] > 0).astype(float), score[m], w)
            res[sname] = s
        out["sets"][ent["name"]] = res
        print(f"\n== {ent['name']}")
        for sname, s in res.items():
            if s.get("kind") == "size":
                print(f"  {sname:<10s} n={s['n']} sol/launch={s['sol_per_launch']*1000:+.2f}m ctrl={s['ctrl_sol_per_launch']*1000:+.2f}m "
                      f"gain={s['gain_vs_ctrl']*1000:+.2f}m p={s['p_lift']:.3f} ev/SOL={s['ev_per_sol']*100:+.2f}% aucTP={s['auc_tp']:.3f}")
            else:
                print(f"  {sname:<10s} n_all={s['n_all']} kept={s['n']} ({s.get('frac', 0)*100:.0f}%) EV kept={s['ev']*100:+.2f}% "
                      f"[{s['boot_lo']*100:+.1f},{s['boot_hi']*100:+.1f}] all={s['ev_all']*100:+.2f}% dropped={(s['ev_dropped'] or 0)*100:+.2f}% "
                      f"lift={s['lift']*100:+.2f} [{s['lift_lo']*100:+.1f},{s['lift_hi']*100:+.1f}] p={s['lift_p']:.3f} "
                      f"busy {s['busy_kept'] if s['busy_kept'] is None else round(s['busy_kept'], 2)}/{s['busy_dropped'] if s['busy_dropped'] is None else round(s['busy_dropped'], 2)} "
                      f"aucB={s['auc_busy']:.3f} aucTP={s['auc_tp']:.3f} p036={s['ev_p036']*100:+.1f} c010={s['ev_c010']*100:+.1f}")
    json.dump(out, open(OUT / "predictor_holdout.json", "w"), indent=1, default=float)


if __name__ == "__main__":
    what = sys.argv[1] if len(sys.argv) > 1 else "dev"
    if what == "freeze":
        freeze(sys.argv[2:])
    else:
        {"dev": dev, "full": full, "holdout": holdout}[what]()
