"""S5 copy-trading research driver.

Usage (from anywhere):
  .venv/bin/python -I analysis/scripts/strategy/copytrade.py discover   # ledger + wallet tables
  .venv/bin/python -I analysis/scripts/strategy/copytrade.py pull       # top-20 pump wallet histories (RPC)
  .venv/bin/python -I analysis/scripts/strategy/copytrade.py sim        # walk-forward signals + policy sweep
Outputs: data/cache/strategy_copy/*, data/reports/strategy/copytrade_*.json
"""
from __future__ import annotations

import collections
import json
import math
import pickle
import sys
import time
from concurrent.futures import ThreadPoolExecutor

import numpy as np

sys.path.insert(0, "/home/user/rugsnipe/analysis")
from rugsnipe_analysis.strategy import copytrade as ct  # noqa: E402

CACHE = ct.CACHE
REPORTS = ct.REPO / "data" / "reports" / "strategy"
CACHE.mkdir(parents=True, exist_ok=True)
REPORTS.mkdir(parents=True, exist_ok=True)


def tapes():
    p = CACHE / "tapes.pkl"
    if p.exists():
        return pickle.load(open(p, "rb"))
    T = ct.load_tapes()
    pickle.dump(T, open(p, "wb"))
    return T


def role_of(t, u):
    if u in t.C:
        return "C"
    if u in t.B:
        return "B"
    if u in t.funders or u in t.xrecv:
        return "F"
    return "O"


