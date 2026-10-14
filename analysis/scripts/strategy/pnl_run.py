"""Task G3 (1): daily P&L of the LEAD and of selection policies, from measured inputs.

Inputs measured here from the tapes (cache only): per-trade EV and SE of the LEAD at 0.25 / 0.5 SOL on
pooled OOS (gap-3 test + deep + fresh) and on the dense window split quiet / busy; the EV sensitivity to the
entry premium; the value of a busy-vs-quiet predictor of given AUC (simulated on the window's own trades).
Fixed inputs (cited): gap-2 attempt outcomes and winning bids, gap-5 failed-buy CU, docs/03 fees, gap-1
create profile, window bundled share, critic audit (rank-2 EV, realistic-fill haircut).

Run: .venv/bin/python -I analysis/scripts/strategy/pnl_run.py   -> data/reports/strategy/pnl_model.json
"""
from __future__ import annotations

import json
import sys
from dataclasses import replace
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
sys.path.insert(0, str(REPO / "analysis/scripts/strategy"))
from rugsnipe_analysis.strategy import pnl_model as pm  # noqa: E402
from rugsnipe_analysis.strategy import screens as sc  # noqa: E402
from statistics import NormalDist  # noqa: E402

OUT = REPO / "data/reports/strategy/pnl_model.json"


def measure():
    import refinements_data as rd
    sets = rd.load()
    res = {}
    for S in (0.25, 0.5):
        for prem in (0.0, 0.011, 0.02, 0.036):
            pooled_x, pooled_w = [], []
            for k in ("test", "deep", "fresh"):
                w = np.array([t.w for t in sets[k]], float)
                w = w / w.mean()
                for t, ww in zip(sets[k], w):
                    lt = sc.lead_trade(t, S=S, prem=prem)
                    if lt:
                        pooled_x.append(lt.ret); pooled_w.append(ww)
            b = sc.boot(pooled_x, pooled_w)
            res[f"pooled_S{S}_p{prem}"] = b
            q = [sc.lead_trade(t, S=S, prem=prem).ret for t in sets["window"] if t.mode == "full_1page"]
            bu = [sc.lead_trade(t, S=S, prem=prem).ret for t in sets["window"] if t.mode == "full"]
            al = [sc.lead_trade(t, S=S, prem=prem).ret for t in sets["window"]]
            res[f"window_quiet_S{S}_p{prem}"] = sc.boot(q)
            res[f"window_busy_S{S}_p{prem}"] = sc.boot(bu)
            res[f"window_all_S{S}_p{prem}"] = sc.boot(al)
            res[f"sd_pooled_S{S}_p{prem}"] = float(np.std(pooled_x))
    # premium slope (EV points per premium point), pooled and window, per size
    for S in (0.25, 0.5):
        xs = np.array([0.0, 0.011, 0.02, 0.036])
        for nm in ("pooled", "window_all", "window_busy", "window_quiet"):
            ys = np.array([res[f"{nm}_S{S}_p{p}"]["ev"] for p in xs])
            res[f"slope_{nm}_S{S}"] = float(-np.polyfit(xs, ys, 1)[0])
    # value of a busy predictor with a given AUC (binormal), top-half / top-third selection on the window
    wr = [(sc.lead_trade(t).ret, t.mode == "full") for t in sets["window"]]
    r = np.array([a for a, _ in wr]); y = np.array([b for _, b in wr], float)
    rng = np.random.default_rng(1)
    pred = {}
    for auc in (0.55, 0.60, 0.65, 0.70, 0.75, 0.85, 0.95, 1.0):
        mu = np.sqrt(2) * NormalDist().inv_cdf(min(auc, 0.9999))
        for frac in (0.5, 0.33):
            evs, bus = [], []
            for _ in range(400):
                s = mu * y + rng.normal(0, 1, y.size) if auc < 1 else y + rng.uniform(0, 1e-6, y.size)
                k = int(round(frac * y.size))
                sel = np.argsort(-s)[:k]
                evs.append(r[sel].mean()); bus.append(y[sel].mean())
            pred[f"auc{auc}_top{frac}"] = dict(ev=float(np.mean(evs)), busy_share=float(np.mean(bus)),
                                              sd_ev=float(np.std(evs)))
    res["predictor"] = pred
    res["window_counts"] = dict(n=int(y.size), busy=int(y.sum()))
    return res


def se_of(b):
    return (b["hi"] - b["lo"]) / (2 * 1.645)


