"""Quick screen (a): graduation momentum on fresh PumpSwap pools (pre-registered in
data/reports/strategy/screens_preregistration.json). Reads data/cache/screens/grad_trades.jsonl.

Run: .venv/bin/python -I analysis/scripts/strategy/screens_grad.py
"""
from __future__ import annotations

import collections
import json
import sys
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
from rugsnipe_analysis.strategy import screens as sc  # noqa: E402

IN = REPO / "data/cache/screens/grad_trades.jsonl"
OUT = REPO / "data/reports/strategy/screens_results.json"
OPEN_MCAP = 84.99e15 / 2.069e14 * 1e-6 * 1e3  # placeholder, replaced below
OPEN_MCAP = 84.99 / 2.069e8 * 1e9               # 410.8 SOL: price of a freshly migrated pool x 1e9 supply
MS = 230                                          # ms per slot for the horizon (measured 217 on these tapes; 230 is conservative)


def load():
    rows = collections.defaultdict(list)
    for l in open(IN):
        r = json.loads(l)
        rows[r["mint"]].append(r)
    tapes, info = [], collections.Counter()
    for mint, rs in rows.items():
        g = rs[0]
        if g["comp"] and g["comp"].get("mayhem") == 1:
            info["mayhem_excluded"] += 1
            continue
        anchor = g["mig"] or g["comp"]
        reached = [r for r in rs if r["reached"]]
        if not reached:
            info["no_coverage_from_start"] += 1
            continue
        cov = min(reached, key=lambda r: r["t_end"])
        trades = {(t["sig"], t["sub"]): t for t in cov["trades"]}
        end = cov["t_end"]
        for r in sorted(rs, key=lambda r: r["t_end"]):
            if r["t_end"] <= end or not r["trades"]:
                continue
            if min(t["ts"] for t in r["trades"]) <= end:
                trades.update({(t["sig"], t["sub"]): t for t in r["trades"]})
                end = r["t_end"]
        tr = sorted((t for t in trades.values() if t["prog"] == "pump_amm" and t["ts"] <= end and t["mcap"] > 0),
                    key=lambda t: (t["slot"], t["txi"], t["sub"]))
        # validation: the pool must be a standard SOL pool. Coins whose curve or pool trades price below the
        # curve floor (non-SOL quote or non-standard supply) fail the constant-product check against K_AMM.
        ks = []
        for a, b in zip(tr, tr[1:]):
            p0, p1 = a["mcap"] / 1e9, b["mcap"] / 1e9
            if abs(p1 / p0 - 1) >= 0.002 and b["tok"] > 0:
                ks.append((b["tok"] / abs(1 / np.sqrt(p0) - 1 / np.sqrt(p1))) ** 2)
        if (len(ks) >= 3 and abs(np.median(ks) / sc.K_AMM - 1) > 0.10) or (tr and tr[0]["mcap"] < 300):
            info["nonstandard_pool_excluded"] += 1
            continue
        mig_slot = g["mig"]["slot"] if g["mig"] else None
        first_amm = tr[0]["slot"] if tr else None
        hor_slot = anchor["slot"] + int((end - anchor["bt"] - 1) * 1000 / MS)
        tapes.append(dict(mint=mint, bt=anchor["bt"], mig_slot=mig_slot, first_amm=first_amm, end=end, hor_slot=hor_slot,
                          tr=tr, cov_s=end - anchor["bt"]))
    return tapes, info


def to_amm(d, entry_slot):
    """AmmTape with a virtual opening trade at the migrate slot so the entry state always exists."""
    tr = d["tr"]
    sl = [entry_slot] + [t["slot"] for t in tr]
    ts = [d["bt"]] + [t["ts"] for t in tr]
    mc = [OPEN_MCAP] + [t["mcap"] for t in tr]
    buy = [True] + [t["buy"] for t in tr]
    sol = [0.0] + [t["sol"] for t in tr]
    tok = [0.0] + [t["tok"] for t in tr]
    o = np.argsort(np.array(sl), kind="stable")
    A = lambda x, dt=float: np.array(x, dtype=dt)[o]
    return sc.AmmTape(d["mint"], A(sl, np.int64), A(ts, np.int64), A(buy, bool), A(sol), A(tok), A(mc), d["end"], d["hor_slot"])


