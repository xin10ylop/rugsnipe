"""C5: adversarial critic of the S5 copy-trading study (docs/research/strategy-copytrade.md).

Stages:
  holdout_pull   fresh holdout: pump.fun creates made AFTER the last tape horizon used for wallet scoring
                 (so every score is frozen before the window) and at least 60 min old; create records via
                 getTransaction, first 150 s of trades via swap-api (<= 700 requests, ~0.3 req/s).
  audit          re-checks of the study: BH q-values recomputed, look-ahead / leakage probes, tape gap check,
                 selection weights, own-impact and rank-premium stress.
  holdout_eval   frozen wallet scores (all 1,023 study tapes) -> signals on the holdout -> the top policies.

Run: .venv/bin/python -I analysis/scripts/strategy/copytrade_critic.py <stage>
Caches: data/cache/strategy_holdout_copy/; results: data/reports/strategy/copytrade_critic.json
"""
from __future__ import annotations

import collections
import json
import math
import pickle
import random
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
sys.path.insert(0, str(REPO / "analysis" / "scripts" / "strategy"))
from rugsnipe_analysis.strategy import copytrade as ct  # noqa: E402

HC = REPO / "data" / "cache" / "strategy_holdout_copy"
HC.mkdir(parents=True, exist_ok=True)
REPORTS = REPO / "data" / "reports" / "strategy"
MINT_AUTH = "TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM"
REQ_CAP = 700
HOLD_W = 150          # seconds of tape pulled per holdout launch


def study_tapes():
    return pickle.load(open(ct.CACHE / "tapes.pkl", "rb"))


# ============================================================================ holdout pull
def holdout_pull():
    from rugsnipe_analysis.dataset import creation_record
    from rugsnipe_analysis.pumpapi import PumpApi, normalize_trade
    from rugsnipe_analysis.rpc import RpcClient

    T = study_tapes()
    t_freeze = max(t.end_time for t in T)          # last moment of data any wallet score can contain
    idx_p = HC / "index.json"
    if idx_p.exists():
        idx = json.load(open(idx_p))
    else:
        sig_rpc = RpcClient(endpoints=("https://api.mainnet-beta.solana.com",), max_rps=2.0)
        anchor = int(time.time())
        sigs, before, pages = [], None, 0
        while True:
            page = sig_rpc.get_signatures(MINT_AUTH, before=before)
            pages += 1
            if not page:
                break
            sigs += [{"sig": s["signature"], "slot": s["slot"], "bt": s.get("blockTime"),
                      "err": s.get("err") is not None} for s in page]
            before = page[-1]["signature"]
            oldest = page[-1].get("blockTime") or anchor
            print(time.strftime("%H:%M:%S"), "index page", pages, len(sigs), "oldest h ago",
                  round((anchor - oldest) / 3600, 2), flush=True)
            if oldest < t_freeze or oldest < anchor - 8 * 3600:
                break
        idx = {"anchor": anchor, "t_freeze": t_freeze, "pages": pages, "sigs": sigs}
        json.dump(idx, open(idx_p, "w"))
    anchor = idx["anchor"]
    lo = max(t_freeze, anchor - 8 * 3600)
    pool = [s for s in idx["sigs"] if not s["err"] and s["bt"] and lo < s["bt"] <= anchor - 3600]
    random.Random(20261014).shuffle(pool)
    print("eligible creates", len(pool), "window", time.strftime("%H:%M", time.gmtime(lo)), "->",
          time.strftime("%H:%M", time.gmtime(anchor - 3600)), flush=True)

    # ---- create records (publicnode getTransaction)
    cr_p = HC / "creates.jsonl"
    have = {}
    if cr_p.exists():
        for line in open(cr_p):
            r = json.loads(line)
            have[r["q_sig"]] = r
    tx_rpc = RpcClient(endpoints=("https://solana-rpc.publicnode.com",), max_rps=6.0)
    want = [s for s in pool[:1000] if s["sig"] not in have]

    def dec(s):
        try:
            rec = creation_record(tx_rpc, s["sig"])
        except Exception as e:  # noqa: BLE001
            return {"q_sig": s["sig"], "err": str(e)[:100]}
        return {"q_sig": s["sig"], "rec": rec}

    if want:
        with ThreadPoolExecutor(6) as ex, open(cr_p, "a") as fo:
            for r in ex.map(dec, want):
                fo.write(json.dumps(r, separators=(",", ":")) + "\n")
                have[r["q_sig"]] = r
        print("decoded", len(have), tx_rpc.stats, flush=True)

    # ---- trades (swap-api), random order, request cap
    tr_p = HC / "trades.jsonl"
    done = set()
    if tr_p.exists():
        done = {json.loads(line)["mint"] for line in open(tr_p)}
    api = PumpApi(max_rps=float(sys.argv[2]) if len(sys.argv) > 2 else 0.3)
    calls0 = sum(json.loads(line).get("calls", 0) for line in open(tr_p)) if tr_p.exists() else 0
    fo = open(tr_p, "a")
    n_skip = collections.Counter()
    for s in pool[:1000]:
        r = have.get(s["sig"], {}).get("rec")
        if not r:
            n_skip["no_rec"] += 1
            continue
        if r["is_mayhem_mode"] or not r["sol_quoted"] or r["n_creates_in_tx"] != 1:
            n_skip["mayhem/quote"] += 1
            continue
        if r["mint"] in done:
            continue
        if sum(x["sol"] for x in r["create_tx_trades"] if x["buy"]) >= 84.9:
            n_skip["slot0_graduation"] += 1      # whole curve bought in the create tx: no curve to copy on
            continue
        if calls0 + api.stats["calls"] + 12 > REQ_CAP:
            break
        c_before = api.stats["calls"]
        bt, slot = r["create_time"], r["create_slot"]
        seen, complete, hor = {}, False, None
        for W, mp in ((HOLD_W, 8), (95, 4)):
            raw, comp = api.trades_before(r["mint"], (bt + W) * 1000 + 999, max_pages=mp)
            for t in raw:
                n = normalize_trade(t)
                seen[(n["sig"], n["sub"], n["user"])] = n
            if comp:
                complete, hor = True, W
                break
        tr = sorted(seen.values(), key=lambda x: (x["slot"], x["txi"], x["sub"]))
        tr = [t for t in tr if t["slot"] >= slot and t["ts"] <= bt + (hor or 0) and t["mcap"] > 0]
        out = {"mint": r["mint"], "complete": complete, "hor": hor, "slot": slot, "bt": bt, "sig": r["create_sig"],
               "creator": r["creator"], "user": r["create_user"], "fee_payer": r["fee_payer"],
               "calls": api.stats["calls"] - c_before,
               "trades": [[t["slot"], t["txi"], t["sub"], t["ts"], t["user"], int(t["buy"]), round(t["sol"], 9),
                           round(t["tok"], 6), t["mcap"], t["sig"], t["prog"]] for t in tr]}
        fo.write(json.dumps(out, separators=(",", ":")) + "\n")
        fo.flush()
        done.add(r["mint"])
        if len(done) % 25 == 0:
            print(time.strftime("%H:%M:%S"), "tapes", len(done), "calls", calls0 + api.stats["calls"], api.stats,
                  flush=True)
    print("done tapes", len(done), "calls", calls0 + api.stats["calls"], dict(n_skip), api.stats, flush=True)


