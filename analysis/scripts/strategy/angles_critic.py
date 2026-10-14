"""G4 skeptic: audits of the G1 predictor claims on the existing table (no network).

  .venv/bin/python -I analysis/scripts/strategy/angles_critic.py frozen   # frozen rule 3: clustering, leaks, fills
  .venv/bin/python -I analysis/scripts/strategy/angles_critic.py wf       # walk-forward winner's curse (split halves)

Results: data/reports/strategy/angles_critic_{frozen,wf}.json
"""
from __future__ import annotations

import collections
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
sys.path.insert(0, str(REPO / "analysis/scripts/strategy"))
from rugsnipe_analysis.strategy import predictor_models as M  # noqa: E402
from rugsnipe_analysis.strategy import stats  # noqa: E402
import predictor_wf as W  # noqa: E402

OUT = REPO / "data" / "reports" / "strategy"


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def cluster_boot(r, w, g, B=2000, seed=11):
    """Weighted mean with a bootstrap over clusters g (e.g. the signer). Returns (lo, hi, p_le0, n_clusters)."""
    ug, inv = np.unique(g, return_inverse=True)
    k = len(ug)
    sw = np.bincount(inv, weights=w, minlength=k)
    sr = np.bincount(inv, weights=w * r, minlength=k)
    rng = np.random.default_rng(seed)
    idx = rng.integers(0, k, size=(B, k))
    means = sr[idx].sum(1) / sw[idx].sum(1)
    lo, hi = np.quantile(means, (0.05, 0.95))
    return float(lo), float(hi), float((np.sum(means <= 0) + 1) / (B + 1)), int(k)


def cluster_boot_diff(r, w, g, sel, B=2000, seed=12):
    """Lift = mean(selected) - mean(all), bootstrap over clusters."""
    ug, inv = np.unique(g, return_inverse=True)
    k = len(ug)
    sw_s = np.bincount(inv, weights=w * sel, minlength=k)
    sr_s = np.bincount(inv, weights=w * r * sel, minlength=k)
    sw = np.bincount(inv, weights=w, minlength=k)
    sr = np.bincount(inv, weights=w * r, minlength=k)
    rng = np.random.default_rng(seed)
    idx = rng.integers(0, k, size=(B, k))
    a = sr_s[idx].sum(1) / np.maximum(sw_s[idx].sum(1), 1e-12)
    b = sr[idx].sum(1) / sw[idx].sum(1)
    d = a - b
    return float(np.quantile(d, 0.05)), float(np.quantile(d, 0.95)), float((np.sum(d <= 0) + 1) / (B + 1))


def frozen_scores(d, rows, ent, models, mutate=None):
    X = d.X.copy()
    if mutate:
        X = mutate(X, d.names)
    dd = M.Data(rows=rows, X=X, names=d.names, bt=d.bt, w=d.w)
    _, _, score = W.apply_rule(ent, dd, models)
    return score


