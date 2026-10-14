"""Orchestrator's independent checks of the S1 lead policy (docs/04-strategy.md section 3).

1. ``reimpl``: re-implements the lead policy from scratch (own curve math, bundle detection and
   exit logic; nothing imported from strategy/sim.py) on the fresh holdout tapes.
2. ``slot1``: the untested "first buyer in slot+1" entry, with the lead exits, on test / deep / fresh.
3. ``window``: the lead policy on the dense 6-hour window of workflow C (Oct 14 02:05-08:05 UTC,
   every complete multi-wallet bundled tape, cache only), split by tape mode (quiet vs busy launches).

Run: .venv/bin/python -I analysis/scripts/strategy/orchestrator_checks.py [reimpl|slot1|window|all]
"""
from __future__ import annotations

import collections
import json
import math
import random
import sys
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
sys.path.insert(0, str(REPO / "analysis/scripts/strategy"))

LEAD = [("tp", 0.15), ("ins", "bundle"), ("idle", 1), ("psl", 1, 0.15), ("slotexit", 5)]
K = 30.0 * 1.073e9
FEE = 0.0125
FLOOR = 27.959
SPLIT = 454_896_000


# ----------------------------------------------------------------------------- 1. from-scratch
def _vs(m):
    return math.sqrt(K * m / 1e9)


def _tokens_for(S, m):
    vs = _vs(m)
    vt = K / vs
    net = S * 10000 / 10125
    return vt - K / (vs + net)


def _value(m, q):
    vs = _vs(m)
    vt = K / vs
    vta = max(vt - q, vt * 0.05)
    return (K / vta - vs) * (1 - FEE)


def _mcap_for_value(target, q):
    g = target / (1 - FEE)
    vs = (-g * q + math.sqrt(g * g * q * q + 4 * q * g * K)) / (2 * q)
    return vs * vs * 1e9 / K


def reimpl(S=0.5, prem=0.011, cost=0.004, lat=1):
    rows = [json.loads(l) for l in open(REPO / "data/cache/strategy/holdout_trades.jsonl")]
    res, reasons = [], collections.defaultdict(list)
    for r in rows:
        tr = sorted(r["trades"], key=lambda t: (t[0], t[1]))
        if not tr or not r["complete"]:
            continue
        slot0, csig, C = r["slot"], r["sig"], {r["creator"], r["user"], r["fee_payer"]}
        s0 = [t for t in tr if t[0] == slot0]
        ctx = next((t[1] for t in s0 if t[9] == csig), None)
        if ctx is None:
            continue
        idx = set()
        for i, t in enumerate(tr):
            if t[0] != slot0:
                break
            if t[5] and (t[9] == csig or t[4] in C):
                idx.add(i)
        prev = ctx
        for i, t in enumerate(tr):
            if t[0] != slot0 or t[1] <= ctx:
                continue
            if t[1] - prev <= 2 and t[1] - ctx <= 5 and t[5]:
                idx.add(i)
                prev = t[1]
            elif t[1] - prev > 2 or t[1] - ctx > 5:
                break
        if not idx:
            continue
        bt_ = [tr[i] for i in sorted(idx)]
        B = {t[4] for t in bt_ if t[4] not in C}
        dev = sum(t[6] for t in bt_ if t[4] in C)
        bsol = sum(t[6] for t in bt_ if t[4] not in C)
        if any(t[10] != "pump" for t in s0) or dev + bsol >= 80 or sum(t[7] for t in s0 if t[5]) >= 0.79e9 or not B:
            continue
        pb = max(idx)
        mc, last = [], FLOOR
        for t in tr:
            if t[8] and t[8] > 0:
                last = t[8]
            mc.append(last)
        ms = 217 if slot0 >= SPLIT else 400
        hor, bt = r["hor"], r["bt"]
        hend = sum(1 for t in tr if t[3] <= bt + hor)
        e, se = pb, slot0
        if e >= hend:
            continue
        m_e = mc[e]
        q = _tokens_for(S, m_e) / (1 + prem)
        seg = range(e + 1, hend)
        fires = {}
        thr, cm = _mcap_for_value(1.15 * S, q), -1.0
        for i in seg:
            cm = max(cm, mc[i])
            if cm >= thr:
                fires["tp"] = tr[i][0]
                break
        for i in seg:
            if (not tr[i][5]) and tr[i][4] in B:
                fires["ins_b"] = tr[i][0]
                break
        N = math.ceil(1000 / ms)
        seq = [se] + [tr[i][0] for i in range(e + 1, min(hend + 1, len(tr)))]
        f = None
        for a, b in zip(seq, seq[1:]):
            if b - a >= N:
                f = a + N
                break
        if f is None and (hend < len(tr) or r["complete"]):
            f = seq[-1] + N
        if f is not None and f <= slot0 + math.floor(hor * 1000 / ms):
            fires["idle"] = f
        act, thr2 = se + math.ceil(1000 / ms), m_e * 0.85
        before = [i for i in range(hend) if tr[i][0] <= act - 1]
        kk = max(max(before) if before else e, e)
        if kk < hend and mc[kk] <= thr2:
            fires["psl"] = act
        else:
            for i in seg:
                if tr[i][0] >= act and mc[i] <= thr2:
                    fires["psl"] = tr[i][0]
                    break
        fires["slot5"] = se + 5
        reason, fs = min(fires.items(), key=lambda kv: kv[1])
        upto = [i for i in range(hend) if tr[i][0] <= fs + lat]
        fi = max(max(upto) if upto else e, e)
        ret = _value(mc[fi], q) / S - 1 - cost / S
        res.append(ret)
        reasons[reason].append(ret)
    n, mean = len(res), sum(res) / len(res)
    random.seed(1)
    boots = sorted(sum(random.choice(res) for _ in range(n)) / n for _ in range(2000))
    print(f"[reimpl] fresh holdout, from scratch: n={n} EV={mean*100:+.2f}% 90% CI [{boots[100]*100:+.1f}, {boots[1899]*100:+.1f}] "
          f"hit={sum(1 for x in res if x > 0)/n:.2f}")
    for k, v in reasons.items():
        print(f"         {k:6s} share={len(v)/n:.2f} avg={sum(v)/len(v)*100:+.1f}%")