def k_check(tapes):
    """Implied pool K from consecutive post-trade prices and each trade's own token amount."""
    ks = []
    for d in tapes:
        tr = d["tr"]
        for a, b in zip(tr, tr[1:]):
            p0, p1 = a["mcap"] / 1e9, b["mcap"] / 1e9
            if abs(p1 / p0 - 1) < 0.002 or b["tok"] <= 0:
                continue
            # tokens leave the pool on a buy: B0 - B1 = sqrt(K)(1/sqrt(p0) - 1/sqrt(p1))
            dinv = abs(1 / np.sqrt(p0) - 1 / np.sqrt(p1))
            ks.append((b["tok"] / dinv) ** 2)
    ks = np.array(ks)
    return dict(n=int(ks.size), median=float(np.median(ks)) if ks.size else None,
                p25=float(np.quantile(ks, 0.25)) if ks.size else None, p75=float(np.quantile(ks, 0.75)) if ks.size else None,
                assumed=sc.K_AMM)


def main():
    tapes, info = load()
    tapes.sort(key=lambda d: d["bt"])
    half = len(tapes) // 2
    kc = k_check(tapes)
    print("coins", len(tapes), dict(info), "K check", kc)
    caps = {"slot+5": ("slotexit", 5), "10s": ("hold", 10), "60s": ("hold", 60)}
    grid = [(tp, cap, ent) for ent in ("G_MIG", "G_FIRST") for tp in (0.10, 0.15, 0.30) for cap in caps]
    res = {"coins": len(tapes), "info": dict(info), "k_check": kc, "coverage_s": [d["cov_s"] for d in tapes], "grid": []}
    for tp, cap, ent in grid:
        specs = [("tp", tp), ("idle", 1), ("psl", 1, 0.15), caps[cap]]
        look, test, reasons = [], [], collections.Counter()
        for i, d in enumerate(tapes):
            if ent == "G_MIG":
                if d["mig_slot"] is None:
                    continue
                s0 = d["mig_slot"]
            else:
                if d["first_amm"] is None:
                    continue
                s0 = d["first_amm"]
            t = to_amm(d, d["mig_slot"] or s0)
            e = int(np.searchsorted(t.slot, s0, side="right")) - 1
            r = sc.amm_trade(t, e, s0 + 1, specs)
            if r is None:
                continue
            (look if i < half else test).append(r[0])
            reasons[["tp", "idle", "psl", "cap"][r[1]]] += 1
        row = dict(entry=ent, tp=tp, cap=cap, look=sc.boot(look), test=sc.boot(test), pooled=sc.boot(look + test),
                   reasons=dict(reasons))
        res["grid"].append(row)
    q = sc.bh([r["pooled"]["p"] for r in res["grid"]])
    for r, qq in zip(res["grid"], q):
        r["pooled"]["q"] = float(qq)
        f = lambda b: f"{b['ev']*100:+.1f}% (n={b['n']})" if b["n"] else "n/a"
        print(f"{r['entry']:8s} tp{r['tp']*100:.0f} {r['cap']:6s} look {f(r['look'])} test {f(r['test'])} "
              f"pooled {r['pooled']['ev']*100:+.2f}% [{r['pooled']['lo']*100:+.1f},{r['pooled']['hi']*100:+.1f}] q={qq:.2f} {r['reasons']}")
    # descriptive: price path after migration (mcap at +1, +5, +15 s relative to the opening state)
    paths = collections.defaultdict(list)
    for d in tapes:
        if d["mig_slot"] is None:
            continue
        t = to_amm(d, d["mig_slot"])
        e = int(np.searchsorted(t.slot, d["mig_slot"], side="right")) - 1
        m0 = t.mc[e]
        for sec in (1, 5, 15, 60):
            if d["cov_s"] < sec + 1:
                continue
            k = int(np.searchsorted(t.slot, d["mig_slot"] + int(sec * 1000 / 217), side="right")) - 1
            paths[sec].append(float(t.mc[max(k, e)] / m0 - 1))
    res["path_vs_entry"] = {s: dict(n=len(v), mean=float(np.mean(v)), median=float(np.median(v)), share_up=float(np.mean(np.array(v) > 0)))
                            for s, v in paths.items()}
    print("price vs G_MIG entry state:", res["path_vs_entry"])
    out = json.load(open(OUT)) if OUT.exists() else {}
    out["a_graduation"] = res
    json.dump(out, open(OUT, "w"), indent=1, default=float)


if __name__ == "__main__":
    main()