def frozen():
    doc = json.load(open(W.FROZEN))
    models = pickle.load(open(W.FROZEN_PKL, "rb"))
    assert hashlib.sha256(open(W.FROZEN_PKL, "rb").read()).hexdigest() == doc["models_sha256"]
    ent = next(e for e in doc["rules"] if e["name"].endswith("tp:size_skip"))
    rows, d = W.load(ent["variant"])
    rows_t, d_t = W.load("rand")                       # same rows, op_tab history (no operator pools)
    assert [r["mint"] for r in rows] == [r["mint"] for r in rows_t]
    post = (d.bt >= M.OCT12) & ~d.pool
    signer = np.array([r["signer"] or r["mint"] for r in rows])
    src = d.src
    base = d.ret["base"]
    w = d.w

    meta_cols = [n for n in d.names if n.startswith("m_") or n == "meta_ok"]
    op_cols = [n for n in d.names if n.startswith("op_") and n != "op_g1_prior_log"]
    op_default = dict(op_prior_log=0.0, op_lead_n_log=0.0, op_has_hist=0.0, op_lead_hit=0.22, op_lead_ev=0.035,
                      op_busy=0.30, op_s0t=1.0)

    def no_meta(X, names):
        for n in meta_cols:
            X[:, names.index(n)] = 0.0
        return X

    def no_cu_used(X, names):
        j = names.index("cu_used")
        X[:, j] = np.median(X[(d.bt < M.OCT12) & ~d.pool, j])
        return X

    def live(X, names):
        return no_cu_used(no_meta(X, names), names)

    def no_op(X, names):
        for n in op_cols:
            X[:, names.index(n)] = op_default[n]
        return X

    def op_tab(X, names):
        X2 = d_t.X
        for n in op_cols:
            X[:, names.index(n)] = X2[:, d_t.names.index(n)]
        return X

    # funding look-ahead: deep/fresh rows whose funding record is as of a LATER create (window by-wallet record of
    # the signer's first window create, or analytics.db as of a later mint): blank their funding features
    import sqlite3
    wfirst = {}
    for line in open(REPO / "data/cache/window/creates.jsonl"):
        q = json.loads(line)
        wfirst[q["create_user"]] = min(wfirst.get(q["create_user"], 9e18), q["create_time"])
    db = sqlite3.connect(f"file:{REPO}/data/analytics.db?mode=ro", uri=True)
    asof = {a: m for a, m in db.execute("select wallet, as_of_mint from funding")}
    ct = {m: b for m, b in db.execute("select mint, block_time from creates")}
    late = np.array([(r["fund_src"] == "window" and r["src"] in ("deep", "fresh", "gap3") and r["bt"] < wfirst.get(r["signer"], 0))
                     or (r["fund_src"] == "db" and (ct.get(asof.get(r["signer"])) or 0) > r["bt"]) for r in rows])
    out_late = int(late.sum())
    fund_cols = [n for n in d.names if n.startswith("fund_")]

    def no_late_funding(X, names):
        for n in fund_cols:
            X[late, names.index(n)] = 0.0
        return X

    variants = {"as_frozen": None, "op_hist_without_pools": op_tab, "no_metadata": no_meta,
                "funding_lookahead_rows_blanked": no_late_funding,
                "live_feasible(no meta, cu_used=median)": live, "no_operator_history": no_op}
    out = {"rule": ent["name"], "t1": ent["t1"], "t2": ent["t2"], "variants": {}, "n_funding_lookahead_rows": out_late}
    log("rows with funding as of a later create", out_late)
    win = post & (src == "window")
    other = post & (src != "window")
    for vn, mut in variants.items():
        sc = frozen_scores(d, rows, ent, models, mut)
        res = {}
        for sn, m in (("all_post", post), ("window", win), ("other_post", other)):
            hi = m & (sc >= ent["t2"])
            lo_ = m & (sc < ent["t1"])
            keep = m & (sc >= ent["t1"])
            r = dict(n=int(m.sum()), auc_tp=M.auc(d.tp[m], sc[m], w[m]),
                     ev_all=stats.wmean(base[m], w[m]),
                     high_share=float(w[hi].sum() / w[m].sum()), ev_high=stats.wmean(base[hi], w[hi]),
                     ev_low=stats.wmean(base[lo_], w[lo_]) if lo_.any() else None,
                     ev_keep=stats.wmean(base[keep], w[keep]), keep_share=float(w[keep].sum() / w[m].sum()))
            if vn == "as_frozen":
                lb, hb, pb = stats.boot(base[hi], w[hi])
                r["ev_high_ci_launch"] = (lb, hb)
                lc, hc, pc, k = cluster_boot(base[hi], w[hi], signer[hi])
                r["ev_high_ci_signer"] = (lc, hc)
                r["n_signers_high"] = k
                r["lift_high_ci_signer"] = cluster_boot_diff(base[m], w[m], signer[m], (sc[m] >= ent["t2"]).astype(float))
                r["lift_keep_ci_signer"] = cluster_boot_diff(base[m], w[m], signer[m], (sc[m] >= ent["t1"]).astype(float))
                for sc_ in ("p036", "c010", "lat2"):
                    r[f"ev_high_{sc_}"] = stats.wmean(d.ret[sc_][hi], w[hi])
                    r[f"ev_all_{sc_}"] = stats.wmean(d.ret[sc_][m], w[m])
                r["s0t_high"] = stats.wmean(d.s0t[hi], w[hi])
                r["s0t_low"] = stats.wmean(d.s0t[lo_], w[lo_])
                # concentration: how much of the high-tertile P&L comes from the top signers
                hs = collections.Counter()
                for i in np.nonzero(hi)[0]:
                    hs[signer[i]] += w[i] * base[i]
                tot = sum(hs.values())
                top5 = sum(v for _, v in hs.most_common(5))
                r["pnl_share_top5_signers_high"] = top5 / tot if tot else None
                r["n_high"] = int(hi.sum())
            res[sn] = r
        out["variants"][vn] = res
        a = res["all_post"]
        log(f"{vn:42s} AUC={a['auc_tp']:.3f} high {a['high_share']*100:.0f}% EV={a['ev_high']*100:+.2f} low EV={a['ev_low']*100:+.2f} "
            f"keep {a['keep_share']*100:.0f}% EV={a['ev_keep']*100:+.2f} all={a['ev_all']*100:+.2f} | window high {res['window']['ev_high']*100:+.2f} "
            f"other high {res['other_post']['ev_high']*100:+.2f} (all {res['other_post']['ev_all']*100:+.2f})")
    a = out["variants"]["as_frozen"]["all_post"]
    log("high tertile CI by launch", a["ev_high_ci_launch"], "by signer", a["ev_high_ci_signer"], "signers", a["n_signers_high"],
        "n", a["n_high"], "lift CI signer", a["lift_high_ci_signer"], "skip-bottom lift CI signer", a["lift_keep_ci_signer"],
        "top5 signer pnl share", a["pnl_share_top5_signers_high"])
    log("stress high:", {k: round(v * 100, 2) for k, v in a.items() if k.startswith("ev_high_") and isinstance(v, float)})
    log("stress all:", {k: round(v * 100, 2) for k, v in a.items() if k.startswith("ev_all_") and isinstance(v, float)})
    json.dump(out, open(OUT / "angles_critic_frozen.json", "w"), indent=1, default=float)