# ----------------------------------------------------------------------------- shared runner
def _run(tapes, entry="E1", S=0.5, prem=0.011, cost=0.004, lat=1):
    from rugsnipe_analysis.strategy import sim
    rs, ws = [], []
    for t in tapes:
        if t.kind != "multi":
            continue
        if entry == "E1":
            e, se = t.pb, t.S0
        else:  # first buyer in slot+1
            idx = np.nonzero(t.slot == t.S0)[0]
            e, se = int(idx[-1]), t.S0 + 1
        if e >= t.hend:
            continue
        m_e = float(t.mc[e])
        q = sim.tokens_for(S, m_e) / (1 + prem)
        f = sim.fire_slots(t, e, se, q, S, LEAD)
        fi = sim.fill_index(t, e, f, lat)
        fill = int(fi[int(np.argmin(f))])
        rs.append(float(sim.value(float(t.mc[fill]), q) / S - 1 - cost / S))
        ws.append(t.w)
    rs, ws = np.array(rs), np.array(ws)
    ev = float((rs * ws).sum() / ws.sum())
    rng = np.random.default_rng(0)
    b = np.sort([(rs[i] * ws[i]).sum() / ws[i].sum() for i in (rng.integers(0, len(rs), len(rs)) for _ in range(2000))])
    return ev, b[100], b[1899], len(rs), float((ws * (rs > 0)).sum() / ws.sum())


def _sets():
    from rugsnipe_analysis.strategy.tapes import load_deep, load_gap3
    import refine
    g3 = load_gap3()
    cut = sorted(t.bt for t in g3)[int(0.6 * len(g3))]
    return [("gap3 test", [t for t in g3 if t.bt >= cut]), ("deep", load_deep()), ("fresh", refine.load_fresh())]


def slot1():
    for name, tp in _sets():
        for entry in ("E1", "S1FIRST"):
            for prem in (0.011, 0.036):
                ev, lo, hi, n, _ = _run(tp, entry, prem=prem)
                print(f"[slot1] {name:9s} {entry:8s} prem={prem:.3f} EV={ev*100:+.2f}% [{lo*100:+.1f},{hi*100:+.1f}] n={n}")


# ----------------------------------------------------------------------------- 3. dense window
def window():
    from rugsnipe_analysis import window as win
    from rugsnipe_analysis.pumpapi import PumpApi
    from rugsnipe_analysis.strategy.tapes import build_tape

    class CacheOnly(PumpApi):
        def get_json(self, url, params=None, cache_key=None):
            if cache_key:
                hit = self.cache.get(cache_key)
                if hit is not None:
                    return hit["v"]
            raise KeyError("not cached")

    api = CacheOnly()
    creates = {}
    for l in open(REPO / "data/cache/window/creates.jsonl"):
        r = json.loads(l)
        creates[r["mint"]] = r
    meta = {}
    for p in ("trades_meta_run1.jsonl", "trades_meta.jsonl"):
        for l in open(REPO / "data/cache/window" / p):
            m = json.loads(l)
            meta[m["mint"]] = m

    def tape_for(m):
        r = creates.get(m["mint"])
        if not r or r.get("is_mayhem_mode"):
            return None
        qm = r.get("quote_mint") or ""
        if qm and not qm.startswith("1111"):
            return None
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
            return None
        if not res["complete"]:
            return None
        tr = [[t["slot"], t["txi"], t["sub"], t["ts"], t["user"], int(t["buy"]), t["sol"], t["tok"], t["mcap"], t["sig"], t["prog"]]
              for t in res["trades"]]
        tr = [t for t in tr if t[0] >= r["create_slot"]]
        C = {r["creator"], r["create_user"], r["fee_payer"]}
        return build_tape(m["mint"], "window", 1.0, r["create_slot"], t0, r["create_sig"], C, tr, res["covered_until"] - t0, True)

    tapes = collections.defaultdict(list)
    for m in meta.values():
        t = tape_for(m)
        if t is not None and t.kind == "multi":
            tapes[m.get("mode")].append(t)
    print("[window] multi-wallet tapes by mode:", {k: len(v) for k, v in tapes.items()})
    for k, v in tapes.items():
        ev, lo, hi, n, hit = _run(v)
        print(f"[window] {k:11s} EV={ev*100:+.2f}% [{lo*100:+.1f},{hi*100:+.1f}] n={n} hit={hit:.2f}")
    allw = [t for v in tapes.values() for t in v]
    for prem, cost in ((0.011, 0.004), (0.011, 0.01), (0.036, 0.004)):
        ev, lo, hi, n, hit = _run(allw, prem=prem, cost=cost)
        print(f"[window] ALL multi prem={prem:.3f} cost={cost:.3f} EV={ev*100:+.2f}% [{lo*100:+.1f},{hi*100:+.1f}] n={n} hit={hit:.2f}")


if __name__ == "__main__":
    what = sys.argv[1] if len(sys.argv) > 1 else "all"
    if what in ("reimpl", "all"):
        reimpl()
    if what in ("slot1", "all"):
        slot1()
    if what in ("window", "all"):
        window()