def main():
    m = measure()
    out = {"measured": m, "scenarios": {}, "sensitivity": {}}
    mk = pm.Market()
    print(f"opportunities/day (multi-wallet SOL non-mayhem, window share x 16-day create profile): {mk.opportunities:,.0f}; "
          f"reachable x{mk.reach}")
    pols = {}
    for S in (0.25, 0.5):
        P = m[f"pooled_S{S}_p0.011"]
        Q, B = m[f"window_quiet_S{S}_p0.011"], m[f"window_busy_S{S}_p0.011"]
        sb = m["window_counts"]["busy"] / m["window_counts"]["n"]
        slope = m[f"slope_pooled_S{S}"]
        sd = m[f"sd_pooled_S{S}_p0.011"]
        pols[f"LEAD_{S}_pooledOOS"] = pm.Policy(f"LEAD {S} SOL, pooled OOS EV", S,
                                               [pm.ActivityClass("all", 1.0, P["ev"], se_of(P))], sd, slope)
        pols[f"LEAD_{S}_window"] = pm.Policy(f"LEAD {S} SOL, window classes (quiet/busy)", S,
                                            [pm.ActivityClass("quiet", 1 - sb, Q["ev"], se_of(Q)),
                                             pm.ActivityClass("busy", sb, B["ev"], se_of(B))], sd, slope)
        pols[f"BUSY_oracle_{S}"] = pm.Policy(f"busy third only (perfect label), {S} SOL", S,
                                            [pm.ActivityClass("quiet", 1 - sb, Q["ev"], se_of(Q), traded=False),
                                             pm.ActivityClass("busy", sb, B["ev"], se_of(B))], sd, slope)
        pols[f"BUSY_half9_{S}"] = pm.Policy(f"busy half at +9% (hypothetical, as specified), {S} SOL", S,
                                           [pm.ActivityClass("rest", 0.5, 0.0, 0.0, traded=False),
                                            pm.ActivityClass("selected", 0.5, 0.09 if S == 0.5 else 0.09 - (m["pooled_S0.5_p0.011"]["ev"] - P["ev"]), 0.01)],
                                           sd, slope)
        for auc in (0.65, 0.75):
            pr = m["predictor"][f"auc{auc}_top0.5"]
            # the predictor values are at 0.5 SOL; shift by the size effect measured on the window
            shift = 0.0 if S == 0.5 else m["window_all_S0.25_p0.011"]["ev"] - m["window_all_S0.5_p0.011"]["ev"]
            pols[f"PRED_auc{auc}_{S}"] = pm.Policy(f"top half by a slot-0 predictor of AUC {auc}, {S} SOL", S,
                                                  [pm.ActivityClass("rest", 0.5, 0.0, 0.0, traded=False),
                                                   pm.ActivityClass("selected", 0.5, pr["ev"] + shift, se_of(B))], sd, slope)
    ex0 = pm.Execution()
    for name, pol in pols.items():
        sc_rows = {}
        for L in (0.05, 0.10, 0.20, 0.30, 0.50):
            for cname, ex in (("base", replace(ex0, L1=L)),
                              ("contested_busy", replace(ex0, L1=L)),
                              ("cheap_fail", replace(ex0, L1=L, c_fail=0.0005, c_fail_range=(0.0002, 0.001))),
                              ("competitive", replace(ex0, L1=L, c_rt=0.01, prem=0.02, prem_range=(0.011, 0.036)))):
                p = pol
                if cname == "contested_busy":
                    p = replace(pol, classes=[replace(c, land_mult=0.6) if c.name in ("busy", "selected") else c for c in pol.classes])
                d = pm.daily(p, mk, ex)
                mc = pm.monte_carlo(p, mk, ex, n=4000)
                sc_rows[f"L{int(L*100)}_{cname}"] = dict(**d, **mc, break_even_L1=pm.break_even_L1(p, ex))
        out["scenarios"][name] = sc_rows
        out["sensitivity"][name] = pm.sensitivity(pol, mk, ex0)
        b = sc_rows["L20_base"]
        print(f"{pol.name:58s} L=20%: {b['sol_day']:+7.1f} SOL/day [{b['p05']:+.1f}, {b['p95']:+.1f}] "
              f"landed {b['landed']:.0f}/day, break-even L1 {b['break_even_L1']*100:.1f}%")
    # hours traded (EV per trade does not depend on the hour: screen (c)); rank-2 fills; critic's realistic fills
    extra = {}
    for name in ("LEAD_0.5_pooledOOS", "LEAD_0.25_pooledOOS", "BUSY_oracle_0.5", "PRED_auc0.75_0.5"):
        pol = pols[name]
        rows = {}
        for hn, hrs in (("24h", tuple(range(24))), ("12-24 UTC", tuple(range(12, 24))), ("00-12 UTC", tuple(range(12))),
                        ("02-08 UTC (window hours)", tuple(range(2, 8)))):
            mk2 = pm.Market(hours=hrs)
            d = pm.daily(pol, mk2, replace(ex0, L1=0.2))
            rows[hn] = dict(opportunities=mk2.opportunities, sol_day=d["sol_day"], landed=d["landed"])
        rows["L2=15% rank-2 fills inside the cap"] = pm.daily(pol, mk, replace(ex0, L1=0.2, L2=0.15))
        rows["break_even_L1_with_L2=15%"] = pm.break_even_L1(pol, replace(ex0, L2=0.15))
        if name.startswith("LEAD"):
            real = replace(pol, classes=[replace(c, ev=-0.0107, se=0.008) for c in pol.classes])
            rows["critic combined-realistic fills (EV -1.07%)"] = pm.daily(real, mk, replace(ex0, L1=0.2))
        extra[name] = rows
        print(name, {k: round(v["sol_day"], 1) if isinstance(v, dict) else round(v, 3) for k, v in rows.items()})
    out["extra"] = extra
    out["inputs"] = dict(
        opportunities_per_day=mk.opportunities, reach=mk.reach, multi_share=pm.MULTI_SHARE_WINDOW,
        execution_base=ex0.__dict__,
        sources={
            "gap-2": "slot-0 attempt outcomes 46% won / 22% slippage / 29% guard abort; first-winner bid p50 0.0034 SOL (p75 0.0165, p90 0.068); rank-1 premium +1.1% median, +3.6% CEX-fresh bundled",
            "gap-5": "a failing slippage buy burns ~49k CU; v1 fee is a fixed total, paid on failure; tips inside the tx revert",
            "docs/03": "landed block-0 buy cost median 0.004 SOL, p75 0.014, p90 0.05; bid in the priority fee on the direct-TPU path",
            "critic_audit": "rank 2 inside a +2% cap: 69% land, EV +0.09%; first follower never lands: -0.2%; combined realistic: -1.07%",
            "gap-1 index": "49,325 creates/day, hour-of-day profile",
            "window": "1,069 multi-wallet SOL non-mayhem launches of 8,531 creates in 6 h; 915 complete tapes, 300 busy"})
    json.dump(out, open(OUT, "w"), indent=1, default=float)
    return out


if __name__ == "__main__":
    main()