# ============================================================================ shared helpers
def load_script():
    """The study's driver module (walk_forward, pop_weights, ...), loaded under another name."""
    import importlib.util
    spec = importlib.util.spec_from_file_location("ct_script", REPO / "analysis/scripts/strategy/copytrade.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


# (kind, lag, size, exit keys, tp, max hold, front)
POLICIES = {
    "P1_front_Sd_1-3": ("Sd_d_1-3", 1, 0.5, ("copy", "idle3"), 1.0, 60, True),
    "P1r_real_Sd_1-3": ("Sd_d_1-3", 1, 0.5, ("copy", "idle3"), 1.0, 60, False),
    "P2_Sb_R3_scalp": ("Sb_R3", 1, 0.5, ("idle3",), 0.3, 60, False),
    "P3_Sd_lt0.3_scalp": ("Sd_d_lt0.3", 1, 0.5, ("idle3",), 0.2, 60, False),
    "D_Sb_R1_default": ("Sb_R1", 1, 0.25, ("copy",), None, 300, False),
    "C0_default": ("C0_first_indep", 1, 0.25, ("copy",), None, 300, False),
    "C0_same_scalp": ("C0_first_indep", 1, 0.5, ("idle3",), 0.3, 60, False),
    "C0_same_scalp_tp20": ("C0_first_indep", 1, 0.5, ("idle3",), 0.2, 60, False),
}
COST = 0.004


def pol_returns(events, pol, evs_key=None):
    kind, lag, size, keys, tp, H, front = pol
    out = []
    for e in events:
        if e["kind"] != kind:
            continue
        ev = e["evs"].get(evs_key or (lag, size))
        if ev is None:
            continue
        v, why = ct.policy_exit(ev, keys, tp, H, front)
        out.append((e["mint"], (v - size - COST) / size, why))
    return out


def summarize(rets, T_by, W, cut, split="test", popn="pop", boot=True):
    x, w = [], []
    for m, r, _ in rets:
        t = T_by[m]
        if (t.bt >= cut) != (split == "test"):
            continue
        wt = W.get(m, 1.0) if popn == "pop" else (t.w if popn == "w" else 1.0)
        if wt <= 0:
            continue
        x.append(r)
        w.append(wt)
    x, w = np.array(x), np.array(w)
    if len(x) == 0:
        return {"n": 0}
    m, se = ct.wmean_se(x, w)
    o = {"n": int(len(x)), "ev": m, "se": se, "p": ct.norm_sf(m / se) if se and se > 0 else float("nan"),
         "hit": float((w * (x > 0)).sum() / w.sum())}
    if boot:
        _, lo, hi = ct.boot_ci(x, w)
        o["ci90"] = [lo, hi]
    return o


def fmt(o):
    if not o or o.get("n", 0) == 0:
        return "n=0"
    ci = o.get("ci90")
    return (f"n={o['n']:4d} ev={o['ev']*100:+6.2f}%" + (f" [{ci[0]*100:+.1f},{ci[1]*100:+.1f}]" if ci else "")
            + f" hit={o['hit']:.2f}")


# ============================================================================ audit
def audit():
    mod = load_script()
    T = study_tapes()
    T_by = {t.mint: t for t in T}
    mod.led = [ct.ledger(t) for t in T]
    mod.preps = [ct.prep(t) for t in T]
    bts = np.array([t.bt for t in T])
    cut = int(np.quantile(bts, 0.60))
    W, _ = mod.pop_weights(T, cut)
    out = {"cut_bt": cut}

    # ---- A1: BH recomputed from the stored sweep
    d = json.load(open(REPORTS / "copytrade_sweep.json"))
    R = d["res"]
    a1 = {}
    for popn in ("pop", "all"):
        rows = [r for r in R if r[f"test_{popn}"].get("n", 0) >= 5]
        p = np.array([r[f"test_{popn}"]["p"] for r in rows])
        o = np.argsort(p)
        m_ = len(p)
        qq = np.minimum.accumulate((p[o] * m_ / np.arange(1, m_ + 1))[::-1])[::-1]
        q = np.empty(m_)
        q[o] = np.minimum(qq, 1.0)
        qneg = np.array(ct.bh([1 - x for x in p]))
        a1[popn] = {"n_tested": m_, "min_p": float(p.min()), "min_q": float(q.min()),
                    "n_p_lt_0.05": int((p < 0.05).sum()), "expected_by_chance": 0.05 * m_,
                    "n_sig_negative_q_lt_0.05": int((qneg < 0.05).sum()),
                    "max_abs_diff_vs_stored": float(np.nanmax(np.abs(q - np.array([r[f"test_{popn}"].get("q", np.nan)
                                                                                        for r in rows]))))}
    out["A1_bh"] = a1
    print("A1", json.dumps(a1))

    # ---- A2: baseline reproduction
    t0 = time.time()
    events, info = mod.walk_forward(T)
    print("walk_forward", round(time.time() - t0), "s", flush=True)
    base = {}
    for nm, pol in POLICIES.items():
        rr = pol_returns(events, pol)
        base[nm] = {"test_pop": summarize(rr, T_by, W, cut), "train_pop": summarize(rr, T_by, W, cut, "train"),
                    "test_all": summarize(rr, T_by, W, cut, popn="all"),
                    "test_w": summarize(rr, T_by, W, cut, popn="w"),
                    "exit_reasons_test": dict(collections.Counter(w for m, _, w in rr if T_by[m].bt >= cut))}
        print("A2", nm, "test", fmt(base[nm]["test_pop"]), "| train", fmt(base[nm]["train_pop"]),
              "| all(unit)", fmt(base[nm]["test_all"]), "| all(w)", fmt(base[nm]["test_w"]), flush=True)
    out["A2_baseline"] = base

    # ---- A3: remove the xrecv look-ahead from the current-launch signal exclusion
    import inspect
    src = inspect.getsource(mod.walk_forward)
    assert "ins = t.C | t.B | t.funders | t.xrecv" in src
    src2 = src.replace("ins = t.C | t.B | t.funders | t.xrecv", "ins = t.C | t.B | t.funders")
    ns = dict(mod.__dict__)
    exec(compile(src2, "wf_noxrecv", "exec"), ns)
    ev2, _ = ns["walk_forward"](T)
    a3 = {}
    for nm, pol in POLICIES.items():
        rr = pol_returns(ev2, pol)
        a3[nm] = summarize(rr, T_by, W, cut)
        print("A3 no-xrecv", nm, fmt(a3[nm]), flush=True)
    out["A3_no_xrecv_lookahead"] = a3
    k1 = collections.Counter(e["kind"] for e in events)
    k2 = collections.Counter(e["kind"] for e in ev2)
    out["A3_event_counts"] = {k: [k1.get(k, 0), k2.get(k, 0)] for k in sorted(set(k1) | set(k2))}

    # ---- A4: slower score refresh (a launch only counts 30 min after its horizon closes)
    src4 = src.replace("while pending and pending[0][0] < t.bt:", "while pending and pending[0][0] + 1800 < t.bt:")
    ns4 = dict(mod.__dict__)
    exec(compile(src4, "wf_delay", "exec"), ns4)
    ev4, _ = ns4["walk_forward"](T)
    out["A4_refresh_delay_30min"] = {nm: summarize(pol_returns(ev4, pol), T_by, W, cut) for nm, pol in POLICIES.items()}
    for nm, o in out["A4_refresh_delay_30min"].items():
        print("A4 delay30", nm, fmt(o))

    # ---- A5: entry stress: lag 2/3 and an extra rank premium on our fill (copiers ahead of us, not in the tape)
    a5 = {}
    q_orig = ct.q_for
    for prem in (0.0, 0.011, 0.036, 0.108):
        for lag in (1, 2, 3):
            if prem > 0 and lag != 1:
                continue
            ct.q_for = (lambda p: (lambda size, mc: q_orig(size, mc * (1 + p))))(prem)
            for nm, pol in POLICIES.items():
                kind, _, size, keys, tp, H, front = pol
                rr = []
                for e in events:
                    if e["kind"] != kind:
                        continue
                    t = T_by[e["mint"]]
                    if t.bt < cut:
                        continue
                    li = e["li"]
                    ev = ct.event_paths(t, mod.preps[li], e["i"], e["w"], lag, size)
                    if ev is None:
                        continue
                    v, why = ct.policy_exit(ev, keys, tp, H, front)
                    rr.append((e["mint"], (v - size - COST) / size, why))
                a5[f"{nm}|lag{lag}|prem{prem}"] = summarize(rr, T_by, W, cut, boot=False)
            ct.q_for = q_orig
    out["A5_entry_stress"] = a5
    for k, o in a5.items():
        print("A5", k, fmt(o))

    # ---- A6: fill check against integer curve math on 200 random events
    from rugsnipe_analysis import curve as cv
    rng = random.Random(5)
    errs = []
    for e in rng.sample(events, 200):
        ev = e["evs"][(1, 0.5)]
        if ev is None:
            continue
        vs, vt = cv.reserves_from_mcap(ev["mc_e"])
        tok, nvs, nvt = cv.buy_tokens_out(int(0.5e9), int(vs), int(vt))
        q = ct.q_for(0.5, ev["mc_e"])
        sol, _, _ = cv.sell_sol_out(tok, nvs, nvt)
        errs.append((abs(tok - q) / tok, abs(sol / 1e9 - ev["v"][0])))
    errs = np.array(errs)
    out["A6_fill_check"] = {"max_rel_tok_err": float(errs[:, 0].max()), "max_abs_sol_err_roundtrip": float(errs[:, 1].max())}
    print("A6", out["A6_fill_check"])

    # ---- A7: entry premium vs the copied fill, and where the front-run exit lands
    a7 = {}
    for kind in ("Sd_d_1-3", "Sb_R3", "Sd_d_lt0.3", "Sb_R1", "C0_first_indep"):
        E = [e for e in events if e["kind"] == kind and T_by[e["mint"]].bt >= cut and e["evs"][(1, 0.5)] is not None]
        prem = np.array([e["evs"][(1, 0.5)]["prem"] for e in E])
        fr0 = np.mean([e["evs"][(1, 0.5)]["trig"].get("front", 1) == 0 for e in E])
        nos = np.mean(["copy" not in e["evs"][(1, 0.5)]["trig"] for e in E])
        a7[kind] = {"n": len(E), "prem_mean": float(prem.mean()), "prem_p90": float(np.quantile(prem, 0.9)),
                    "share_prem_eq_0": float((prem == 0).mean()), "share_front_at_entry": float(fr0),
                    "share_leader_never_sells_in_horizon": float(nos)}
    out["A7_entry_and_front"] = a7
    print("A7", json.dumps(a7))
    pickle.dump(events, open(HC / "study_events.pkl", "wb"))
    json.dump(out, open(HC / "audit.json", "w"), indent=1, default=float)


# ============================================================================ hot launches dropped by the study (survivorship)
HOT_DEEP = ("GK9Tu2ko", "Hsh2mAmP")      # bonding-curve launches with >2,000 trades in 300 s (test period)


def hot_pull():
    from rugsnipe_analysis.pumpapi import PumpApi, normalize_trade
    api = PumpApi(max_rps=0.25)
    out_p = HC / "hot_trades.jsonl"
    done = {json.loads(line)["mint"] for line in open(out_p)} if out_p.exists() else set()
    fo = open(out_p, "a")
    for r in map(json.loads, open(REPO / "data" / "tokens_sample.jsonl")):
        if not r["mint"].startswith(HOT_DEEP) or r["mint"] in done:
            continue
        raw, comp = api.trades_before(r["mint"], (r["create_time"] + 100) * 1000 + 999, max_pages=25)
        tr = sorted((normalize_trade(t) for t in raw), key=lambda x: (x["slot"], x["txi"], x["sub"]))
        tr = [t for t in tr if t["slot"] >= r["create_slot"] and t["mcap"] > 0]
        fo.write(json.dumps({"mint": r["mint"], "complete": comp, "hor": 100, "slot": r["create_slot"],
                             "bt": r["create_time"], "sig": r["create_sig"], "creator": r["creator"],
                             "user": r["create_user"], "fee_payer": r["fee_payer"],
                             "trades": [[t["slot"], t["txi"], t["sub"], t["ts"], t["user"], int(t["buy"]), t["sol"],
                                         t["tok"], t["mcap"], t["sig"], t["prog"]] for t in tr]}) + "\n")
        fo.flush()
        print(r["mint"][:8], "complete", comp, "trades", len(tr), api.stats, flush=True)


# ============================================================================ holdout evaluation
def mk_tapes(path, src, frozen):
    """Holdout tapes in the study's Tape format; `frozen` ones never feed back into wallet scores."""
    H, n_inc, n_none = [], 0, 0
    if not Path(path).exists():
        return H, {"n_incomplete": 0, "n_none": 0}
    for r in map(json.loads, open(path)):
        if not r.get("complete") or not r.get("trades"):
            n_inc += int(not r.get("complete"))
            n_none += int(bool(r.get("complete")))
            continue
        C = {r["creator"], r["user"], r["fee_payer"]}
        t = ct._mk(r["mint"], "deep", 1.0, r["slot"], r["bt"], r["sig"], r["creator"], C, r["trades"], int(r["hor"]))
        if t is None:
            n_none += 1
            continue
        t.src = src
        if frozen:
            t.hor = 10 ** 9       # end_time never passes -> never flushed into wallet scores
        H.append(t)
    return H, {"n_incomplete": n_inc, "n_none": n_none}


def run_with(mod, T, extra):
    TT = sorted(T + extra, key=lambda t: (t.bt, t.mint))
    mod.led = [ct.ledger(t) for t in TT]
    mod.preps = [ct.prep(t) for t in TT]
    events, info = mod.walk_forward(TT)
    return TT, events


def unit_summary(rets, boot=True):
    x = np.array([r for _, r, _ in rets])
    if len(x) == 0:
        return {"n": 0}
    w = np.ones_like(x)
    m, se = ct.wmean_se(x, w)
    o = {"n": int(len(x)), "ev": m, "se": se, "p": ct.norm_sf(m / se) if se and se > 0 else float("nan"),
         "hit": float((x > 0).mean()), "median": float(np.median(x))}
    if boot:
        _, lo, hi = ct.boot_ci(x, w)
        o["ci90"] = [lo, hi]
    return o


def holdout_eval():
    mod = load_script()
    T = study_tapes()
    t_freeze = max(t.end_time for t in T)
    out = {"t_freeze": t_freeze}
    HA, stA = mk_tapes(HC / "trades.jsonl", "holdA", True)
    assert all(t.bt > t_freeze for t in HA)
    HB, stB = mk_tapes(REPO / "data" / "cache" / "strategy" / "holdout_trades.jsonl", "holdB", True)
    study = {t.mint for t in T}
    HB = [t for t in HB if t.mint not in study]
    HOT, stH = mk_tapes(HC / "hot_trades.jsonl", "hot", True)
    nA = sum(1 for _ in open(HC / "trades.jsonl"))
    out["holdA_meta"] = {"pulled": nA, "usable": len(HA), **stA, "calls": sum(json.loads(l).get("calls", 0) for l in open(HC / "trades.jsonl")),
                         "window_utc": [time.strftime("%m-%d %H:%M", time.gmtime(min(t.bt for t in HA))),
                                        time.strftime("%m-%d %H:%M", time.gmtime(max(t.bt for t in HA)))],
                         "bundled_class_share": float(np.mean([t.gap3cls for t in HA]))}
    out["holdB_meta"] = {"usable": len(HB), **stB, "bundled_class_share": float(np.mean([t.gap3cls for t in HB])) if HB else None}
    print("holdA", out["holdA_meta"], "holdB", out["holdB_meta"], "hot", len(HOT), stH, flush=True)
    TT, events = run_with(mod, T, HA + HB + HOT)
    by = {t.mint: t for t in TT}
    pickle.dump([e for e in events if by[e["mint"]].src in ("holdA", "holdB", "hot")], open(HC / "holdout_events.pkl", "wb"))
    res = {}
    for split in ("holdA", "holdB", "holdAB", "hot"):
        srcs = {"holdAB": ("holdA", "holdB")}.get(split, (split,))
        E = [e for e in events if by[e["mint"]].src in srcs]
        nl = sum(1 for t in TT if t.src in srcs)
        res[split] = {"n_launches": nl, "events": dict(collections.Counter(e["kind"] for e in E))}
        for nm, pol in POLICIES.items():
            for lag in (1, 2):
                pl = (pol[0], lag, *pol[2:])
                rr = pol_returns(E, pl)
                o = unit_summary(rr)
                o["exit_reasons"] = dict(collections.Counter(w for _, _, w in rr))
                o["signals_per_launch"] = len(rr) / max(nl, 1)
                res[split][f"{nm}|lag{lag}"] = o
                print(split, f"{nm}|lag{lag}", fmt(o), flush=True)
    # BH over the holdout tests of the three surviving policies (lag 1 and 2, holdout A and A+B)
    tests = [(k, sp) for sp in ("holdA", "holdAB") for k in res[sp] if k.split("|")[0] in
             ("P1_front_Sd_1-3", "P1r_real_Sd_1-3", "P2_Sb_R3_scalp", "P3_Sd_lt0.3_scalp")]
    ps = [res[sp][k].get("p", 1.0) if res[sp][k].get("n", 0) >= 2 else 1.0 for k, sp in tests]
    ps = [1.0 if (p is None or (isinstance(p, float) and math.isnan(p))) else p for p in ps]
    qs = ct.bh(ps)
    for (k, sp), q in zip(tests, qs):
        res[sp][k]["q_bh"] = q
    out["n_holdout_tests_bh"] = len(tests)
    out["res"] = res
    json.dump(out, open(REPORTS / "copytrade_critic_holdout.json", "w"), indent=1, default=float)


# ============================================================================ lead check: fast leaders' own ROI on the holdouts
def lead_check():
    """Wallets with a point-in-time median hold < 5 s over >= 3 earlier early launches and a positive mean ROI there.
    Their own ROI (and the default copy of them) on later launches: study train / test, holdout A (frozen), holdout B."""
    T = study_tapes()
    cut = int(np.quantile([t.bt for t in T], 0.60))
    HA, _ = mk_tapes(HC / "trades.jsonl", "holdA", True)
    HB, _ = mk_tapes(REPO / "data" / "cache" / "strategy" / "holdout_trades.jsonl", "holdB", True)
    study = {t.mint for t in T}
    HB = [t for t in HB if t.mint not in study]
    allt = sorted(T + HA + HB, key=lambda t: t.bt)
    seen = collections.defaultdict(list)
    pending, rows = [], []
    for t in allt:
        pending.sort(key=lambda x: x[0])
        while pending and pending[0][0] < t.bt:
            _, ul = pending.pop(0)
            for u, hd, roi in ul:
                seen[u].append((hd, roi))
        L = ct.ledger(t)
        P = ct.prep(t)
        ins = t.C | t.B | t.funders | t.xrecv
        done = []
        for u, r in L.items():
            if r["fb"] is None or u in ins or r["fb_s"] >= 30:
                continue
            h = seen.get(u, [])
            roi = r["pnl"] / r["sin"] if r["sin"] else 0.0
            if len(h) >= 3 and np.median([x[0] for x in h]) < 5 and np.mean([x[1] for x in h]) > 0:
                ev = ct.event_paths(t, P, r["fb"], u, 1, 0.5)
                cp = (ct.policy_exit(ev, ("copy",), None, 300)[0] - 0.5 - COST) / 0.5 if ev else None
                split = t.src if t.src in ("holdA", "holdB") else ("test" if t.bt >= cut else "train")
                rows.append((split, roi, r["pnl"], r["sin"], cp, t.mint))
            done.append((u, r["hold"] if r["hold"] is not None else 900.0, roi))
        if t.src not in ("holdA", "holdB"):          # holdout launches never feed the history
            pending.append((t.end_time, done))
    out = {}
    for sp in ("train", "test", "holdA", "holdB"):
        R = [r for r in rows if r[0] == sp]
        if not R:
            out[sp] = {"n": 0}
            continue
        x = np.array([r[1] for r in R])
        pnl = np.array([r[2] for r in R])
        sin = np.array([r[3] for r in R])
        cps = np.array([r[4] for r in R if r[4] is not None])
        # cluster bootstrap by launch for the per-trade mean
        mints = sorted({r[5] for r in R})
        idx = {m: [i for i, r in enumerate(R) if r[5] == m] for m in mints}
        rng = np.random.default_rng(11)
        bs, bw = [], []
        for _ in range(2000):
            pick = rng.integers(0, len(mints), len(mints))
            ii = [i for k in pick for i in idx[mints[k]]]
            bs.append(x[ii].mean())
            bw.append((pnl[ii].sum() - COST * len(ii)) / sin[ii].sum())
        out[sp] = {"pairs": len(R), "launches": len(mints), "own_roi_mean": float(x.mean()),
                   "own_roi_ci90": [float(np.quantile(bs, 0.05)), float(np.quantile(bs, 0.95))],
                   "own_roi_sol_weighted": float(pnl.sum() / sin.sum()),
                   "own_roi_sol_weighted_net_0.004": float((pnl.sum() - COST * len(R)) / sin.sum()),
                   "own_roi_sol_weighted_net_ci90": [float(np.quantile(bw, 0.05)), float(np.quantile(bw, 0.95))],
                   "copy_default_0.5": float(cps.mean()) if len(cps) else None}
        print(sp, json.dumps(out[sp]), flush=True)
    json.dump(out, open(HC / "lead_check.json", "w"), indent=1)


# ============================================================================ probes (A8-A12)
def _bad_reserves(t):
    end = t.hend
    if end < 2:
        return False
    vs = np.sqrt(ct.K_CURVE * t.mc[:end] / 1e6) / 1e9
    d = np.diff(vs)
    sg = np.where(t.buy[1:end], t.sol[1:end], -t.sol[1:end])
    _, inv = np.unique(t.slot[1:end], return_inverse=True)
    b = np.abs(np.bincount(inv, weights=d) - np.bincount(inv, weights=sg)) > \
        (0.02 * np.bincount(inv, weights=np.abs(sg)) + 0.002)
    return bool(b.sum() > 0)


def probes():
    import itertools
    mod = load_script()
    T = study_tapes()
    cut = int(np.quantile([t.bt for t in T], 0.60))
    out = {}
    # ---- A8: drop tapes whose implied reserves are inconsistent (paging gaps / non-standard curves)
    bad = [t for t in T if _bad_reserves(t)]
    out["A8_inconsistent_tapes"] = {s_: sum(1 for t in bad if t.src == s_) for s_ in ("gap3", "ops", "deep")}
    T2 = [t for t in T if not _bad_reserves(t)]
    TT, ev = run_with(mod, T2, [])
    W, _ = mod.pop_weights(TT, cut)
    by = {t.mint: t for t in TT}
    out["A8_clean"] = {nm: summarize(pol_returns(ev, pol), by, W, cut) for nm, pol in POLICIES.items()}
    # ---- A9: restore the 2 hot test launches dropped by paging (not frozen: they are study-period launches)
    HOT, _ = mk_tapes(HC / "hot_trades.jsonl", "deep", False)
    for t in HOT:
        t.src = "deep"
    TT, ev = run_with(mod, T, HOT)
    W, _ = mod.pop_weights(TT, cut)
    by = {t.mint: t for t in TT}
    out["A9_hot_restored"] = {nm: summarize(pol_returns(ev, pol), by, W, cut) for nm, pol in POLICIES.items()}
    # ---- baseline events (study tapes only)
    TT, events = run_with(mod, T, [])
    W, _ = mod.pop_weights(TT, cut)
    by = {t.mint: t for t in TT}
    # ---- A10: the guru's "auto-sell on big buy" exit (missing from the study's grid)
    kinds = ["Sb_R1", "Sb_R3", "Sb_R4", "Sc_2in5s", "Sd_d_lt0.3", "Sd_d_0.3-1", "Sd_d_1-3", "Sd_band", "C0_first_indep"]
    BIG, COMBO, TPS_, HS = (0.5, 1.0, 2.0, 4.0), [(), ("idle3",), ("copy",), ("copy", "idle3")], (None, 0.3), (60, 300)
    res = []
    for kind in kinds:
        E = [e for e in events if e["kind"] == kind]
        for size in (0.25, 0.5):
            base = []
            for e in E:
                ev = e["evs"][(1, size)]
                if ev is None:
                    continue
                t = by[e["mint"]]
                e0, end = ev["e"], t.hend
                ws = {e["w"]} if isinstance(e["w"], str) else set(e["w"])
                trig = {}
                for X in BIG:
                    j = next((k for k in range(e0 + 1, end) if t.buy[k] and t.sol[k] >= X and t.user[k] not in ws), None)
                    if j is not None:
                        s_ = max(int(t.slot[j]) + 1, ev["s_ent"] + 1)
                        trig[X] = max(int(np.searchsorted(t.slot[e0:end], s_, side="right")) - 1, 0)
                base.append((e["mint"], ev, trig))
            for X, keys, tp, h in itertools.product(BIG, COMBO, TPS_, HS):
                x, w, sp = [], [], []
                for m, ev, trig in base:
                    v, _ = ct.policy_exit(ev, keys, tp, h)
                    if X in trig:
                        c_ = [ev["trig"][k] for k in keys if k in ev["trig"]]
                        if tp is not None and f"tp{tp}" in ev["trig"]:
                            c_.append(ev["trig"][f"tp{tp}"])
                        c_.append(ev["holds"][h][0])
                        if trig[X] < min(c_):
                            v = float(ev["v"][trig[X]])
                    x.append((v - size - COST) / size)
                    w.append(W[m])
                    sp.append(by[m].bt >= cut)
                x, w, sp = np.array(x), np.array(w), np.array(sp)
                row = {"kind": kind, "size": size, "X": X, "keys": keys, "tp": tp, "h": h}
                for nm, msk in (("train", ~sp & (w > 0)), ("test", sp & (w > 0))):
                    m_, se = ct.wmean_se(x[msk], w[msk])
                    row[nm] = [m_, se, int(msk.sum()), ct.norm_sf(m_ / se) if se > 0 else 1.0]
                res.append(row)
    q = ct.bh([r["test"][3] for r in res])
    bt_ = max(res, key=lambda r: r["train"][0])
    bs_ = max(res, key=lambda r: r["test"][0])
    out["A10_bigbuy"] = {"configs": len(res), "min_q": float(min(q)), "n_test_pos": sum(r["test"][0] > 0 for r in res),
                         "best_on_train": bt_, "best_on_test": bs_}
    print("A10", json.dumps(out["A10_bigbuy"], default=str), flush=True)
    # ---- A11: study section 5 (leader vs copy, persistence) and A12: leaders by point-in-time hold
    pairs = []
    seen = collections.defaultdict(list)
    pending = []
    for t in TT:
        pending.sort(key=lambda z: z[0])
        while pending and pending[0][0] < t.bt:
            _, ul = pending.pop(0)
            for u, hd, roi in ul:
                seen[u].append((hd, roi))
        L = ct.ledger(t)
        P = ct.prep(t)
        ins = t.C | t.B | t.funders | t.xrecv
        done = []
        for u, r in L.items():
            if r["fb"] is None or u in ins or r["fb_s"] >= 30:
                continue
            roi = r["pnl"] / r["sin"] if r["sin"] else 0.0
            done.append((u, r["hold"] if r["hold"] is not None else 900.0, roi))
            if mod.role_of(t, u) != "O":
                continue
            e25 = ct.event_paths(t, P, r["fb"], u, 1, 0.25)
            e50 = ct.event_paths(t, P, r["fb"], u, 1, 0.5)
            if e25 is None:
                continue
            h = seen.get(u, [])
            pairs.append({"u": u, "test": t.bt >= cut, "roi": roi,
                          "cp": (ct.policy_exit(e25, ("copy",), None, 300)[0] - 0.25 - COST) / 0.25,
                          "fr": (ct.policy_exit(e25, ("copy",), None, 300, True)[0] - 0.25 - COST) / 0.25,
                          "sc": (ct.policy_exit(e25, ("idle3", "trail20"), 0.2, 60)[0] - 0.25 - COST) / 0.25,
                          "cp50": (ct.policy_exit(e50, ("copy",), None, 300)[0] - 0.5 - COST) / 0.5 if e50 else None,
                          "nh": len(h), "mh": float(np.median([z[0] for z in h])) if h else None,
                          "proi": float(np.mean([z[1] for z in h])) if h else None})
        pending.append((t.end_time, done))
    A = lambda k, sel: float(np.mean([p_[k] for p_ in sel])) if sel else None
    out["A11_section5"] = {"pairs": len(pairs), "leader_roi": A("roi", pairs), "copy": A("cp", pairs),
                           "front": A("fr", pairs), "scalp": A("sc", pairs)}
    byu = collections.defaultdict(lambda: {"tr": [], "te": [], "teo": []})
    for p_ in pairs:
        d = byu[p_["u"]]
        (d["te"] if p_["test"] else d["tr"]).append(p_["cp"])
        if p_["test"]:
            d["teo"].append(p_["roi"])
    Wl = sorted([(np.mean(d["tr"]), np.mean(d["te"]), np.mean(d["teo"])) for d in byu.values()
                 if len(d["tr"]) >= 4 and len(d["te"]) >= 3])
    n = len(Wl)
    out["A11_persistence"] = {"wallets": n, "terciles": [[float(np.mean([z[i] for z in Wl[a:b]])) for i in range(3)]
                                                         for a, b in ((0, n // 3), (n // 3, 2 * n // 3), (2 * n // 3, n))],
                              "corr": float(np.corrcoef([z[0] for z in Wl], [z[1] for z in Wl])[0, 1])}
    a12 = {}
    for lo, hi in ((0, 5), (5, 30), (30, 120), (120, 1e9)):
        sel = [p_ for p_ in pairs if p_["nh"] >= 3 and lo <= p_["mh"] < hi and p_["cp50"] is not None]
        a12[f"hold_{lo}_{hi}"] = {"n": len(sel), "copy50": A("cp50", sel), "leader": A("roi", sel)}
    for lo, prmin in ((30, 0.0), (60, 0.0)):
        for split in (False, True):
            sel = [p_ for p_ in pairs if p_["nh"] >= 3 and p_["mh"] >= lo and p_["proi"] > prmin
                   and p_["test"] == split and p_["cp50"] is not None]
            a12[f"slow_ge{lo}_profitable_{'test' if split else 'train'}"] = {"n": len(sel), "copy50": A("cp50", sel),
                                                                          "leader": A("roi", sel)}
    out["A12_leaders_by_hold"] = a12
    print(json.dumps({k: v for k, v in out.items() if k != "A10_bigbuy"}, indent=1, default=float))
    json.dump(out, open(HC / "probes.json", "w"), indent=1, default=float)


if __name__ == "__main__":
    stage = sys.argv[1] if len(sys.argv) > 1 else "audit"
    {"holdout_pull": holdout_pull, "audit": audit, "hot_pull": hot_pull, "holdout_eval": holdout_eval, "lead_check": lead_check, "probes": probes}[stage]()
