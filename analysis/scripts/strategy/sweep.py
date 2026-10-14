"""S1 policy sweep on bundled pump.fun launches (gap-3 tapes + deep-sample external holdout).

Run:  .venv/bin/python -I analysis/scripts/strategy/sweep.py [--quick]

Stage 1 (grid, base scenario: 0.25 SOL, +1.1 % premium, 0.004 SOL/round trip, 1-slot exit latency):
  every entry x exit policy x filter, weighted mean EV and SE on: all / train (first 60 % of launches
  by create time) / test (last 40 %) / even days / odd days.
Stage 2 (pre-registered selection, honest OOS):
  A: best exit per (entry, filter) chosen on TRAIN by lower 90 % bound; top-20 pairs evaluated on TEST
     (bootstrap p, BH over the 20).  B: same, EVEN days -> ODD days.
  C: the top-15 by TEST lower bound over the whole grid, with BH q-values over every configuration
     tried (shows how much ranking on the test set inflates).
Stage 3 (finalists): bootstrap CIs, hit rate, avg win/loss, trades/day, stress grid (premium x cost x
  size x latency), pre/post Oct-9 regimes, deep-sample holdout, oracle bounds per entry.
Outputs: data/reports/strategy/sweep_results.json (+ sim cache in data/cache/strategy/).
"""
from __future__ import annotations

import json
import math
import sys
import time
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
from rugsnipe_analysis.strategy import sim, stats  # noqa: E402
from rugsnipe_analysis.strategy.tapes import load_deep, load_gap3  # noqa: E402

OUT = REPO / "data" / "reports" / "strategy"
CACHE = REPO / "data" / "cache" / "strategy"
OUT.mkdir(parents=True, exist_ok=True)
CACHE.mkdir(parents=True, exist_ok=True)
# population scaling: gap-1 sampled 9,947 of 781,123 create sigs over 16 days; gap-3 weights restore
# the sample counts, so 1 unit of weight = 781123/9947/16 launches per day.
PER_DAY = 781_123 / 9_947 / 16
BASE = sim.Scenario(size=0.25, prem=0.011, cost=0.004, lat=1)
SPLITS = ("all", "train", "test", "even", "odd")
LOOKAHEAD_AT_E1 = {"s0third0", "s0third1+", "s0third2+"}  # third-party slot-0 buys come after a rank-1 entry


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def filters_for(tapes):
    me = np.array([t.me for t in tapes]); nB = np.array([t.nB for t in tapes])
    dev = np.array([t.dev_sol for t in tapes]); post = np.array([t.regime == "post" for t in tapes])
    s3 = np.array([t.s0_third for t in tapes])
    return {
        "all": np.ones(len(tapes), bool),
        "me<=36": me <= 36, "me<=40": me <= 40, "me<=45": me <= 45, "me<=50": me <= 50, "me>50": me > 50,
        "dev_only": nB == 0, "nB1": nB == 1, "nB2-3": (nB >= 2) & (nB <= 3), "nB4+": nB >= 4,
        "dev<1": dev < 1, "dev1-2": (dev >= 1) & (dev < 2), "dev2-4": (dev >= 2) & (dev < 4), "dev4+": dev >= 4,
        "pre": ~post, "post": post,
        "s0third0": s3 == 0, "s0third1+": s3 >= 1, "s0third2+": s3 >= 2,
    }


