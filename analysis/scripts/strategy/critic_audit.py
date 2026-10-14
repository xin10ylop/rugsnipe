"""S4 critic: adversarial audit of the S1 lead and the S3 arming result (no network).

  .venv/bin/python -I analysis/scripts/strategy/critic_audit.py

Sections (all written to data/reports/strategy/critic_audit.json):
  sets        the lead re-priced on every out-of-sample set (sanity: must reproduce S1 / orchestrator)
  fills       rank-r entries (r-1 historical third-party slot-0 buyers ahead of us) with slippage caps and
              failed-attempt costs; gap-2's skewed rank-1 premium; SOL-fixed own-impact model
  gaps        token-flow continuity of every tape in the first 25 s (swap-api paging / ordering)
  regimes     pre / post Oct-9 (400 ms vs 217 ms slots) per set
  mt          multiple testing recomputed from the stored p-values (S1 stage 2, S3 grid)
  s3          S3: armed operators' random-sample vs listing launches, the weighting, and a corrected
              armed-minus-control lift test (armed-side operator-cluster variance included)
"""
from __future__ import annotations

import json
import sys
import time
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
sys.path.insert(0, str(REPO / "analysis" / "scripts" / "strategy"))
from rugsnipe_analysis.strategy import arming as A  # noqa: E402
from rugsnipe_analysis.strategy import critic as C  # noqa: E402
from rugsnipe_analysis.strategy import stats  # noqa: E402
from rugsnipe_analysis.strategy.tapes import build_tape, load_deep, load_gap3  # noqa: E402

OUT = REPO / "data" / "reports" / "strategy" / "critic_audit.json"


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def load_window():
    """Orchestrator's dense window (Oct 14 02:05-08:05 UTC), same loader logic as orchestrator_checks.window."""
    from rugsnipe_analysis import window as win
    from rugsnipe_analysis.pumpapi import PumpApi

    class CacheOnly(PumpApi):
        def get_json(self, url, params=None, cache_key=None):
            if cache_key:
                hit = self.cache.get(cache_key)
                if hit is not None:
                    return hit["v"]
            raise KeyError("not cached")

    api = CacheOnly()
    creates = {}
    for line in open(REPO / "data/cache/window/creates.jsonl"):
        r = json.loads(line)
        creates[r["mint"]] = r
    meta = {}
    for p in ("trades_meta_run1.jsonl", "trades_meta.jsonl"):
        for line in open(REPO / "data/cache/window" / p):
            m = json.loads(line)
            meta[m["mint"]] = m
    out, drop = [], Counter()
    for m in meta.values():
        r = creates.get(m["mint"])
        if not r or r.get("is_mayhem_mode"):
            drop["mayhem/none"] += 1
            continue
        qm = r.get("quote_mint") or ""
        if qm and not qm.startswith("1111"):
            drop["quote"] += 1
            continue
        t0 = r["create_time"]
        try:
            if m.get("mode") == "full_1page":
                raw, comp = api.trades_before(m["mint"], (t0 + 900) * 1000 + 999, max_pages=1)
                res = win._as_result(raw, t0 + 900 if comp else None, 900, "full_1page")
            elif m.get("mode") == "full":
                res = win.early_trades(api, m["mint"], t0, 900, 15)
            else:
                raw, comp = api.trades_before(m["mint"], (t0 + 60) * 1000 + 999, max_pages=2)
                res = win._as_result(raw, t0 + 60 if comp else None, 900, "prefix60")
        except KeyError:
            drop["not_cached"] += 1
            continue
        if not res["complete"]:
            drop["incomplete"] += 1
            continue
        tr = [[t["slot"], t["txi"], t["sub"], t["ts"], t["user"], int(t["buy"]), t["sol"], t["tok"], t["mcap"], t["sig"],
               t["prog"]] for t in res["trades"]]
        tr = [t for t in tr if t[0] >= r["create_slot"]]
        cset = {r["creator"], r["create_user"], r["fee_payer"]}
        t = build_tape(m["mint"], "window", 1.0, r["create_slot"], t0, r["create_sig"], cset, tr,
                       res["covered_until"] - t0, True)
        if t is None:
            drop["not_in_class"] += 1
            continue
        t.mode = m.get("mode")
        out.append(t)
    out.sort(key=lambda t: (t.bt, t.mint))
    return out, dict(drop)


def oos_sets():
    import refine
    from sweep import splits_for
    g3 = load_gap3()
    test = splits_for(g3)["test"]
    win, wdrop = load_window()
    sets = {"gap3_test": [t for t, m in zip(g3, test) if m], "deep": load_deep(), "fresh": refine.load_fresh(),
            "window": win}
    return sets, g3, wdrop


