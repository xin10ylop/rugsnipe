"""G4 skeptic: score the pre-registered rules (data/reports/strategy/angles_prereg.json) on the fresh holdouts.

  .venv/bin/python -I analysis/scripts/strategy/angles_eval.py funding [rps]   # signer funding as of each create (RPC)
  .venv/bin/python -I analysis/scripts/strategy/angles_eval.py meta            # metadata JSON of each uri (study only)
  .venv/bin/python -I analysis/scripts/strategy/angles_eval.py build           # holdout rows: slot-0 features + labels
  .venv/bin/python -I analysis/scripts/strategy/angles_eval.py eval            # every rule, once -> angles_holdout.json

Sets: HA = critic holdout (Oct 14 08:43-13:43, random + armed groups; only random is scored), HB = the new dense block
(data/cache/strategy_holdout2), HC = copy-trade critic random holdout after Oct 14 08:05. Every row is built exactly as
predictor_build.py builds the table (same feature code); operator history is point in time over the table rows plus
every earlier holdout launch (all groups), outcomes counted 10 s after the launch.
"""
from __future__ import annotations

import bisect
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
from rugsnipe_analysis.strategy import predictor as P  # noqa: E402
from rugsnipe_analysis.strategy import predictor_models as M  # noqa: E402
from rugsnipe_analysis.strategy import stats  # noqa: E402
from rugsnipe_analysis.strategy.tapes import SPLIT_SLOT, build_tape  # noqa: E402

HA = REPO / "data/cache/strategy_holdout"
HB = REPO / "data/cache/strategy_holdout2"
HC = REPO / "data/cache/strategy_holdout_copy"
OUT = REPO / "data/reports/strategy"
ROWS = HB / "rows.jsonl"
TABLE_END = 1791965100          # Oct 14 08:05 UTC: last predictor-table row
RPC = ("https://api.mainnet-beta.solana.com",)
SCEN = {"base": dict(S=0.5), "s025": dict(S=0.25), "s100": dict(S=1.0), "p036": dict(S=0.5, prem=0.036),
        "c010": dict(S=0.5, cost=0.01), "lat2": dict(S=0.5, lat=2)}


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def _recs(d: Path) -> dict:
    out = {}
    for line in open(d / "creates.jsonl"):
        r = json.loads(line)
        if r.get("rec"):
            out[r["rec"]["mint"]] = r["rec"]
    return out


def hb_block() -> tuple[int, int]:
    """HB's dense block [L, H]: every eligible create in it was decoded and prescreened (L >= the prescreen frontier)
    and every multi / unknown launch in it was pulled (walk newest first until the first one not pulled)."""
    import angles_holdout as AH
    pulled = {json.loads(l)["sig"] for l in open(HB / "tapes.jsonl")}
    pre = sorted((json.loads(l) for l in open(HB / "prescreen.jsonl")), key=lambda x: -x["bt"])
    F = AH.frontier()
    L = H = None
    for x in pre:
        if x["multi"] is False:
            continue
        if x["sig"] not in pulled:
            break
        H = H or x["bt"]
        L = x["bt"]
    return max(L, F), H


def load_sets() -> list[dict]:
    """[{set, group, t (Tape), rec}] for every complete multi-wallet tape of HA / HB / HC (deduped by mint)."""
    table = {json.loads(l)["mint"] for l in open(M.TABLE)}
    out, seen = [], set(table)
    hb_lo, hb_hi = hb_block() if (HB / "tapes.jsonl").exists() else (0, 0)
    for name, d, path in (("HA", HA, HA / "tapes.jsonl"), ("HB", HB, HB / "tapes.jsonl"), ("HC", HC, HC / "trades.jsonl")):
        if not path.exists():
            continue
        recs = _recs(d)
        for line in open(path):
            r = json.loads(line)
            if r["mint"] in seen or not r["complete"] or not r["trades"]:
                continue
            if name == "HC" and r["bt"] <= TABLE_END:
                continue
            if name == "HB" and not (hb_lo <= r["bt"] <= hb_hi):
                continue
            t = build_tape(r["mint"], name, 1.0, r["slot"], r["bt"], r["sig"], {r["creator"], r["user"], r["fee_payer"]},
                           r["trades"], r["hor"], True)
            if t is None or t.kind != "multi":
                continue
            seen.add(r["mint"])
            out.append(dict(set=name, group=r.get("group", "dense" if name == "HB" else "random"), t=t,
                            rec=recs.get(r["mint"]), sig=r["sig"], signer=r["user"], creator=r["creator"],
                            fee_payer=r["fee_payer"], pre=r.get("pre")))
    out.sort(key=lambda x: (x["t"].bt, x["t"].mint))
    return out