# ----------------------------------------------------------------------------- fills on the selected set
G2_ROWS = Path("/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/g2")


def g2_bids():
    """gap-2 bundled launches: first independent slot-0 winner's bid (fee + tips, SOL) by the number of
    independent slot-0 winners. Returns {bucket: sorted bids} with buckets 1 and 2 (= 2 or more)."""
    g = json.load(open(G2_ROWS / "rows.json"))
    link = set(json.load(open(G2_ROWS / "linked_wallets.json")))
    out = {1: [], 2: []}
    for x in g:
        if x["status"] != "ok" or not x["solq"] or x["mayhem"] or not x["bundled"]:
            continue
        r0 = [y for y in x["r0"] if y["buy"] and not y["owner_linked"] and y["signer"] not in link]
        win = {y["signer"] for y in r0 if y["ok"]}
        succ = sorted([y for y in r0 if y["ok"]], key=lambda y: y["i"])
        if succ:
            out[min(2, len(win))].append(succ[0]["bid"] / 1e9)
    return {k: sorted(v) for k, v in out.items()}


def fills():
    from rugsnipe_analysis.strategy import critic as C
    from rugsnipe_analysis.strategy import predictor as P
    doc = json.load(open(W.FROZEN))
    models = pickle.load(open(W.FROZEN_PKL, "rb"))
    ent = next(e for e in doc["rules"] if e["name"].endswith("tp:size_skip"))
    rows, d = W.load(ent["variant"])
    _, _, sc = W.apply_rule(ent, d, models)
    post = (d.bt >= M.OCT12) & ~d.pool
    tapes = {t.mint: t for t in P.load_all()}
    log("tapes", len(tapes))
    bids = g2_bids()
    log("gap-2 first-winner bids: n", {k: len(v) for k, v in bids.items()},
        {k: [round(float(np.quantile(v, p)), 4) for p in (0.5, 0.75, 0.9)] for k, v in bids.items()})
    res = collections.defaultdict(list)
    idx = []
    for i in np.nonzero(post)[0]:
        t = tapes.get(rows[i]["mint"])
        if t is None:
            continue
        r1 = C.lead_trade(t)
        r2 = C.lead_trade(t, rank=2)
        r2c = C.lead_trade(t, rank=2, cap=0.02, fail_cost=0.003)
        rsol = C.lead_trade(t, impact="sol")
        rdf = C.lead_trade(C.drop_first_follower(t))
        real = C.lead_trade(t, prem=0.024, cost=0.01, impact="sol")
        if None in (r1, r2, r2c, rsol, rdf, real):
            continue
        idx.append(i)
        res["r1"].append(r1["r"])
        res["r2"].append(r2["r"])
        res["r2cap"].append(r2c["r"])
        res["r2_prem"].append(r2["prem_tape"])
        res["sol_impact"].append(rsol["r"])
        res["drop_first"].append(rdf["r"])
        res["realistic"].append(real["r"])
    idx = np.array(idx)
    R = {k: np.array(v) for k, v in res.items()}
    log("re-simulated", len(idx), "max |r1 - table base|", float(np.max(np.abs(R["r1"] - d.ret["base"][idx]))))
    w = d.w[idx]
    s = sc[idx]
    s0t = d.s0t[idx]
    # competition-priced fixed cost: to be first we must outbid the fastest follower. Draw the needed bid
    # from gap-2's first-winner bids given the number of followers (0 -> base cost), averaged over draws.
    rng = np.random.default_rng(5)
    need = np.zeros(len(idx))
    for j, k in enumerate(s0t):
        if k >= 1:
            pool = bids[2] if k >= 2 else bids[1]
            need[j] = float(np.mean(rng.choice(pool, 200)))
    cost_bid = np.maximum(0.004, need + 0.001)            # + the exit transaction
    R["bid_priced"] = d.ret["base"][idx] - (cost_bid - 0.004) / 0.5
    # milder: the MEDIAN first-winner bid of the follower bucket (0 followers -> base cost)
    med = {1: float(np.median(bids[1])), 2: float(np.median(bids[2]))}
    cost_med = np.array([max(0.004, (med[2] if k >= 2 else med[1]) + 0.001) if k >= 1 else 0.004 for k in s0t])
    R["bid_median_priced"] = d.ret["base"][idx] - (cost_med - 0.004) / 0.5
    out = {"n": int(len(idx)), "gap2_bid_quantiles": {k: [float(np.quantile(v, p)) for p in (0.25, 0.5, 0.75, 0.9)] for k, v in bids.items()}}
    groups = {"all": np.ones(len(idx), bool), "high": s >= ent["t2"], "mid": (s >= ent["t1"]) & (s < ent["t2"]),
              "low": s < ent["t1"], "keep(skip low)": s >= ent["t1"]}
    for gname, gm in groups.items():
        row = {"n": int(gm.sum()), "s0t": stats.wmean(s0t[gm], w[gm]), "bid_cost_mean": stats.wmean(cost_bid[gm], w[gm])}
        for k, v in R.items():
            row[k] = stats.wmean(v[gm], w[gm])
        out[gname] = row
        log(f"{gname:15s} n={row['n']:4d} followers={row['s0t']:.2f} rank1 {row['r1']*100:+.2f} rank2 {row['r2']*100:+.2f} "
            f"rank2+cap {row['r2cap']*100:+.2f} rank2-prem {row['r2_prem']*100:+.1f}% sol-impact {row['sol_impact']*100:+.2f} "
            f"drop-1st-follower {row['drop_first']*100:+.2f} realistic {row['realistic']*100:+.2f} "
            f"bid-priced {row['bid_priced']*100:+.2f} (mean cost {row['bid_cost_mean']:.4f} SOL) median-bid {row['bid_median_priced']*100:+.2f}")
    json.dump(out, open(OUT / "angles_critic_fills.json", "w"), indent=1, default=float)