def ev_of(rows, w=None, B=2000, seed=7):
    r = np.array([x["r"] for x in rows])
    w = np.ones(len(r)) if w is None else np.asarray(w, float)
    if len(r) < 2:
        return dict(n=int(len(r)))
    lo, hi, p = stats.boot(r, w, B=B, seed=seed)
    return dict(n=int(len(r)), ev=stats.wmean(r, w), lo=lo, hi=hi, p=p)


def run_set(tapes, **kw):
    rows, ws = [], []
    for t in tapes:
        if t.nB < 1:
            continue
        x = C.lead_trade(t, **kw)
        if x is None:
            continue
        rows.append(x)
        ws.append(t.w)
    return rows, np.array(ws)


def pooled(sets, **kw):
    """Pooled OOS as in lead_check: gap-3 test weights rescaled to mean 1; other sets weight 1."""
    R, W = [], []
    per = {}
    for nm, tapes in sets.items():
        rows, w = run_set(tapes, **kw)
        if not rows:
            continue
        w = w / w.mean()
        per[nm] = ev_of(rows, w, B=500)
        R += rows
        W.append(w)
    W = np.concatenate(W)
    out = ev_of(R, W, B=2000)
    landed = np.array([x["landed"] for x in R])
    out["land_share"] = float(np.sum(W * landed) / W.sum())
    rl = np.array([x["r"] for x in R])
    out["ev_landed"] = float(np.sum((W * rl)[landed]) / W[landed].sum()) if landed.any() else None
    out["per_set"] = per
    return out


def section_fills(sets):
    res = {}
    base = pooled(sets)
    res["base_rank1_prem1.1"] = base
    log("base", round(base["ev"], 4), base["n"])
    # gap-2 rank-1 premium on bundled launches: p25 0.4 / p50 1.6 / p75 4.6 / p90 8.4 % (own impact of a
    # median 0.30 SOL buy included, ~0.8 %). Mean of the piecewise-linear quantile function with p0 = 0
    # and p100 = 15 %: the mean, not the median, enters EV.
    qs = np.array([0, 0.25, 0.5, 0.75, 0.9, 1.0])
    vs = np.array([0.0, 0.004, 0.016, 0.046, 0.084, 0.15])
    u = np.linspace(0, 1, 100001)
    mean_prem = float(np.interp(u, qs, vs).mean())
    res["gap2_bundled_rank1_mean_premium"] = mean_prem
    for nm, prem in (("prem_gap2_mean_minus_own_impact", mean_prem - 0.008), ("prem_gap2_mean", mean_prem)):
        o = pooled(sets, prem=prem)
        res[nm] = dict(prem=prem, **{k: o[k] for k in ("ev", "lo", "hi", "n")})
        log(nm, round(prem, 4), round(o["ev"], 4))
    # rank-r entries from the tape itself (natural premium = the r-1 historical snipers ahead of us)
    for rank in (1, 2, 3):
        for cap in (None, 0.02, 0.05):
            for fc in (0.0, 0.003):
                if cap is None and fc > 0:
                    continue
                key = f"rank{rank}_cap{cap}_fail{fc}"
                o = pooled(sets, rank=rank, prem=0.0 if rank > 1 else 0.011, cap=cap, fail_cost=fc)
                res[key] = {k: o[k] for k in ("ev", "lo", "hi", "n", "land_share", "ev_landed")}
                log(key, {k: (round(v, 4) if isinstance(v, float) else v) for k, v in res[key].items()})
    # E1 with the +2 % cap applied to the S1 premium model: which launches would a +2 % cap reject at rank 2?
    rows, _ = run_set(sum(sets.values(), []), rank=2, prem=0.0)
    pt = np.array([x["prem_tape"] for x in rows])
    rows1, _ = run_set(sum(sets.values(), []))
    r1 = np.array([x["r"] for x in rows1])
    res["rank2_tape_premium_quantiles"] = {str(q): float(np.quantile(pt, q)) for q in (0.25, 0.5, 0.75, 0.9)}
    hot = pt > 0.02
    res["rank1_ev_by_rank2_premium"] = dict(le2pct=dict(n=int((~hot).sum()), ev=float(r1[~hot].mean())),
                                           gt2pct=dict(n=int(hot.sum()), ev=float(r1[hot].mean())))
    log("rank1 EV split by whether a rank-2 entry would be >2% over post-bundle", res["rank1_ev_by_rank2_premium"])
    # own-impact model
    o = pooled(sets, impact="sol")
    res["impact_solfixed"] = {k: o[k] for k in ("ev", "lo", "hi", "n")}
    log("sol-fixed impact", round(o["ev"], 4))
    # the first third-party follower fails to land (gap-2: 27 % of slot-0 attempts on bundled launches fail
    # on slippage, 38 % on bundled repeat-creator launches); and everything at once
    ff = {nm: [C.drop_first_follower(t) for t in tapes] for nm, tapes in sets.items()}
    o_ff = pooled(ff)
    res["first_follower_never_lands"] = {k: o_ff[k] for k in ("ev", "lo", "hi", "n")}
    for pf in (0.27, 0.38):
        res[f"first_follower_fails_p{pf}"] = (1 - pf) * base["ev"] + pf * o_ff["ev"]
    real = dict(prem=mean_prem - 0.008, impact="sol", cost=0.01)
    o_r = pooled(sets, **real)
    o_rf = pooled(ff, **real)
    res["combined_realistic"] = dict(spec="gap-2 mean rank-1 premium net of own impact (+2.4%), SOL-fixed impact, 0.01 SOL "
                                          "per landed round trip, first follower fails with p = 0.33",
                                     ev=0.67 * o_r["ev"] + 0.33 * o_rf["ev"], ev_no_follower_fail=o_r["ev"],
                                     lo_no_follower_fail=o_r["lo"], hi_no_follower_fail=o_r["hi"])
    log("first follower never lands", round(o_ff["ev"], 4), "combined realistic", round(res["combined_realistic"]["ev"], 4))
    # failed attempts at rank 1 (S1 recommendation: cap +2 %, priority-only failures)
    grid = {}
    for land in (1.0, 0.5, 0.3, 0.2, 0.1):
        for fc in (0.0002, 0.003):
            grid[f"land{land}_fail{fc}"] = land * base["ev"] - (1 - land) * fc / 0.5
    res["per_attempt_ev_rank1"] = grid
    return res