# ============================================================================ discover
def discover():
    T = tapes()
    rows = []          # one row per (wallet, launch)
    for t in T:
        L = ct.ledger(t)
        for u, r in L.items():
            if r["fb"] is None:
                continue
            rows.append({"u": u, "mint": t.mint, "src": t.src, "bt": t.bt, "role": role_of(t, u),
                         "fb_slot": r["fb_slot"], "fb_s": r["fb_s"], "fb_sol": r["fb_sol"], "sin": r["sin"],
                         "sout": r["sout"], "pnl": r["pnl"], "rpnl": r["rpnl"], "hold": r["hold"],
                         "closed": r["closed"], "creator": t.creator})
    json.dump(rows, open(CACHE / "ledger.json", "w"), separators=(",", ":"))
    print("ledger rows", len(rows), "wallets", len({r["u"] for r in rows}))

    # ---- operators: union-find over creator, bundle wallets, creator funder (full sample, descriptive)
    par = {}

    def find(x):
        par.setdefault(x, x)
        while par[x] != x:
            par[x] = par[par[x]]
            x = par[x]
        return x

    for t in T:
        for b in t.B | t.funders:
            ra, rb = find(t.creator), find(b)
            if ra != rb:
                par[ra] = rb
    op_of = {t.mint: find(t.creator) for t in T}
    op_n = collections.Counter(op_of.values())

    # ---- (a) wave wallets: post-slot-0 buyers that recur in >= 2 launches of one operator, are linked
    # (bundle wallet elsewhere in that operator, funder/xfer link) or exclusive (>= 50% of their launches)
    by_u = collections.defaultdict(list)
    for r in rows:
        by_u[r["u"]].append(r)
    mint_t = {t.mint: t for t in T}
    bundle_ops = collections.defaultdict(set)
    for t in T:
        for b in t.B:
            bundle_ops[b].add(op_of[t.mint])
    wave = []
    for u, rs in by_u.items():
        post = [r for r in rs if r["role"] in ("O", "F") and r["fb_slot"] > 0]
        if len(post) < 2:
            continue
        c = collections.Counter(op_of[r["mint"]] for r in post)
        op, k = c.most_common(1)[0]
        if k < 2 or op_n[op] < 3:
            continue
        excl = k / len(rs)
        linked = (op in bundle_ops.get(u, set())) or any(r["role"] == "F" for r in post)
        if not (linked or excl >= 0.5):
            continue
        prs = [r for r in post if op_of[r["mint"]] == op]
        wave.append({"u": u, "op": op, "op_launches": op_n[op], "n": k, "n_all": len(rs), "excl": round(excl, 2),
                     "linked": linked, "med_slot": float(np.median([r["fb_slot"] for r in prs])),
                     "med_sol": float(np.median([r["fb_sol"] for r in prs])),
                     "pnl": float(sum(r["pnl"] for r in prs)), "sin": float(sum(r["sin"] for r in prs)),
                     "med_hold": float(np.median([r["hold"] for r in prs if r["hold"] is not None] or [np.nan])),
                     "srcs": dict(collections.Counter(r["src"] for r in prs))})
    wave.sort(key=lambda r: (-r["n"], -r["excl"]))
    print("wave wallets", len(wave))

    # ---- (b) independent pump wallets: role O, >= 4 launches with first buy < 30 s, positive PnL
    pump = []
    for u, rs in by_u.items():
        ind = [r for r in rs if r["role"] == "O" and r["fb_s"] < 30]
        if len(ind) < 4 or u in bundle_ops:
            continue
        pn = np.array([r["pnl"] for r in ind])
        sin = sum(r["sin"] for r in ind)
        roi = np.array([r["pnl"] / r["sin"] for r in ind if r["sin"] > 0])
        m, se = float(roi.mean()), float(roi.std(ddof=1) / math.sqrt(len(roi))) if len(roi) > 1 else float("nan")
        pump.append({"u": u, "n": len(ind), "n_all": len(rs), "pnl": float(pn.sum()), "sin": float(sin),
                     "roi": float(pn.sum() / sin) if sin else 0.0, "roi_mean": m, "t": m / se if se and se > 0 else 0.0,
                     "win": float((pn > 0).mean()), "med_s": float(np.median([r["fb_s"] for r in ind])),
                     "med_slot": float(np.median([r["fb_slot"] for r in ind])),
                     "med_sol": float(np.median([r["fb_sol"] for r in ind])),
                     "med_hold": float(np.median([r["hold"] for r in ind if r["hold"] is not None] or [np.nan])),
                     "srcs": dict(collections.Counter(r["src"] for r in ind)), "qual": bool(pn.sum() > 0)})
    pump.sort(key=lambda r: -r["t"])
    print("independent early wallets >=4:", len(pump), "positive pnl:", sum(p["qual"] for p in pump))
    json.dump({"wave": wave, "pump": pump, "n_ops": len(op_n), "op_n": op_n.most_common(30)},
              open(REPORTS / "copytrade_wallets.json", "w"), indent=1)
    top = [p for p in pump if p["qual"] and p["n"] >= 4][:20]
    json.dump([p["u"] for p in top], open(CACHE / "top20_pump.json", "w"))
    for p in top:
        print(p["u"][:8], p["n"], round(p["pnl"], 2), round(p["roi"], 3), round(p["t"], 2), p["med_s"], p["med_sol"])


# ============================================================================ pull
TX_CAP = 150