def splits_for(tapes):
    n = len(tapes)
    order = np.argsort([t.bt for t in tapes], kind="stable")
    train = np.zeros(n, bool); train[order[: int(round(0.6 * n))]] = True
    day = np.array([t.bt // 86400 for t in tapes])
    return {"all": np.ones(n, bool), "train": train, "test": ~train, "even": day % 2 == 0, "odd": day % 2 == 1}


def sim_cached(tapes, entry, scen, tag):
    p = CACHE / f"sim_{tag}_{entry}_{scen.key}.npz"
    if p.exists():
        z = np.load(p)
        return z["F"], z["R"], z["trig"], {k: z[k] for k in ("orc_max", "orc_dump", "ent_mc")}
    F, R, trig, o = sim.simulate_entry(tapes, entry, scen)
    np.savez_compressed(p, F=F, R=R, trig=trig, **o)
    return F, R, trig, o


def grid(tapes, entries, P, FILT, SPL):
    w = np.array([t.w for t in tapes])
    fnames = list(FILT); nf = len(fnames); ns = len(SPLITS)
    Wm = np.stack([w * FILT[f] * SPL[s] for f in fnames for s in SPLITS])   # (nf*ns, n)
    MU = np.full((len(entries), len(P), nf, ns), np.nan, np.float32)
    SE = np.full_like(MU, np.nan)
    CNT = np.zeros((len(entries), nf, ns), np.int32)
    SW = np.zeros((len(entries), nf, ns))
    oracle = {}
    for ei, ent in enumerate(entries):
        t0 = time.time()
        F, R, trig, o = sim_cached(tapes, ent, BASE, "g3")
        Wt = Wm * trig[None, :]
        if ent == "E1":
            for fi, f in enumerate(fnames):
                if f in LOOKAHEAD_AT_E1:
                    Wt[fi * ns:(fi + 1) * ns] = 0
        sw = Wt.sum(1); sw2 = (Wt ** 2).sum(1); cnt = (Wt > 0).sum(1)
        CNT[ei] = cnt.reshape(nf, ns); SW[ei] = sw.reshape(nf, ns)
        tw = (w * trig).sum()
        oracle[ent] = dict(n=int(trig.sum()), per_day=float(tw * PER_DAY),
                           orc_max=float(np.nansum(o["orc_max"] * w) / tw) if tw else None,
                           orc_dump=float(np.nansum(o["orc_dump"] * w) / tw) if tw else None,
                           hold900=float(np.nansum(R[:, sim.CIDX["hold900"]] * w) / tw) if tw else None)
        for c0 in range(0, len(P), 10000):
            Pc = P[c0:c0 + 10000]
            ret = sim.eval_policies(F, R, Pc)
            ret = np.nan_to_num(ret, nan=0.0)
            s1 = Wt @ ret
            with np.errstate(invalid="ignore", divide="ignore"):
                mu = s1 / sw[:, None]
                s2w = (Wt ** 2) @ ret; s2w2 = (Wt ** 2) @ (ret ** 2)
                var = s2w2 - 2 * mu * s2w + mu ** 2 * sw2[:, None]
                corr = np.where(cnt > 1, cnt / np.maximum(cnt - 1, 1), np.nan)[:, None]
                se = np.sqrt(np.maximum(var, 0) * corr) / sw[:, None]
            MU[ei, c0:c0 + len(Pc)] = mu.T.reshape(len(Pc), nf, ns)
            SE[ei, c0:c0 + len(Pc)] = se.T.reshape(len(Pc), nf, ns)
        log(f"grid {ent:12s} trig={int(trig.sum()):4d} {time.time()-t0:.1f}s")
    return MU, SE, CNT, SW, oracle


def detail(tapes, ent, prow, fname, scen, FILT, SPL, tag="g3", boot_B=2000):
    """Full per-split numbers for one configuration."""
    w = np.array([t.w for t in tapes])
    F, R, trig, o = sim_cached(tapes, ent, scen, tag)
    ret = sim.eval_policies(F, R, prow[None, :])[:, 0]
    out = {}
    for s, sm in SPL.items():
        m = trig & FILT[fname] & sm
        r, ww = ret[m], w[m]
        if m.sum() == 0:
            out[s] = dict(n=0); continue
        lo, hi, p = stats.boot(r, ww, B=boot_B)
        win = r > 0
        out[s] = dict(n=int(m.sum()), wsum=float(ww.sum()), ev=stats.wmean(r, ww), lo=lo, hi=hi, p=p,
                      hit=float(np.sum(ww * win) / ww.sum()),
                      avg_win=stats.wmean(r[win], ww[win]) if win.any() else None,
                      avg_loss=stats.wmean(r[~win], ww[~win]) if (~win).any() else None,
                      per_day=float(ww.sum() * PER_DAY * (1 if s == "all" else float(np.sum(w[FILT['all']])) / max(1e-9, float(np.sum(w[sm]))))))
    return out


def main():
    quick = "--quick" in sys.argv
    T0 = time.time()
    tapes = load_gap3(); deep = load_deep()
    log("tapes", len(tapes), "deep holdout", len(deep))
    FILT = filters_for(tapes); SPL = splits_for(tapes)
    FILTd = filters_for(deep); SPLd = {"all": np.ones(len(deep), bool)}
    entries = sim.entry_names()
    P = sim.enumerate_policies()
    if quick:
        entries = entries[:3]; P = P[::50]
    fnames = list(FILT)
    log("entries", len(entries), "exit policies", len(P), "filters", len(fnames))
    MU, SE, CNT, SW, oracle = grid(tapes, entries, P, FILT, SPL)
    si = {s: i for i, s in enumerate(SPLITS)}
    E_, P_, F_ = len(entries), len(P), len(fnames)
    evaluated = CNT[:, :, si["all"]] > 0                                # (E, F)
    m_total = int(evaluated.sum()) * P_
    log("configurations evaluated (entry x exit x filter):", m_total)

    def flat(arr, s):
        return arr[:, :, :, si[s]]

    cnt = lambda s: np.broadcast_to(CNT[:, None, :, si[s]], (E_, P_, F_))
    # ---- p-values on TEST for every configuration (normal approx; n_test >= 10, else p = 1)
    mu_te, se_te = flat(MU, "test"), flat(SE, "test")
    with np.errstate(invalid="ignore", divide="ignore"):
        z = mu_te / se_te
    pte = stats.norm_sf_fast(np.nan_to_num(z, nan=-10.0)).astype(np.float32)
    pte[(cnt("test") < 10) | ~np.isfinite(z)] = 1.0
    pte[~np.broadcast_to(evaluated[:, None, :], pte.shape)] = np.nan
    pv = pte[np.isfinite(pte)]
    q_all = stats.bh_qvalues(pv)
    n_q10 = int(np.sum(q_all <= 0.10)); n_q05 = int(np.sum(q_all <= 0.05))
    bonf = 0.05 / len(pv)
    log(f"BH over {len(pv)} configs: q<=0.10: {n_q10}, q<=0.05: {n_q05}; min p {pv.min():.2e} (Bonferroni 5% needs p<{bonf:.1e})")
    share_pos = {s: float(np.nanmean(flat(MU, s)[np.broadcast_to(evaluated[:, None, :], (E_, P_, F_))] > 0)) for s in SPLITS}

    def pick(sel_split, eval_split, k=20, min_sel=25, min_eval=15):
        mu, se = flat(MU, sel_split), flat(SE, sel_split)
        ok = (cnt(sel_split) >= min_sel) & (cnt(eval_split) >= min_eval)
        score = np.where(ok, mu - 1.645 * se, -np.inf)
        best_p = np.argmax(score, axis=1)                                    # (E, F)
        best_s = np.take_along_axis(score, best_p[:, None, :], 1)[:, 0, :]
        order = np.argsort(-best_s, axis=None)[:k]
        rows = []
        for flat_i in order:
            e, f = np.unravel_index(flat_i, best_s.shape)
            if not np.isfinite(best_s[e, f]):
                continue
            p = best_p[e, f]
            rows.append((int(e), int(p), int(f)))
        return rows

    def test_rank(k=15, min_test=20):
        mu, se = flat(MU, "test"), flat(SE, "test")
        score = np.where(cnt("test") >= min_test, mu - 1.645 * se, -np.inf)
        order = np.argsort(-score, axis=None)[: k * 40]
        rows, seen = [], set()
        for flat_i in order:
            e, p, f = np.unravel_index(flat_i, score.shape)
            rows.append((int(e), int(p), int(f)))
            if len(rows) >= k:
                break
        return rows

    def q_of(e, p, f):
        pval = float(pte[e, p, f])
        rank = int(np.sum(pv <= pval))
        return pval, min(1.0, pval * len(pv) / max(rank, 1))

    sets = {"A_time": pick("train", "test"), "B_evenodd": pick("even", "odd"), "C_testtop": test_rank()}
    # entry-level reference rows: best TRAIN exit for 'all' filter per entry
    ref_rows = []
    for e in range(E_):
        mu, se = flat(MU, "train")[e, :, 0], flat(SE, "train")[e, :, 0]
        ok = (CNT[e, 0, si["train"]] >= 25)
        if ok:
            ref_rows.append((e, int(np.nanargmax(mu - 1.645 * se)), 0))
    sets["entry_best_train_all"] = ref_rows

    results = {"meta": dict(n_tapes=len(tapes), n_deep=len(deep), wsum=float(sum(t.w for t in tapes)),
                            per_day_unit=PER_DAY, base=BASE.__dict__, entries=entries, n_exit_policies=int(P_),
                            filters=fnames, configs_evaluated=m_total, n_pvalues=int(len(pv)),
                            bh_q10=n_q10, bh_q05=n_q05, min_p=float(pv.min()), bonferroni_p=bonf,
                            share_configs_positive=share_pos,
                            split_sizes={s: int(SPL[s].sum()) for s in SPL},
                            components=sim.COMP),
               "oracle": oracle, "sets": {}}
    # ---- stage 3 details
    stress = [sim.Scenario(0.25, p, c, 1) for p in (0.0, 0.011, 0.036, 0.108) for c in (0.004, 0.01)]
    stress += [sim.Scenario(s, 0.011, 0.004, 1) for s in (0.1, 0.5)]
    stress += [sim.Scenario(0.25, 0.011, 0.004, l) for l in (0, 2, 4)]
    for name, rows in sets.items():
        out = []
        for (e, p, f) in rows:
            ent, fname, prow = entries[e], fnames[f], P[p]
            d = detail(tapes, ent, prow, fname, BASE, FILT, SPL)
            pval, qv = q_of(e, p, f)
            rec = dict(entry=ent, exit=sim.policy_name(prow), filter=fname, prow=prow.tolist(), base=d,
                       p_test_normal=pval, q_test_allgrid=qv,
                       grid=dict(train=float(MU[e, p, f, si["train"]]), test=float(MU[e, p, f, si["test"]]),
                                 even=float(MU[e, p, f, si["even"]]), odd=float(MU[e, p, f, si["odd"]])))
            if name in ("A_time", "B_evenodd", "C_testtop") and (len(out) < 10 or name != "C_testtop"):
                rec["stress_test"] = {}
                for sc in stress:
                    dd = detail(tapes, ent, prow, fname, sc, FILT, {"test": SPL["test"], "all": SPL["all"]}, boot_B=400)
                    rec["stress_test"][sc.key] = dict(test=dd["test"].get("ev"), all=dd["all"].get("ev"))
                # regimes
                rec["regime"] = {}
                for reg in ("pre", "post"):
                    m2 = {"all": FILT[reg]}
                    dd = detail(tapes, ent, prow, fname, BASE, FILT, m2, boot_B=1000)
                    rec["regime"][reg] = dd["all"]
                # deep holdout (independent random draw, Oct 12-14)
                dd = detail(deep, ent, prow, fname, BASE, FILTd, SPLd, tag="deep")
                rec["deep"] = dd["all"]
            out.append(rec)
        # BH within the pre-registered set, on bootstrap test p
        if name in ("A_time", "B_evenodd"):
            ev_split = "test" if name == "A_time" else "odd"
            ps = np.array([r["base"][ev_split].get("p", 1.0) if r["base"][ev_split].get("n", 0) else 1.0 for r in out])
            if len(ps):
                qs = stats.bh_qvalues(ps)
                for r, qq in zip(out, qs):
                    r["q_set"] = float(qq)
        results["sets"][name] = out
        log("set", name, len(out))
    json.dump(results, open(OUT / ("sweep_results_quick.json" if quick else "sweep_results.json"), "w"), indent=1, default=float)
    log(f"done in {time.time()-T0:.0f}s")


if __name__ == "__main__":
    main()