class _MultiCache:
    def __init__(self):
        from rugsnipe_analysis.cache import DEFAULT_CACHE, DiskCache
        self.cs = [DiskCache(str(d), "rpc") for d in (HB, HA, HC)] + [DiskCache(DEFAULT_CACHE, "rpc")]
        self.rpc = None

    def get(self, key):
        for c in self.cs:
            hit = c.get(key)
            if hit is not None:
                return hit
        if key.startswith("tx:"):
            from rugsnipe_analysis.rpc import RpcClient
            if self.rpc is None:
                self.rpc = RpcClient(endpoints=RPC, max_rps=1.5, cache_root=str(HB))
            v = self.rpc.get_transaction(key[3:])
            return {"v": v} if v else None
        return None


# ----------------------------------------------------------------------------- network stages
def funding():
    from rugsnipe_analysis.rpc import RpcClient
    from rugsnipe_analysis.window import creator_funding
    rps = float(sys.argv[2]) if len(sys.argv) > 2 else 1.5
    rpc = RpcClient(endpoints=RPC, max_rps=rps, cache_root=str(HB))
    p = HB / "funding.jsonl"
    have = set()
    if p.exists():
        have = {json.loads(l)["as_of_sig"] for l in open(p)}
    L = [x for x in load_sets() if x["sig"] not in have and x["group"] != "armed"]
    log("funding todo", len(L))
    with open(p, "a") as fo:
        for i, x in enumerate(L):
            try:
                f = creator_funding(rpc, x["signer"], x["sig"])
            except Exception as e:  # noqa: BLE001
                f = {"status": "error", "err": str(e)[:100]}
            f.update(wallet=x["signer"], as_of_sig=x["sig"])
            fo.write(json.dumps(f) + "\n")
            fo.flush()
            if (i + 1) % 50 == 0:
                log("funding", i + 1, rpc.stats)
    log("funding done", rpc.stats)


def meta():
    import predictor_meta as pm
    from concurrent.futures import ThreadPoolExecutor
    pm.OUTD = HB / "meta"
    pm.OUTD.mkdir(exist_ok=True)
    rc = _MultiCache()
    todo = []
    for x in load_sets():
        cx = P.create_tx_features(x["sig"], rc)
        if cx.get("uri") and cx["uri"].startswith("http"):
            todo.append((x["t"].mint, cx["uri"]))
    log("meta todo", len(todo))
    with ThreadPoolExecutor(8) as ex:
        list(ex.map(lambda a: pm.fetch(*a), todo))
    with ThreadPoolExecutor(4) as ex:
        res = list(ex.map(pm.refetch_ipfs, [m for m, _ in todo]))
    log("meta json ok", sum(1 for d in res if d.get("json") is not None), "of", len(res))


# ----------------------------------------------------------------------------- rows
class _Index:
    """Every successful create (mint-authority signatures) from the holdout indexes, for the create rate."""

    def __init__(self):
        ts = set()
        lo, hi = [], []
        for d in (HA, HB, HC):
            p = d / "index.json"
            if not p.exists():
                continue
            j = json.load(open(p))
            S = j["sigs"]
            for s in S:
                if isinstance(s, dict):
                    if s.get("bt") and not s.get("err"):
                        ts.add((s["sig"], s["bt"]))
                else:
                    if s[2] and not s[3]:
                        ts.add((s[0], s[2]))
            bts = [b for _, b in ts]
        self.ts = np.array(sorted(b for _, b in ts), dtype=np.int64)
        self.lo = int(self.ts[0])

    def rate(self, bt, win):
        if bt - win < self.lo:
            return None
        return float(np.searchsorted(self.ts, bt, side="left") - np.searchsorted(self.ts, bt - win, side="left"))