def pull():
    """getSignaturesForAddress (<=1000) + getTransaction for each, decode pump TradeEvents."""
    from rugsnipe_analysis.rpc import RpcClient
    from rugsnipe_analysis.events import extract_events, TradeEvent
    wallets = json.load(open(CACHE / "top20_pump.json"))
    sig_rpc = RpcClient(endpoints=("https://api.mainnet-beta.solana.com",), max_rps=3.0)
    tx_rpc = RpcClient(endpoints=("https://solana-rpc.publicnode.com",), max_rps=12.0)
    out_p = CACHE / "wallet_hist.jsonl"
    done = set()
    if out_p.exists():
        for line in open(out_p):
            done.add(json.loads(line)["wallet"])
    fo = open(out_p, "a")
    t0 = time.time()
    for w in wallets:
        if w in done:
            continue
        try:
            sigs = sig_rpc.call("getSignaturesForAddress", [w, {"limit": 1000}], cache_key=f"copy_sigs1000:{w}")
        except Exception as e:  # noqa: BLE001
            print("sigs fail", w[:8], e, flush=True)
            continue
        ok_all = [s for s in sigs or [] if not s.get("err")]
        step = max(1, len(ok_all) // TX_CAP)
        ok = ok_all[::step][:TX_CAP]          # evenly spaced over the 1000-signature window

        def get(s):
            try:
                tx = tx_rpc.get_transaction(s["signature"])
            except Exception:  # noqa: BLE001
                return None
            if not tx:
                return None
            evs = []
            for ev in extract_events(tx):
                if isinstance(ev, TradeEvent) and ev.user == w:
                    evs.append({"mint": ev.mint, "buy": bool(ev.is_buy), "sol": ev.sol_amount / 1e9,
                                "tok": ev.token_amount / 1e6, "ts": int(ev.timestamp),
                                "vsol": ev.virtual_sol_reserves, "vtok": ev.virtual_token_reserves})
            return {"sig": s["signature"], "slot": s["slot"], "bt": s.get("blockTime"), "ev": evs}

        with ThreadPoolExecutor(12) as ex:
            txs = [r for r in ex.map(get, ok) if r is not None]
        rec = {"wallet": w, "n_sigs": len(sigs or []), "n_ok": len(ok_all), "n_sampled": len(ok), "n_tx": len(txs),
               "t_first": min((s.get("blockTime") or 0) for s in sigs) if sigs else None,
               "t_last": max((s.get("blockTime") or 0) for s in sigs) if sigs else None,
               "trades": [dict(e, sig=r["sig"], slot=r["slot"]) for r in txs for e in r["ev"]]}
        fo.write(json.dumps(rec, separators=(",", ":")) + "\n")
        fo.flush()
        print(time.strftime("%H:%M:%S"), w[:8], rec["n_sigs"], rec["n_tx"], len(rec["trades"]),
              round(time.time() - t0), tx_rpc.stats, flush=True)



# ============================================================================ sim (walk-forward)
LAGS = (1, 2)
SIZES = (0.1, 0.25, 0.5)
COST = 0.004
COST_COMP = 0.01
SB_RULES = ("R1", "R2", "R3", "R4")
SD_RANGES = {"d_lt0.3": (0.0, 0.3), "d_0.3-1": (0.3, 1.0), "d_1-3": (1.0, 3.0), "d_ge3": (3.0, 1e9)}
SA_N = (10, 50, 150)


class UF:
    def __init__(self):
        self.p = {}

    def find(self, x):
        p = self.p
        p.setdefault(x, x)
        while p[x] != x:
            p[x] = p[p[x]]
            x = p[x]
        return x

    def union(self, a, b):
        ra, rb = self.find(a), self.find(b)
        if ra != rb:
            self.p[ra] = rb


def sb_ok(rule, h):
    if h is None or h["n"] < 4:
        return False
    if rule == "R1":
        return h["pnl"] > 0
    if rule == "R2":
        return h["sin"] > 0 and h["pnl"] / h["sin"] >= 0.10
    if rule == "R3":
        return h["nc"] >= 4 and h["cr"] / h["nc"] > 0
    if rule == "R4":
        return h["n"] >= 8 and h["pnl"] > 0 and h["win"] / h["n"] >= 0.5
    return False


def default_copy_ret(t, P, i, u):
    ev = ct.event_paths(t, P, i, u, 1, 0.25)
    if ev is None:
        return None
    val, _ = ct.policy_exit(ev, ("copy",), None, 300)
    return (val - 0.25 - COST) / 0.25


def walk_forward(T):
    """Generate signal events point-in-time. Returns list of events and the per-launch info."""
    hist = {}                    # wallet -> running stats from flushed launches
    known_bundle = set()         # wallets seen as bundle wallets in flushed launches
    seen_any = collections.Counter()   # wallet -> number of flushed launches it bought in
    uf = UF()
    flushed_launches = []        # (creator, wave_buyers dict u->fb_slot, bundle set, xrecv/funder set)
    pending = []                 # (end_time, idx)
    events = []
    info = {}
    copyret_cache = {}
    t_start = time.time()
    for li, t in enumerate(T):
        # ---- flush launches whose data horizon ended before this create
        pending.sort()
        while pending and pending[0][0] < t.bt:
            _, pj = pending.pop(0)
            tp_ = T[pj]
            Lp = led[pj]
            for b in tp_.B:
                uf.union(tp_.creator, b)
                known_bundle.add(b)
            for f in tp_.funders:
                uf.union(tp_.creator, f)
            uf.find(tp_.creator)
            wave_b = {}
            for u, r in Lp.items():
                if r["fb"] is None:
                    continue
                seen_any[u] += 1
                role = role_of(tp_, u)
                if role in ("O", "F") and r["fb_slot"] > 0:
                    wave_b[u] = (r["fb_slot"], role)
                if role == "O" and r["fb_s"] < 30:
                    h = hist.setdefault(u, {"n": 0, "pnl": 0.0, "sin": 0.0, "win": 0, "nc": 0, "cr": 0.0, "sols": []})
                    h["n"] += 1
                    h["pnl"] += r["pnl"]
                    h["sin"] += r["sin"]
                    h["win"] += int(r["pnl"] > 0)
                    h["sols"].append(r["fb_sol"])
                    cr = copyret_cache.get((pj, u))
                    if cr is not None:
                        h["nc"] += 1
                        h["cr"] += cr
            flushed_launches.append((tp_.creator, wave_b, set(tp_.B), set(tp_.funders) | set(tp_.xrecv)))
        P = preps[li]
        L = led[li]
        # ---- point-in-time operator of this launch (creator or slot-0 bundle wallets already known)
        roots = set()
        for x in [t.creator, *t.B]:
            if x in uf.p:
                roots.add(uf.find(x))
        wave_set = {}
        if roots:
            cnt = collections.Counter()
            linked = set()
            n_op = 0
            for (cr_, wb, bs, fs) in flushed_launches:
                if uf.find(cr_) in roots:
                    n_op += 1
                    for u, (fs_, role) in wb.items():
                        cnt[u] += 1
                        if role == "F":
                            linked.add(u)
                    linked |= bs
            for u, k in cnt.items():
                if k >= 2 and (u in linked or k / max(seen_any[u], 1) >= 0.5):
                    wave_set[u] = k
        info[t.mint] = {"op_known": bool(roots), "n_wave": len(wave_set)}
        # ---- first buys in this launch, in tape order
        firsts = sorted(((r["fb"], u, r) for u, r in L.items() if r["fb"] is not None), key=lambda x: x[0])
        ins = t.C | t.B | t.funders | t.xrecv
        fired = {}

        def fire(kind, i, wallets, extra=None):
            if kind in fired:
                return
            fired[kind] = True
            evs = {}
            for lag in LAGS:
                for size in SIZES:
                    evs[(lag, size)] = ct.event_paths(t, P, i, wallets, lag, size)
            events.append({"kind": kind, "li": li, "mint": t.mint, "i": i, "w": wallets, "evs": evs,
                           "sol": float(t.sol[i]), "slot_off": int(t.slot[i] - t.S0), **(extra or {})})

        r1_times = []
        for i, u, r in firsts:
            if u in ins:
                continue
            fb_s = r["fb_s"]
            # S_a: wave wallet of the known operator, buy after slot 0 within N slots
            if u in wave_set and r["fb_slot"] > 0:
                for N in SA_N:
                    if r["fb_slot"] <= N:
                        fire(f"Sa_N{N}", i, u)
            if fb_s >= 30 or u in known_bundle:
                continue
            h = hist.get(u)
            # controls
            if r["fb_slot"] > 0:
                fire("C0_first_indep", i, u)
            if h is not None and h["n"] >= 4:
                fire("C1_active4", i, u)
            for rule in SB_RULES:
                if sb_ok(rule, h):
                    fire(f"Sb_{rule}", i, u)
            if sb_ok("R1", h):
                for nm, (lo, hi) in SD_RANGES.items():
                    if lo <= r["fb_sol"] < hi:
                        fire(f"Sd_{nm}", i, u)
                med = float(np.median(h["sols"]))
                if 0.5 * med <= r["fb_sol"] <= 2 * med:
                    fire("Sd_band", i, u)
                # S_c: a second R1 wallet within 5 s
                tnow = r["fb_slot"] * t.ms / 1000
                for (tw, uw) in r1_times:
                    if uw != u and tnow - tw <= 5:
                        fire("Sc_2in5s", i, [uw, u])
                        break
                r1_times.append((tnow, u))
        # ---- default copy return for every early independent wallet (feeds R3 later)
        for i, u, r in firsts:
            if u in ins or r["fb_s"] >= 30:
                continue
            copyret_cache[(li, u)] = default_copy_ret(t, P, i, u)
        pending.append((t.end_time, li))
        if li % 200 == 0:
            print("wf", li, len(events), round(time.time() - t_start, 1), flush=True)
    return events, info


def pop_weights(T, cut_bt):
    """Per-launch weights proportional to population trades per day (POP) and unit (ALL)."""
    deep = [t for t in T if t.src == "deep"]
    d0, d1 = min(t.bt for t in deep), max(t.bt for t in deep)
    g3_test = [t for t in T if t.src == "gap3" and t.bt >= cut_bt]
    g3_train = [t for t in T if t.src == "gap3" and t.bt < cut_bt]
    D_test = (max(t.bt for t in g3_test) - cut_bt) / 86400
    D_train = (cut_bt - min(t.bt for t in g3_train)) / 86400
    rep_g3 = 781_123 / 9_905 * (ct.CREATES_PER_DAY / (781_123 / 16))     # creates represented, rescaled to 46k/d
    rep_deep = (1215 / 400) * (93_191 / 1_496) * (ct.CREATES_PER_DAY / (93_191 / 2))
    W = {}
    for t in T:
        if t.src == "gap3":
            D = D_test if t.bt >= cut_bt else D_train
            w = t.w * rep_g3 / D
            if d0 <= t.bt <= d1:
                w *= 0.5
        elif t.src == "deep":
            w = rep_deep / 2.0
            if t.gap3cls:
                w *= 0.5
        else:
            w = 0.0
        W[t.mint] = w
    return W, {"D_test": D_test, "D_train": D_train, "deep_window": [d0, d1]}


def exit_grid():
    keys_all = []
    for m in range(16):
        keys_all.append(tuple(k for b, k in enumerate(ct.EXIT_KEYS) if m >> b & 1))
    grid = []
    for keys in keys_all:
        for tp in ct.TPS:
            for H in ct.HOLDS:
                grid.append((keys, tp, H, False))
                if "copy" in keys:
                    grid.append((keys, tp, H, True))
    return grid


def pname(kind, lag, size, keys, tp, H, front):
    ex = "+".join(keys) if keys else "none"
    return f"{kind}|lag{lag}|{size}|{'FRONT:' if front else ''}{ex}|tp{tp}|h{H}"


def sim():
    T = tapes()
    global led, preps
    led = [ct.ledger(t) for t in T]
    preps = [ct.prep(t) for t in T]
    bts = np.array([t.bt for t in T])
    cut_bt = int(np.quantile(bts, 0.60))
    events, info = walk_forward(T)
    print("events", collections.Counter(e["kind"] for e in events))
    W, wmeta = pop_weights(T, cut_bt)
    mint_t = {t.mint: t for t in T}
    grid = exit_grid()
    kinds = sorted({e["kind"] for e in events})
    res = []
    for kind in kinds:
        E = [e for e in events if e["kind"] == kind]
        for lag in LAGS:
            for size in SIZES:
                ev_ok = [(e, e["evs"][(lag, size)]) for e in E if e["evs"][(lag, size)] is not None]
                if not ev_ok:
                    continue
                tt = np.array([mint_t[e["mint"]].bt >= cut_bt for e, _ in ev_ok])
                wp = np.array([W[e["mint"]] for e, _ in ev_ok])
                srcs = np.array([mint_t[e["mint"]].src for e, _ in ev_ok])
                orc = np.array([(ev["oracle"] - size - COST) / size for _, ev in ev_ok])
                prem = np.array([ev["prem"] for _, ev in ev_ok])
                for (keys, tp, H, front) in grid:
                    vals = np.array([ct.policy_exit(ev, keys, tp, H, front)[0] for _, ev in ev_ok])
                    ret = (vals - size - COST) / size
                    retc = (vals - size - COST_COMP) / size
                    row = {"name": pname(kind, lag, size, keys, tp, H, front), "kind": kind, "lag": lag, "size": size,
                           "keys": list(keys), "tp": tp, "hold": H, "front": front}
                    for split, msk in (("train", ~tt), ("test", tt)):
                        for popn, wv in (("pop", wp), ("all", np.ones_like(wp))):
                            m_ = msk & (wv > 0)
                            n = int(m_.sum())
                            key = f"{split}_{popn}"
                            if n == 0:
                                row[key] = {"n": 0}
                                continue
                            mean, se = ct.wmean_se(ret[m_], wv[m_])
                            meanc, _ = ct.wmean_se(retc[m_], wv[m_])
                            hit = float((wv[m_] * (ret[m_] > 0)).sum() / wv[m_].sum())
                            p = ct.norm_sf(mean / se) if se and se > 0 and not math.isnan(se) else (0.5 if mean <= 0 else 0.5)
                            row[key] = {"n": n, "ev": mean, "se": se, "ev_comp": meanc, "hit": hit, "p": p,
                                        "oracle": float((wv[m_] * orc[m_]).sum() / wv[m_].sum()),
                                        "prem_med": float(np.median(prem[m_])),
                                        "nsrc": dict(collections.Counter(srcs[m_].tolist()))}
                    res.append(row)
        print("evaluated", kind, len(res), flush=True)
    # BH on test (pop and all separately), one-sided H0: EV <= 0
    for popn in ("pop", "all"):
        idx = [k for k, r in enumerate(res) if r[f"test_{popn}"].get("n", 0) >= 5]
        q = ct.bh([res[k][f"test_{popn}"]["p"] for k in idx])
        for k, qq in zip(idx, q):
            res[k][f"test_{popn}"]["q"] = qq
    # signal frequency (population trades/day on test, default lag1 0.25)
    freq = {}
    for kind in kinds:
        E = [e for e in events if e["kind"] == kind and mint_t[e["mint"]].bt >= cut_bt]
        g3 = sum(mint_t[e["mint"]].w for e in E if mint_t[e["mint"]].src == "gap3")
        dp_nb = sum(1 for e in E if mint_t[e["mint"]].src == "deep" and not mint_t[e["mint"]].gap3cls)
        dp_b = sum(1 for e in E if mint_t[e["mint"]].src == "deep" and mint_t[e["mint"]].gap3cls)
        rep_g3 = 781_123 / 9_905 * (ct.CREATES_PER_DAY / (781_123 / 16))
        rep_deep = (1215 / 400) * (93_191 / 1_496) * (ct.CREATES_PER_DAY / (93_191 / 2))
        per_day = g3 * rep_g3 / wmeta["D_test"] + dp_nb * rep_deep / 2.0
        freq[kind] = {"n_test_events": len(E), "gap3_w": g3, "deep_nonbundled": dp_nb, "deep_bundled": dp_b,
                      "trades_per_day": per_day,
                      "bundled_class_per_day_from_deep": dp_b * rep_deep / 2.0}
    meta = {"n_tapes": len(T), "src": dict(collections.Counter(t.src for t in T)), "cut_bt": cut_bt,
            "n_train_launches": int((bts < cut_bt).sum()), "n_test_launches": int((bts >= cut_bt).sum()),
            "configs": len(res), "grid_exits": len(grid), "kinds": kinds, "wmeta": wmeta, "freq": freq,
            "op_known_share": float(np.mean([v["op_known"] for v in info.values()])),
            "launches_with_wave": int(sum(1 for v in info.values() if v["n_wave"] > 0))}
    pickle.dump({"events": [{k: v for k, v in e.items() if k != "evs"} for e in events]}, open(CACHE / "events_meta.pkl", "wb"))
    pickle.dump(events, open(CACHE / "events.pkl", "wb"))
    json.dump({"meta": meta, "res": res}, open(REPORTS / "copytrade_sweep.json", "w"))
    print(json.dumps(meta, indent=1, default=str)[:3000])


if __name__ == "__main__":
    stage = sys.argv[1] if len(sys.argv) > 1 else "discover"
    {"discover": discover, "pull": pull, "sim": sim}[stage]()