# ----------------------------------------------------------------------------- walk-forward winner's curse
def wf():
    """Re-run G1's full walk-forward grid (same code), then estimate how much of the reported best-config EV
    is selection on the test folds: choose the best config on one half of the Oct 12-14 OOS launches
    (alternate folds) and read it on the other half; plus signer-clustered CIs for the reported configs."""
    named = ["mdl:rand:all:gbt_3:tp:keep0.2", "mdl:rand:core:lr_0.5:tp:keep0.2", "mdl:pool:all:lr_0.5:ret:keep0.4",
             "mdl:pool:all:lr_0.5:ret:keep0.6", "mdl:rand:all:gbt_3:tp:keep0.4"]
    out = {"named": {}, "split": []}
    allsel = {}
    for variant in W.VARIANTS:
        rows, d = W.load(variant)
        train_ok = np.ones(len(rows), bool) if variant == "pool" else ~d.pool
        test_ok = ~d.pool
        te_all, configs, preds = W.run_grid(d, train_ok, test_ok, M.FULL_EDGES, variant)
        log(variant, "configs", len(configs))
        allsel[variant] = (rows, d, te_all, configs)
    rows, d, te_all, _ = allsel["rand"]
    post = te_all & (d.bt >= M.OCT12)
    edges = [e for e in M.FULL_EDGES if e >= M.OCT12]
    fold = np.searchsorted(np.array(edges), d.bt, side="right") - 1
    signer = np.array([r["signer"] or r["mint"] for r in rows])
    base, w = d.ret["base"], d.w
    halves = {"A": post & (fold % 2 == 0), "B": post & (fold % 2 == 1)}
    log("Oct 12-14 OOS", int(post.sum()), {k: int(v.sum()) for k, v in halves.items()},
        {k: round(stats.wmean(base[v], w[v]) * 100, 2) for k, v in halves.items()})
    cand = {}
    for variant, (rows_v, d_v, te_v, configs) in allsel.items():
        assert [r["mint"] for r in rows_v] == [r["mint"] for r in rows]
        for name, c in configs.items():
            if "size" in c:
                continue
            cand[name] = c["sel"]
    log("selection configs", len(cand))

    def stat(sel, m):
        k = sel & m
        if k.sum() < 30 or w[k].sum() / w[m].sum() < 0.15:
            return None
        ev = stats.wmean(base[k], w[k])
        sd = math.sqrt(np.sum(w[k] * (base[k] - ev) ** 2) / w[k].sum())
        neff = w[k].sum() ** 2 / np.sum(w[k] ** 2)
        return dict(ev=ev, lo=ev - 1.645 * sd / math.sqrt(neff), n=int(k.sum()), frac=float(w[k].sum() / w[m].sum()))

    for a, b in (("A", "B"), ("B", "A")):
        scored = []
        for name, sel in cand.items():
            s = stat(sel, halves[a])
            if s:
                scored.append((s["lo"], s["ev"], name))
        scored.sort(reverse=True)
        top = scored[:10]
        res = []
        for lo, ev, name in top:
            s2 = stat(cand[name], halves[b]) or {}
            res.append(dict(name=name, pick_half=a, ev_pick=ev, ev_other=s2.get("ev"), frac_other=s2.get("frac"),
                            ev_all_other=stats.wmean(base[halves[b]], w[halves[b]])))
        # also: the mean over the top-1, top-10 picks
        out["split"].append(dict(pick=a, read=b, top10=res))
        log(f"pick on {a}, read on {b}: top-1 {res[0]['name']} {res[0]['ev_pick']*100:+.2f} -> {res[0]['ev_other']*100 if res[0]['ev_other'] is not None else float('nan'):+.2f} "
            f"(all {res[0]['ev_all_other']*100:+.2f}); top-10 mean on {b}: "
            f"{np.nanmean([r['ev_other'] if r['ev_other'] is not None else np.nan for r in res])*100:+.2f}")
    for name in named:
        sel = cand.get(name)
        if sel is None:
            continue
        k = sel & post
        lo, hi, p, ns = cluster_boot(base[k], w[k], signer[k])
        dl, dh, dp = cluster_boot_diff(base[post], w[post], signer[post], sel[post].astype(float))
        r = dict(n=int(k.sum()), frac=float(w[k].sum() / w[post].sum()), ev=stats.wmean(base[k], w[k]),
                 ci_signer=(lo, hi), n_signers=ns, lift=stats.wmean(base[k], w[k]) - stats.wmean(base[post], w[post]),
                 lift_ci_signer=(dl, dh), lift_p_signer=dp,
                 by_half={h: stats.wmean(base[sel & m], w[sel & m]) for h, m in halves.items()},
                 window=stats.wmean(base[k & (d.src == "window")], w[k & (d.src == "window")]),
                 other=stats.wmean(base[k & (d.src != "window")], w[k & (d.src != "window")]),
                 other_all=stats.wmean(base[post & (d.src != "window")], w[post & (d.src != "window")]))
        out["named"][name] = r
        log(f"{name:40s} n={r['n']} keep={r['frac']*100:.0f}% EV={r['ev']*100:+.2f} CI(signer) [{lo*100:+.1f},{hi*100:+.1f}] "
            f"lift {r['lift']*100:+.2f} [{dl*100:+.1f},{dh*100:+.1f}] halves {', '.join(f'{h}={v*100:+.2f}' for h, v in r['by_half'].items())} "
            f"window {r['window']*100:+.2f} other {r['other']*100:+.2f} (other all {r['other_all']*100:+.2f})")
    # BH over every selection config on the Oct 12-14 OOS launches, with signer-clustered p-values for the top 40
    json.dump(out, open(OUT / "angles_critic_wf.json", "w"), indent=1, default=float)


if __name__ == "__main__":
    globals()[sys.argv[1]]()