def section_gaps(sets, L):
    out = {}
    for nm, tapes in sets.items():
        g = np.array([C.tape_gaps(t) for t in tapes])
        out[nm] = dict(n=int(len(g)), with_gaps=int((g > 0).sum()))
    by = defaultdict(lambda: [0, 0])
    bad_mints = set()
    for x in L:
        k = C.tape_gaps(x.tape)
        by[x.src][0] += 1
        by[x.src][1] += int(k > 0)
        if k > 0:
            bad_mints.add(x.mint)
    out["s3_sources"] = {s: dict(n=v[0], with_gaps=v[1]) for s, v in by.items()}
    return out, bad_mints


def section_regimes(sets):
    out = {}
    for nm, tapes in sets.items():
        for reg in ("pre", "post"):
            rows, w = run_set([t for t in tapes if t.regime == reg])
            if len(rows) >= 2:
                out[f"{nm}_{reg}"] = ev_of(rows, w, B=500)
    return out


def section_mt():
    out = {}
    rr = json.load(open(REPO / "data/reports/strategy/refine_results.json"))
    p = np.array([b["p"] for b in rr["bh"]])
    out["s1_stage2_15tests"] = dict(q_recomputed=stats.bh_qvalues(p).tolist(), q_stored=[b["q"] for b in rr["bh"]])
    fp = np.array([b["p"] for b in rr["bh"] if b["set"] == "fresh"])
    out["s1_fresh_5tests_q"] = stats.bh_qvalues(fp).tolist()
    out["s1_configs_searched"] = 25_397_064 + 659_232
    out["s1_bonferroni_fresh_lead"] = float(min(1, rr["bh"][8]["p"] * 5))
    ow = json.load(open(REPO / "data/reports/strategy/operator_walkforward.json"))
    rows = ow["rows"]
    p = np.array([r["p_oos"] for r in rows])
    q = stats.bh_qvalues(p)
    out["s3_n_configs"] = len(rows)
    out["s3_q_max_abs_diff"] = float(np.max(np.abs(q - np.array([r["q_bh"] for r in rows]))))
    out["s3_q_le_0.05"] = int((q <= 0.05).sum())
    lead_unarmed = [r for r in rows if r["policy"] == "LEAD_0.5" and r["key"] == "none"]
    out["s3_note"] = ("the S3 BH family tests 'EV > 0'; the lead is positive unarmed, so q < 0.05 there does not "
                      "test the arming. The arming hypothesis is the lift over the matched control.")
    out["s3_lead_unarmed_q"] = [dict(cfg=r["cfg"], q=r["q_bh"]) for r in lead_unarmed]
    return out