def build():
    import predictor_build as PB
    P.CACHE = HB                     # meta_features() reads <CACHE>/meta/<mint>.json
    L = load_sets()
    log("launches", len(L), collections.Counter((x["set"], x["group"]) for x in L))
    rc = _MultiCache()
    fund = P.Funding()
    if (HB / "funding.jsonl").exists():
        for line in open(HB / "funding.jsonl"):
            r = json.loads(line)
            if r.get("status") != "error":
                fund.by_asof[r["as_of_sig"]] = r
    cidx = _Index()
    g1s = PB.gap1_signer_times()
    rows = []
    for x in L:
        t, sig, signer = x["t"], x["sig"], x["signer"]
        r = dict(mint=t.mint, src=x["set"], group=x["group"], pool=0, w=1.0, bt=t.bt, slot=t.S0, hor=t.hor,
                 regime="post" if t.S0 >= SPLIT_SLOT else "pre", sig=sig, signer=signer, creator=x["creator"])
        for k, kw in SCEN.items():
            ev = P.lead_eval(t, **kw)
            if not ev:
                break
            r[f"ret_{k}"] = ev["ret"]
            if k == "base":
                r["reason"], r["hold_s"] = ev["reason"], ev["hold_s"]
        if "ret_lat2" not in r:
            continue
        r["tp_hit"] = int(r["reason"] == "tp")
        act = P.activity(t)
        if t.hor < 10:
            act["n10"] = None
        if t.hor < 60:
            act["n60"] = None
        r.update(act)
        r["s0_third"] = t.s0_third
        r.update(P.bundle_features(t))
        cx = P.create_tx_features(sig, rc)
        r.update({k: v for k, v in cx.items() if k not in ("name", "symbol", "uri", "signer")})
        r["uri"], r["name"], r["symbol"] = cx.get("uri"), cx.get("name"), cx.get("symbol")
        r.update(P.text_features(cx.get("name"), cx.get("symbol"), cx.get("uri")))
        r.update(P.meta_features(t.mint))
        r["creator_ne_signer"] = int(bool(cx.get("creator_arg") and signer and cx.get("creator_arg") != signer))
        r["payer_ne_signer"] = int(bool(cx.get("fee_payer") and signer and cx.get("fee_payer") != signer))
        r.update(fund.get(sig, signer, t.bt))
        r["create_rate60"] = cidx.rate(t.bt, 60)
        r["create_rate600"] = cidx.rate(t.bt, 600)
        r["hour"] = int((t.bt // 3600) % 24)
        r["weekday"] = int(((t.bt // 86400) + 4) % 7)
        r["op_g1_prior"] = bisect.bisect_left(g1s.get(signer, []), t.bt)
        if x["pre"] and x["pre"]["reached"]:
            r["pre_s0_fail"], r["pre_s1_fail"], r["pre_s0_ok"] = x["pre"]["n_s0_fail"], x["pre"]["n_s1_fail"], x["pre"]["n_s0_ok"]
        rows.append(r)
    log("rows", len(rows))
    # point-in-time operator history: table rows (op_tab: random rows; op_all: + pools) + earlier holdout rows
    table = M.load_table()
    hist = collections.defaultdict(list)
    for r in table + rows:
        hist[r["signer"]].append(r)
    for r in rows:
        for pre, allow_pool in (("op_tab", False), ("op_all", True)):
            prev = [p for p in hist[r["signer"]] if p["bt"] < r["bt"] and (allow_pool or not p["pool"])]
            lead_known = [p for p in prev if p["bt"] <= r["bt"] - 10]
            busy_known = [p for p in prev if p["bt"] <= r["bt"] - 900 and p.get("busy900") is not None]
            r[f"{pre}_prior"] = len(prev)
            r[f"{pre}_lead_n"] = len(lead_known)
            r[f"{pre}_lead_hit"] = float(np.mean([p["tp_hit"] for p in lead_known])) if lead_known else None
            r[f"{pre}_lead_ev"] = float(np.mean([p["ret_base"] for p in lead_known])) if lead_known else None
            r[f"{pre}_busy"] = float(np.mean([p["busy900"] for p in busy_known])) if busy_known else None
            r[f"{pre}_s0t"] = float(np.mean([p["s0_third"] for p in lead_known])) if lead_known else None
    with open(ROWS, "w") as f:
        for r in rows:
            f.write(json.dumps(r) + "\n")
    log("wrote", ROWS, collections.Counter((r["src"], r["group"]) for r in rows))


# ----------------------------------------------------------------------------- evaluation
def cluster_p(diff_fn, n, g, B=4000, seed=21):
    """One-sided bootstrap p (statistic <= 0) and 90% CI, resampling clusters g; diff_fn(index array) -> statistic."""
    ug, inv = np.unique(g, return_inverse=True)
    members = [np.nonzero(inv == k)[0] for k in range(len(ug))]
    rng = np.random.default_rng(seed)
    vals = []
    for _ in range(B):
        pick = rng.integers(0, len(ug), len(ug))
        ix = np.concatenate([members[k] for k in pick])
        v = diff_fn(ix)
        if v == v:
            vals.append(v)
    vals = np.array(vals)
    return float(np.quantile(vals, 0.05)), float(np.quantile(vals, 0.95)), float((np.sum(vals <= 0) + 1) / (len(vals) + 1))


def sel_block(ret, sel, g, extra=None):
    """Selection rule: kept EV, all EV, lift with signer-clustered CI / p."""
    n = len(ret)
    if sel.sum() < 3:
        return dict(n=int(n), kept=int(sel.sum()))
    ev_k, ev_a = float(ret[sel].mean()), float(ret.mean())
    lo, hi, p = cluster_p(lambda ix: (ret[ix][sel[ix]].mean() - ret[ix].mean()) if sel[ix].sum() else float("nan"), n, g)
    klo, khi, kp = cluster_p(lambda ix: ret[ix][sel[ix]].mean() if sel[ix].sum() else float("nan"), n, g, seed=22)
    out = dict(n=int(n), kept=int(sel.sum()), keep=float(sel.mean()), ev_kept=ev_k, ev_all=ev_a,
               ev_dropped=float(ret[~sel].mean()) if (~sel).any() else None, lift=ev_k - ev_a, lift_ci=(lo, hi), p_lift=p,
               ev_kept_ci=(klo, khi), p_kept_pos=kp)
    if extra:
        for k, v in extra.items():
            out[f"{k}_kept"] = float(np.nanmean(v[sel]))
            out[f"{k}_all"] = float(np.nanmean(v))
    return out


def pair_block(var, lead, g):
    d = var - lead
    lo, hi, p = cluster_p(lambda ix: d[ix].mean(), len(d), g)
    return dict(n=int(len(d)), ev=float(var.mean()), ev_lead=float(lead.mean()), diff=float(d.mean()), diff_ci=(lo, hi), p_diff=p,
                changed=int(np.sum(np.abs(d) > 1e-12)))


def apply(ent, d, models):
    import predictor_wf as W
    sel, sz, score = W.apply_rule(ent, d, models)
    return sel, score


def refit_on(extra_rows, ent):
    """Refit a pre-registered config on the table + extra rows (armed / non-random rows count as pool rows)."""
    import predictor_wf as W
    _, variant, fs, kind, target, pol = ent["name"].split(":")
    rows = M.load_table() + extra_rows
    rows.sort(key=lambda r: (r["bt"], r["mint"]))
    d = M.make_data(rows, W.VARIANTS[variant])
    tr = np.ones(len(rows), bool) if variant == "pool" else ~d.pool
    rnd = tr & ~d.pool
    cj = np.array([d.names.index(n) for n in ent["features"]])
    y, mt = W.target_y(d, target)
    trm = tr & ~np.isnan(y)
    _, ptr, model = M.fit_predict(kind, mt, d.X[trm][:, cj], y[trm], d.w[trm], d.X[trm][:1, cj])
    e2 = dict(ent, threshold=W.wquant(ptr[rnd[trm]], d.w[rnd], 1 - float(pol[4:])), n_train=int(trm.sum()))
    return e2, {ent["name"]: model}


def evaluate():
    import predictor_wf as W
    import refinements as RF
    from rugsnipe_analysis.strategy import critic as C
    from rugsnipe_analysis.strategy import pnl_model as PM
    from rugsnipe_analysis.strategy import postentry as pe
    from rugsnipe_analysis.strategy import sim
    pre = json.load(open(OUT / "angles_prereg.json"))
    pkl = (OUT / "angles_prereg_models.pkl").read_bytes()
    assert hashlib.sha256(pkl).hexdigest() == pre["models_sha256"]
    rt_models = pickle.loads(pkl)
    g1 = json.load(open(W.FROZEN))
    g1pkl = W.FROZEN_PKL.read_bytes()
    assert hashlib.sha256(g1pkl).hexdigest() == g1["models_sha256"]
    g1m = pickle.loads(g1pkl)
    g2 = json.load(open(OUT / "frozen_refinements.json"))
    rows_all = [json.loads(l) for l in open(ROWS)]
    for r in rows_all:
        r["pool"] = int(r["group"] == "armed")          # operator-selected rows: history only, never scored
    rows = [r for r in rows_all if r["group"] != "armed"]
    log("scored rows", collections.Counter(r["src"] for r in rows))
    dA, dT = M.make_data(rows, "op_all"), M.make_data(rows, "op_tab")
    E = {e["name"]: e for e in g1["rules"]}
    r3 = next(e for e in g1["rules"] if e["name"].endswith("tp:size_skip"))
    _, s3 = apply(r3, dA, g1m)
    S = {}
    S["G1-R3-high"] = s3 >= r3["t2"]
    S["G1-R3-skip"] = s3 >= r3["t1"]
    rt = {e["name"]: e for e in pre["retrained"]}
    e_g = rt["mdl:rand:all:gbt_3:tp:keep0.2"]
    e_r = rt["mdl:pool:all:lr_0.5:ret:keep0.4"]
    S["G1-RT-gbt3-tp-top20"], sc_g = apply(e_g, dT, rt_models)
    S["G1-RT-ridge-ret-top40"], sc_r = apply(e_r, dA, rt_models)
    S["sec:G1-R1-txindex"], _ = apply(E["rand:uni:tx_index:le:q0.25"], dT, g1m)
    S["sec:G1-R2-n10gbt"], _ = apply(E["mdl:pool:core:gbt_2:n10:keep0.4"], dA, g1m)
    S["sec:G1-uri-uxento"] = dT.X[:, dT.names.index("host_uxento")] > 0
    # live-feasible variant of R3 (no metadata JSON, no CU used): diagnostic
    Xl = dA.X.copy()
    for n in dA.names:
        if n.startswith("m_") or n == "meta_ok":
            Xl[:, dA.names.index(n)] = 0.0
    dl = M.Data(rows=rows, X=Xl, names=dA.names, bt=dA.bt, w=dA.w)
    _, s3l = apply(r3, dl, g1m)
    S["sec:G1-R3-high-nometa"] = s3l >= r3["t2"]
    # HB walk-forward step: refit both retrained configs on table + HA (+ HC) rows, score HB only
    early = [dict(r) for r in rows_all if r["src"] in ("HA", "HC")]
    hb = np.array([r["src"] == "HB" for r in rows])
    if hb.any() and early:
        for key, e, d_ in (("sec:HB-WF-gbt3-tp-top20", e_g, dT), ("sec:HB-WF-ridge-ret-top40", e_r, dA)):
            e2, m2 = refit_on(early, e)
            sel, _ = apply(e2, d_, m2)
            S[key] = np.where(hb, sel, False)
            S[key + ":mask"] = hb
    # per-launch outcomes
    tapes = {x["t"].mint: x["t"] for x in load_sets()}
    T = [tapes[r["mint"]] for r in rows]
    base = dA.ret["base"]
    g = np.array([r["signer"] for r in rows])
    src = np.array([r["src"] for r in rows])
    s0t = dA.s0t
    def _r(t, **kw):
        o = C.lead_trade(t, **kw)
        return o["r"] if o else float("nan")

    rank2 = np.array([_r(t, rank=2) for t in T])
    rank2cap = np.array([_r(t, rank=2, cap=0.02, fail_cost=0.003) for t in T])
    comp = 1.0 / (1.0 + dA.s0t)          # crude chance of beating k equally fast followers to rank 1
    import angles_critic as AC
    bids = AC.g2_bids()
    med = {1: float(np.median(bids[1])), 2: float(np.median(bids[2]))}
    cost_med = np.array([max(0.004, (med[2] if k >= 2 else med[1]) + 0.001) if k >= 1 else 0.004 for k in s0t])
    bidp = base - (cost_med - 0.004) / 0.5
    extra = dict(p036=dA.ret["p036"], c010=dA.ret["c010"], lat2=dA.ret["lat2"], rank2=rank2, rank2cap=rank2cap,
                 bid_median=bidp, tp=dA.tp, followers=s0t, comp=comp,
                 s0_fail=np.array([r.get("pre_s0_fail", np.nan) if r.get("pre_s0_fail") is not None else np.nan for r in rows], float),
                 s1_fail=np.array([r.get("pre_s1_fail", np.nan) if r.get("pre_s1_fail") is not None else np.nan for r in rows], float))
    # G2 variants
    V = [pe.TV(t, 1) for t in T]
    thr = {k: tuple(v) for k, v in g2["ladder_thresholds"].items()}
    fz = {f["fam"]: f["cfg"] for f in g2["frozen"]}
    for f in fz.values():
        p_ = f["p"]
        if "trail" in p_ and isinstance(p_["trail"], list):
            p_["trail"] = tuple(p_["trail"])
        if "loose" in p_ and isinstance(p_["loose"], dict) and isinstance(p_["loose"].get("trail"), list):
            p_["loose"]["trail"] = tuple(p_["loose"]["trail"])
        if "extra" in p_:
            p_["extra"] = tuple(p_["extra"])
    lead_pe = np.array([pe.run_lead(v).pnl / 0.5 for v in V])
    log("LEAD engine check, max |postentry - sim|", float(np.max(np.abs(lead_pe - base))))
    VAR = {}
    for fam in ("gate", "reentry", "scaleout", "flow"):
        VAR[fam] = np.array([RF.run_cfg(v, fz[fam], RF.BASE, thr)[0] / 0.5 for v in V])
    lad = [RF.run_cfg(v, fz["ladder"], RF.BASE, thr) for v in V]
    lad_pnl = np.array([a for a, b, c in lad])
    lad_stake = np.array([b for a, b, c in lad])
    Sflat = float(lad_stake.mean())
    flat = np.array([pe.run_lead(v, Sflat).pnl for v in V])

    def time_exit(v, j, S_=0.5):
        q = sim.tokens_for(S_, v.base) / (1 + 0.011)
        return (pe.sell_value(v.fill_mc(v.se + j), q, q) - S_ - 0.004) / S_

    k0 = np.array([v.third_buys_by(v.se) for v in V])
    te1 = np.array([time_exit(v, 0) for v in V])
    VAR["k0-bail"] = np.where(k0 == 0, te1, lead_pe)
    VAR["slot1-always"] = te1
    sets = {"HA": src == "HA", "HB": src == "HB", "HC": src == "HC", "HA+HB": (src == "HA") | (src == "HB"),
            "pooled": np.ones(len(rows), bool)}
    res = {"n": {k: int(v.sum()) for k, v in sets.items()}, "lead": {}, "select": {}, "refine": {}, "volume": {}}
    for sn, m in sets.items():
        if m.sum() < 5:
            continue
        lo, hi, p = cluster_p(lambda ix: base[m][ix].mean(), int(m.sum()), g[m])
        res["lead"][sn] = dict(n=int(m.sum()), ev=float(base[m].mean()), ci=(lo, hi), p_le0=p,
                               **{k: float(np.nanmean(v[m])) for k, v in extra.items()})
        log(f"LEAD {sn:7s} n={m.sum():4d} EV={base[m].mean()*100:+.2f} [{lo*100:+.1f},{hi*100:+.1f}] p={p:.3f} "
            f"rank2 {np.nanmean(rank2[m])*100:+.2f} p036 {extra['p036'][m].mean()*100:+.2f} bid-med {bidp[m].mean()*100:+.2f} tp {dA.tp[m].mean():.2f}")
    for name, sel in S.items():
        if name.endswith(":mask"):
            continue
        mask = S.get(name + ":mask", np.ones(len(rows), bool))
        res["select"][name] = {}
        for sn, m in sets.items():
            mm = m & mask
            if mm.sum() < 10:
                continue
            b = sel_block(base[mm], sel[mm], g[mm], {k: v[mm] for k, v in extra.items()})
            res["select"][name][sn] = b
        b = res["select"][name].get("pooled") or res["select"][name].get("HB")
        if b and "lift" in b:
            log(f"{name:28s} pooled keep {b['keep']*100:4.0f}% ({b['kept']}) EV kept {b['ev_kept']*100:+.2f} vs all {b['ev_all']*100:+.2f} "
                f"lift {b['lift']*100:+.2f} [{b['lift_ci'][0]*100:+.1f},{b['lift_ci'][1]*100:+.1f}] p={b['p_lift']:.3f} | "
                + " ".join(f"{sn}:{res['select'][name][sn]['lift']*100:+.1f}({res['select'][name][sn]['kept']})" for sn in ("HA", "HB", "HC")
                           if sn in res["select"][name] and "lift" in res["select"][name][sn]))
    for name, var in VAR.items():
        res["refine"][name] = {}
        for sn, m in sets.items():
            if m.sum() < 10:
                continue
            res["refine"][name][sn] = pair_block(var[m], base[m], g[m])
        b = res["refine"][name]["pooled"]
        log(f"G2 {name:14s} pooled EV {b['ev']*100:+.2f} vs LEAD {b['ev_lead']*100:+.2f} diff {b['diff']*100:+.2f} "
            f"[{b['diff_ci'][0]*100:+.2f},{b['diff_ci'][1]*100:+.2f}] p={b['p_diff']:.3f} changed {b['changed']}")
    # ladder: SOL per SOL deployed vs a flat stake of the same mean size
    res["refine"]["ladder"] = {}
    for sn, m in sets.items():
        if m.sum() < 10:
            continue
        per_sol = lad_pnl[m].sum() / lad_stake[m].sum()
        per_sol_flat = flat[m].sum() / (Sflat * m.sum())
        lo, hi, p = cluster_p(lambda ix: lad_pnl[m][ix].sum() / lad_stake[m][ix].sum() - flat[m][ix].sum() / (Sflat * len(ix)),
                              int(m.sum()), g[m])
        res["refine"]["ladder"][sn] = dict(n=int(m.sum()), ev=float(per_sol), ev_flat=float(per_sol_flat), diff=float(per_sol - per_sol_flat),
                                           diff_ci=(lo, hi), p_diff=p, flat_size=Sflat)
    res["refine"]["ladder"]["sol_per_launch"] = dict(ladder=float(lad_pnl.mean()), lead=float(0.5 * base.mean()), flat=float(flat.mean()))
    # survivorship probe on HB: launches whose first 10 s did not fit in one page (> 100 trades) vs the rest
    n10 = dA.n10
    hb_ = src == "HB"
    hot = hb_ & (np.isnan(n10) | (n10 > 100))
    res["hb_hot"] = dict(n_hot=int(hot.sum()), ev_hot=float(base[hot].mean()) if hot.any() else None,
                         n_rest=int((hb_ & ~hot).sum()), ev_rest=float(base[hb_ & ~hot].mean()) if (hb_ & ~hot).any() else None)
    log("HB hot (>100 trades in 10 s)", res["hb_hot"])
    b = res["refine"]["ladder"]["pooled"]
    log(f"G2 ladder per SOL {b['ev']*100:+.2f} vs flat {Sflat:.3f} SOL {b['ev_flat']*100:+.2f} diff {b['diff']*100:+.2f} p={b['p_diff']:.3f}")
    # score skill: TP AUC of the frozen and refitted scores, with / without operator history
    hist = dA.X[:, dA.names.index("op_has_hist")] > 0
    res["auc"] = {}
    for nm, sc in (("G1-R3", s3), ("G1-RT-gbt3-tp", sc_g), ("G1-RT-ridge-ret", sc_r), ("G1-R3-nometa", s3l)):
        res["auc"][nm] = {}
        for sn, m in sets.items():
            if m.sum() < 20:
                continue
            res["auc"][nm][sn] = dict(all=M.auc(dA.tp[m], sc[m]), hist=M.auc(dA.tp[m & hist], sc[m & hist]) if (m & hist).sum() > 10 else None,
                                      nohist=M.auc(dA.tp[m & ~hist], sc[m & ~hist]) if (m & ~hist).sum() > 10 else None,
                                      n_hist=int((m & hist).sum()), pos=M.auc((base[m] > 0).astype(float), sc[m]))
        a = res["auc"][nm]["pooled"]
        log(f"AUC TP {nm:16s} pooled {a['all']:.3f} (with history {a['hist']} n={a['n_hist']}, first-seen {a['nohist']}) "
            + " ".join(f"{sn}:{res['auc'][nm][sn]['all']:.2f}" for sn in ("HA", "HB", "HC") if sn in res["auc"][nm]))
    # EV by frozen tertile per set
    res["tertiles"] = {}
    for sn, m in sets.items():
        lo_, mid_, hi_ = m & (s3 < r3["t1"]), m & (s3 >= r3["t1"]) & (s3 < r3["t2"]), m & (s3 >= r3["t2"])
        res["tertiles"][sn] = {k: dict(n=int(v.sum()), share=float(v.sum() / max(1, m.sum())), ev=float(base[v].mean()) if v.any() else None,
                                       tp=float(dA.tp[v].mean()) if v.any() else None, followers=float(s0t[v].mean()) if v.any() else None)
                               for k, v in (("low", lo_), ("mid", mid_), ("high", hi_))}
        log("tertiles", sn, {k: (v["n"], round((v["ev"] or 0) * 100, 2)) for k, v in res["tertiles"][sn].items()})
    # BH over the primary family (pooled)
    prim = [("G1-R3-high", "select"), ("G1-R3-skip", "select"), ("G1-RT-gbt3-tp-top20", "select"), ("G1-RT-ridge-ret-top40", "select"),
            ("gate", "refine"), ("reentry", "refine"), ("ladder", "refine"), ("k0-bail", "refine")]
    ps = []
    for k, kind in prim:
        b = res[kind][k]["pooled"]
        ps.append(b.get("p_lift", b.get("p_diff", 1.0)))
    qs = stats.bh_qvalues(np.array(ps))
    res["primary_bh"] = [dict(rule=k, p=float(p_), q=float(q_)) for (k, _), p_, q_ in zip(prim, ps, qs)]
    log("BH primary:", [(k, round(p_, 3), round(q_, 3)) for (k, _), p_, q_ in zip(prim, ps, qs)])
    # volume and SOL/day at a 20% landing rate (pnl_model accounting; opportunities 6,181/day and 3,750/day)
    mk = PM.Market()
    for name, sel in list(S.items()) + [("LEAD", np.ones(len(rows), bool))]:
        if name.endswith(":mask") or name.startswith("sec:HB-WF"):
            continue
        m = sel
        f = float(m.mean())
        ev = float(base[m].mean()) if m.any() else float("nan")
        se = float(base[m].std() / math.sqrt(max(1, m.sum())))
        pol = PM.Policy(name, 0.5, [PM.ActivityClass("kept", f, ev, se), PM.ActivityClass("dropped", 1 - f, 0.0, 0.0, traded=False)])
        lm = float(comp[m].mean() / comp.mean())
        polc = PM.Policy(name, 0.5, [PM.ActivityClass("kept", f, ev, se, land_mult=lm), PM.ActivityClass("dropped", 1 - f, 0.0, 0.0, traded=False)])
        out = {"land_mult_contested": lm, "sol_day_L20_contested": PM.daily(polc, mk, PM.Execution(L1=0.2))["sol_day"]}
        for L1 in (0.1, 0.2, 0.5):
            dd = PM.daily(pol, mk, PM.Execution(L1=L1))
            out[f"L{int(L1*100)}"] = dict(trades_day=dd["landed"], attempts_day=dd["attempts"], sol_day=dd["sol_day"],
                                          sol_day_3750=dd["sol_day"] * 3750 / mk.opportunities, trades_day_3750=dd["landed"] * 3750 / mk.opportunities)
        out["keep"] = f
        out["ev"] = ev
        out["break_even_L1"] = PM.break_even_L1(pol, PM.Execution())
        res["volume"][name] = out
        log(f"volume {name:28s} keep {f*100:4.0f}% EV {ev*100:+.2f} trades/day@20% {out['L20']['trades_day']:6.0f} SOL/day@20% "
            f"{out['L20']['sol_day']:+6.1f} (3,750/day: {out['L20']['sol_day_3750']:+5.1f}) break-even L1 {out['break_even_L1']*100:.1f}% "
            f"contested x{lm:.2f}: {out['sol_day_L20_contested']:+.1f}")
    json.dump(res, open(OUT / "angles_holdout.json", "w"), indent=1, default=float)
    np.savez(HB / "selections.npz", mints=np.array([r["mint"] for r in rows]), **{k.replace(":", "_"): v for k, v in S.items()})
    log("wrote", OUT / "angles_holdout.json")


def compete():
    """Diagnostic (after the one-shot evaluation): competition for rank 1 on the selected HB launches, from the chain-only
    prescreen (failed slot-0 / slot+1 transactions on the curve; 'saturated' = >= 1000 curve signatures in ~5 s)."""
    z = np.load(HB / "selections.npz")
    rows = {json.loads(l)["mint"]: json.loads(l) for l in open(ROWS)}
    pre = {json.loads(l)["mint"]: json.loads(l) for l in open(HB / "prescreen.jsonl")}
    mints = z["mints"]
    hb = np.array([rows[m]["src"] == "HB" for m in mints])
    fol = np.array([rows[m]["s0_third"] for m in mints], float)
    sat = np.array([bool(pre.get(m) and not pre[m]["reached"]) for m in mints])
    fail = np.array([pre[m]["n_s0_fail"] if (pre.get(m) and pre[m]["reached"]) else np.nan for m in mints], float)
    ret = np.array([rows[m]["ret_base"] for m in mints])
    out = {}
    for k in z.files:
        if k == "mints" or k.endswith("_mask"):
            continue
        sel = z[k].astype(bool) & hb
        if sel.sum() < 5:
            continue
        def stat(m):
            return dict(n=int(m.sum()), saturated=float(sat[m].mean()), fails=float(np.nanmean(fail[m])), followers=float(fol[m].mean()),
                        p_first=float(np.mean(1.0 / (1.0 + fol[m] + np.where(np.isnan(fail[m]), np.nanmean(fail[hb & sat]) if (hb & sat & ~np.isnan(fail)).any() else 10.0, fail[m])))),
                        ev=float(ret[m].mean()))
        out[k] = dict(kept=stat(sel), all=stat(hb))
        a, b = out[k]["kept"], out[k]["all"]
        log(f"{k:28s} HB kept {a['n']:3d}: saturated {a['saturated']*100:4.0f}% vs {b['saturated']*100:4.0f}% | failed slot-0 txs {a['fails']:.2f} vs {b['fails']:.2f} "
            f"| followers {a['followers']:.2f} vs {b['followers']:.2f}")
    # EV by competition on HB
    for nm, m in (("saturated", hb & sat), ("fails>=5", hb & ~sat & (fail >= 5)), ("fails 1-4", hb & ~sat & (fail >= 1) & (fail < 5)), ("fails 0", hb & ~sat & (fail == 0))):
        log(f"HB {nm:10s} n={m.sum():3d} LEAD EV {ret[m].mean()*100:+.2f}")
        out[f"ev_{nm}"] = dict(n=int(m.sum()), ev=float(ret[m].mean()) if m.any() else None)
    json.dump(out, open(OUT / "angles_compete.json", "w"), indent=1)


def describe():
    """HB coverage: the contiguous block actually pulled, creates in it, eligible, multi-wallet share."""
    idx = json.load(open(HB / "index.json"))
    lo, hi = hb_block()
    tp = [t for t in (json.loads(l) for l in open(HB / "tapes.jsonl")) if lo <= t["bt"] <= hi]
    pre = {json.loads(l)["sig"]: json.loads(l) for l in open(HB / "prescreen.jsonl")}
    creates = [s for s in idx["sigs"] if not s["err"] and s["bt"] and lo <= s["bt"] <= hi]
    recs = {}
    for line in open(HB / "creates.jsonl"):
        r = json.loads(line)
        if r.get("rec") and lo <= r["rec"]["create_time"] <= hi:
            recs[r["rec"]["create_sig"]] = r["rec"]
    import angles_holdout as AH
    elig = [r for r in recs.values() if AH.eligible(r)]
    pres = [pre[r["create_sig"]] for r in elig if r["create_sig"] in pre]
    kinds = collections.Counter()
    for r in tp:
        if not r["complete"]:
            kinds["incomplete"] += 1
            continue
        t = build_tape(r["mint"], "HB", 1.0, r["slot"], r["bt"], r["sig"], {r["creator"], r["user"], r["fee_payer"]},
                       r["trades"], r["hor"], True) if r["trades"] else None
        kinds["none" if t is None else t.kind] += 1
    out = dict(block=(time.strftime("%H:%M:%S", time.gmtime(lo)), time.strftime("%H:%M:%S", time.gmtime(hi))), minutes=(hi - lo) / 60,
               creates=len(creates), decoded=len(recs), eligible=len(elig), prescreened=len(pres),
               pre_multi=collections.Counter(str(x["multi"]) for x in pres), pulled=len(tp), calls=sum(t["calls"] for t in tp),
               kinds=dict(kinds), multi_share_of_creates=kinds["multi"] / max(1, len(creates)),
               multi_per_day_at_this_rate=kinds["multi"] / max(1e-9, (hi - lo) / 86400))
    print(json.dumps(out, indent=1, default=str))
    json.dump(out, open(OUT / "angles_hb_coverage.json", "w"), indent=1, default=str)


if __name__ == "__main__":
    globals()[sys.argv[1]]()