def section_s3(L):
    import operator_walkforward as OW
    P = OW.POLICIES[0]
    r, tp, _ = A.policy_returns(L, P)
    src = np.array([x.src for x in L])
    g3tr = np.array([x.g3_train for x in L])
    day = np.array([x.day for x in L])
    w = np.array([x.w for x in L])
    oos = (~g3tr) & (day >= 2)
    rand = np.isin(src, ["gap3", "deep", "fresh"])
    out = {}
    for name, cfg in (("creator_EB", A.WFConfig("creator", "EB", True, False, "2d")),
                      ("creator_G100", A.WFConfig("creator", "G100", True, False, "2d")),
                      ("creator_GTP", A.WFConfig("creator", "GTP", True, False, "2d")),
                      ("bcomp_EB", A.WFConfig("bcomp", "EB", True, False, "2d"))):
        wins = A.walk_forward(L, r, tp, cfg)
        armed, op, win = A.flatten(L, wins)
        scor = np.isfinite(r) & oos & (win >= 0)
        m = armed & scor
        rec = dict(n=int(m.sum()), ev=float(np.sum(w[m] * r[m]) / w[m].sum()))
        for nm, mm in (("random_weighted", m & rand), ("listing", m & ~rand)):
            rec[nm] = dict(n=int(mm.sum()), ev=float(np.sum(w[mm] * r[mm]) / w[mm].sum()), ev_unweighted=float(r[mm].mean()))
        u = scor & ~armed
        rec["unarmed_random_weighted"] = float(np.sum(w[u & rand] * r[u & rand]) / w[u & rand].sum())
        rec["unarmed_random_unweighted"] = float(r[u & rand].mean())
        rec["unarmed_listing"] = float(r[u & ~rand].mean())
        # corrected lift test: resample armed operators (cluster bootstrap) AND the matched control draw
        rng = np.random.default_rng(17)
        band = np.array([int(x.me // 5) for x in L])
        hour = np.array([(x.bt // 3600) % 24 for x in L])
        pool_bh, pool_b = defaultdict(list), defaultdict(list)
        for j in np.nonzero(u)[0]:
            pool_bh[(win[j], band[j], hour[j])].append(j)
            pool_b[(win[j], band[j])].append(j)
        ai = np.nonzero(m)[0]
        cands = [np.array(pool_bh.get((win[i], band[i], hour[i])) or pool_b.get((win[i], band[i])) or []) for i in ai]
        ops_u, inv = np.unique(op[ai].astype(str), return_inverse=True)
        diffs = []
        for _ in range(2000):
            pick_ops = rng.integers(0, len(ops_u), len(ops_u))
            cnt = np.bincount(pick_ops, minlength=len(ops_u))
            mult = cnt[inv]
            sel = mult > 0
            a_ev = np.sum(mult * w[ai] * r[ai]) / np.sum(mult * w[ai])
            cw, cr = [], []
            for k, i in enumerate(ai):
                if not sel[k] or not len(cands[k]):
                    continue
                j = cands[k][rng.integers(len(cands[k]))]
                cw.append(mult[k] * w[j]); cr.append(r[j])
            cw, cr = np.array(cw), np.array(cr)
            diffs.append(a_ev - np.sum(cw * cr) / cw.sum())
        diffs = np.array(diffs)
        rec["lift"] = dict(mean=float(diffs.mean()), lo=float(np.quantile(diffs, 0.05)), hi=float(np.quantile(diffs, 0.95)),
                           p=float((np.sum(diffs <= 0) + 1) / (len(diffs) + 1)))
        out[name] = rec
        log("S3", name, json.dumps(rec, default=float)[:400])
    return out


def main():
    T = time.time()
    sets, g3, wdrop = oos_sets()
    res = dict(sets_n={k: len(v) for k, v in sets.items()}, window_drops=wdrop)
    res["sets"] = {nm: ev_of(*run_set(t), B=1000) for nm, t in sets.items()}
    for nm, t in sets.items():
        if nm == "window":
            for mode in sorted({getattr(x, "mode", None) for x in t}):
                res["sets"][f"window_{mode}"] = ev_of(*run_set([x for x in t if getattr(x, "mode", None) == mode]), B=500)
    log("sets", {k: (round(v.get("ev", float("nan")), 4), v["n"]) for k, v in res["sets"].items()})
    res["fills"] = section_fills(sets)
    import pickle
    try:
        L = pickle.load(open(sys.argv[1], "rb")) if len(sys.argv) > 1 else A.load_launches(verbose=False)
    except FileNotFoundError:
        L = A.load_launches(verbose=False)
    res["gaps"], bad = section_gaps(sets, L)
    log("gaps", res["gaps"])
    res["regimes"] = section_regimes(sets)
    log("regimes", {k: (round(v["ev"], 4), v["n"]) for k, v in res["regimes"].items()})
    res["mt"] = section_mt()
    log("mt", {k: v for k, v in res["mt"].items() if k not in ("s1_stage2_15tests",)})
    res["s3"] = section_s3(L)
    res["runtime_s"] = time.time() - T
    json.dump(res, open(OUT, "w"), indent=1, default=float)
    log("written", OUT)


if __name__ == "__main__":
    main()
