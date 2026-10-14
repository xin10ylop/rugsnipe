#!/usr/bin/env python3
"""Adversarial verification of the Task C backtest (Task D).

Usage (repo root):
    .venv/bin/python -I analysis/scripts/verify_backtest.py                  # offline, cache only, ~10 s
    .venv/bin/python -I analysis/scripts/verify_backtest.py --probe 48       # + <= 96 swap-api requests
    .venv/bin/python -I analysis/scripts/verify_backtest.py --spot 5         # + 2 x 5 coin/DexScreener requests

Offline checks (data/cache only):
  units      swap-api priceSol is the post-trade spot of the k = 30 SOL x 1.073e15 curve and
             amountSol is net of fees (each trade re-derived from the previous one);
             SOL/USD of the window vs the ATH prefilter's SOL_USD_LOW
  curve/fee  initial reserves of every SOL-quoted window create; fee bps on create-tx TradeEvents
  lookahead  every slot-0 feature recomputed from a truncated and from a scrambled future
  time       slot-0 trade timestamps == create blockTime; first trade in the create slot
  ath        coin API ATH (USD) / trade SOL-USD >= max traded mcap (ATH can never be lower)
  censoring  launches whose 60 s prefix (2 pages = 200 trades) never reached slot 0 are stored
             as bundled=False; compare them with the sample and split Task A's unbiased random
             arm by the same 200-trades-in-60-s line
  payoffs    TP-hit vs non-hit payoff of the guru rule -> break-even TP-hit rate
Network (optional, rate limited, cached): --probe N reads slot 0 of N random censored launches
(<= 2 pages each) and bounds their guru-rule outcome; --spot N re-checks N sample tokens against
pump.fun coins-v2 and DexScreener.  Output: data/reports/backtest_verification.json.
"""
from __future__ import annotations

import argparse
import collections
import importlib.util
import json
import random
import statistics as st
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from rugsnipe_analysis import backtest as B  # noqa: E402
from rugsnipe_analysis import curve  # noqa: E402
from rugsnipe_analysis import discovery as D  # noqa: E402
from rugsnipe_analysis import window as W  # noqa: E402
from rugsnipe_analysis.cache import REPO_ROOT  # noqa: E402
from rugsnipe_analysis.dataset import log  # noqa: E402
from rugsnipe_analysis.pumpapi import PumpApi, normalize_trade  # noqa: E402

DATA = REPO_ROOT / "data"
OUT = DATA / "reports" / "backtest_verification.json"
HOT_TRADES_60S = 200  # = 2 pages: the cheap 60 s prefix of window.staged_trades


def _bt_script():
    """The backtest script's loaders (same cache-only loading as the published numbers)."""
    spec = importlib.util.spec_from_file_location("bt_script", Path(__file__).with_name("backtest.py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)  # type: ignore[union-attr]
    return mod


def q(xs: list[float], p: float) -> float:
    xs = sorted(xs)
    return xs[int(p * (len(xs) - 1))] if xs else float("nan")


def n60(tok: dict, tr: list[dict]) -> int:
    return sum(1 for t in tr if t["ts"] <= tok["create_time"] + 60)


# --------------------------------------------------------------------------- offline checks
def check_units(window: list) -> dict:
    """Re-derive each trade's post-trade mcap / token amount from the previous trade's state."""
    mc_err, tok_err, tok_err_gross = [], [], []
    for tok, tr in window:
        for a, b in zip(tr, tr[1:]):
            if a.get("prog") == "pump_amm" or b.get("prog") == "pump_amm":
                continue
            vsol, vtok = B.state(a["mcap"])
            if b["buy"]:
                nv = vsol + b["sol"] * 1e9
                if b["tok"] > 0:
                    tok_err.append(abs((vtok - B.K / nv) / (b["tok"] * 1e6) - 1))
                    ng = vsol + b["sol"] * 1e9 / 1.0125
                    tok_err_gross.append(abs((vtok - B.K / ng) / (b["tok"] * 1e6) - 1))
            else:
                nv = B.K / (vtok + b["tok"] * 1e6)
            mc_err.append(abs(nv * nv / B.K * 1e6 / b["mcap"] - 1))
    su = [t["sol_usd"] for _, tr in window for t in tr if t.get("sol_usd")]
    return {"pairs": len(mc_err), "mcap_rel_err_p50": q(mc_err, .5), "mcap_rel_err_p90": q(mc_err, .9),
            "buy_tokens_rel_err_p50_net": q(tok_err, .5), "buy_tokens_rel_err_p50_if_gross": q(tok_err_gross, .5),
            "sol_usd_min": min(su), "sol_usd_median": st.median(su), "sol_usd_max": max(su),
            "sol_usd_low_prefilter": W.SOL_USD_LOW, "prefilter_conservative": min(su) >= W.SOL_USD_LOW}


def check_curve_and_fees(bundled_mints: set[str]) -> dict:
    """Initial reserves of SOL-quoted window creates and fee bps of create-tx TradeEvents."""
    inits, fees_all, fees_b = collections.Counter(), collections.Counter(), collections.Counter()
    for r in W.read_jsonl(DATA / "cache" / "window" / "creates.jsonl"):
        if r.get("sol_quoted"):
            inits[(r["init_vsol"], r["init_vtok"])] += 1
        for f in {(t["fee_bps"], t["creator_fee_bps"]) for t in r.get("create_tx_trades", [])}:
            fees_all[str(f)] += 1
            if r["mint"] in bundled_mints:
                fees_b[str(f)] += 1
    std = inits.get((curve.INITIAL_VSOL, curve.INITIAL_VTOK), 0)
    return {"sol_quoted_creates": sum(inits.values()), "standard_initial_reserves": std,
            "fee_pairs_bundled": dict(fees_b), "fee_pairs_all_top": dict(fees_all.most_common(6)),
            "non_default_fee_share_all": round(1 - fees_all.get("(95, 30)", 0) / max(1, sum(fees_all.values())), 4)}


def check_lookahead(samples: list, seed: int = 3) -> dict:
    """slot0_structure must not change when the future is removed or scrambled (eos1_mcap excepted)."""
    rng = random.Random(seed)
    bad: collections.Counter = collections.Counter()
    for tok, tr in samples:
        cut = [t for t in tr if t["slot"] <= tok["create_slot"]]
        fut = [dict(t) for t in tr if t["slot"] > tok["create_slot"]]
        for t in fut:
            t["mcap"] *= rng.uniform(0.1, 10)
            t["user"] = rng.choice([tok["creator"], "X"])
            t["buy"] = rng.random() < 0.5
        f1 = D.slot0_structure(tok, cut)
        f2 = D.slot0_structure(tok, sorted(cut + fut, key=lambda t: (t["slot"], t["txi"], t["sub"])))
        bad.update(k for k in f1 if k != "eos1_mcap" and f1[k] != f2[k])
        if {**tok["s0"], "eos1_mcap": None} != {**f1, "eos1_mcap": None}:
            bad["stored_features_differ"] += 1
    return {"tokens": len(samples), "mismatches": dict(bad)}


def check_time(window: list) -> dict:
    dts = collections.Counter(t["ts"] - tok["create_time"] for tok, tr in window for t in tr
                              if t["slot"] == tok["create_slot"])
    dup = 0
    for _, tr in window:
        c = collections.Counter((t["sig"], t["user"], t["buy"], round(t["sol"], 9), round(t["tok"], 3)) for t in tr)
        dup += sum(v - 1 for v in c.values() if v > 1)
    return {"slot0_ts_minus_create_time": dict(dts),
            "first_trade_not_in_create_slot": sum(1 for tok, tr in window if tr[0]["slot"] != tok["create_slot"]),
            "duplicate_trade_rows": dup}


def check_ath(window: list) -> dict:
    """API ATH (USD) converted with the trades' own SOL/USD vs the max traded mcap."""
    rel = []
    for tok, tr in window:
        if not tok.get("ath_usd"):
            continue
        su = [t["sol_usd"] for t in tr if t.get("sol_usd")]
        if su:
            rel.append(tok["ath_usd"] / st.median(su) / max(t["mcap"] for t in tr) - 1)
    return {"n": len(rel), "api_ath_below_trade_max_by_gt_0.5pct": sum(1 for x in rel if x < -0.005),
            "rel_p10": q(rel, .1), "rel_p50": q(rel, .5)}


def check_censoring(window: list, taskA: list, base: B.Strategy) -> dict:
    """Launches with > 200 trades in their first 60 s never had slot 0 read in the window."""
    rows = W.read_jsonl(DATA / "window_tokens.jsonl")
    cens = [r for r in rows if r.get("trades_mode") == "prefix60" and r.get("covered_until") is None]
    cens_nm = [r for r in cens if not r.get("mayhem")]
    bund = [r for r in rows if r.get("bundled")]
    sol_usd = 107.4

    def ath(rs: list[dict]) -> dict:
        a = [(r.get("ath_usd") or 0) / sol_usd for r in rs]
        return {"n": len(a), "ath_sol_p50": round(q(a, .5), 1), "share_ath_ge_84_sol": round(sum(x >= 84 for x in a) / len(a), 3),
                "graduated": sum(1 for r in rs if r.get("complete"))}
    out = {"censored_launches": len(cens), "censored_non_mayhem": len(cens_nm),
           "censored_non_mayhem_ath": ath(cens_nm), "bundled_sample_ath": ath(bund),
           "window_sample_max_trades_60s": max(n60(t, tr) for t, tr in window)}
    for name, S in (("taskA_hot", [x for x in taskA if n60(*x) > HOT_TRADES_60S]),
                    ("taskA_not_hot", [x for x in taskA if n60(*x) <= HOT_TRADES_60S])):
        s = B.summarize([B.simulate(t, tr, base) for t, tr in S], base.size_sol)
        out[name] = {k: s[k] for k in ("n_trades", "tp_hits", "hit_rate", "expectancy_pct", "expectancy_ci95")}
    return out


def check_payoffs(window: list, base: B.Strategy) -> dict:
    res = [r for r in (B.simulate(t, tr, base) for t, tr in window) if r.get("entered")]
    hit = [r["pnl_sol"] for r in res if r["hit"]]
    miss = [r["pnl_sol"] for r in res if not r["hit"]]
    w, l_ = st.mean(hit), st.mean(miss)
    cov = collections.Counter(t["covered_until"] - t["create_time"] for t, tr in window
                              if B.simulate(t, tr, base).get("exit_reason") == "horizon")
    why = collections.Counter(B.simulate(t, tr, base).get("why") for t, tr in window)
    return {"n": len(res), "tp_hits": len(hit), "tp_payoff_pct": round(100 * w / base.size_sol, 1),
            "non_hit_payoff_pct": round(100 * l_ / base.size_sol, 1),
            "breakeven_tp_hit_rate": round(-l_ / (w - l_), 4),
            "expectancy_pct_at_30pct_tp_hits": round(100 * (0.3 * w + 0.7 * l_) / base.size_sol, 1),
            "horizon_exits_by_coverage_s": dict(cov), "not_entered": {k: v for k, v in why.items() if k}}


# --------------------------------------------------------------------------- network checks
def probe_censored(n: int, base: B.Strategy, window: list, seed: int = 20261014) -> dict:
    """Read slot 0 of n random censored non-mayhem launches (<= 2 pages each) and bound their outcome.

    Lower bound: guru rule on slot 0 + the cached 60 s and 900 s pages, no idle exit (the gaps
    between pages would fake one).  Upper bound: also a TP if the API ATH >= 2.02 x eos0 within 900 s.
    """
    rows = W.read_jsonl(DATA / "window_tokens.jsonl")
    cens = sorted(r["mint"] for r in rows if r.get("trades_mode") == "prefix60"
                  and r.get("covered_until") is None and not r.get("mayhem"))
    recs = {r["mint"]: r for r in W.read_jsonl(DATA / "cache" / "window" / "creates.jsonl") if r["mint"] in set(cens)}
    api = PumpApi(max_rps=1.5)
    noidle = B.variant(base, idle_s=None)
    comp = curve.completion_mcap_sol()
    lo, hi, kinds = [], [], collections.Counter()
    for m in random.Random(seed).sample(cens, min(n, len(cens))):
        r = recs[m]
        t0 = r["create_time"]
        pages = []
        raw0, comp0 = api.trades_before(m, t0 * 1000 + 999, max_pages=2)
        for ts_ms, mp in (((t0 + 60) * 1000 + 999, 2), ((t0 + 900) * 1000 + 999, 1)):  # cached by the window run
            pages += api.trades_before(m, ts_ms, max_pages=mp)[0]
        rec = {"create_slot": r["create_slot"], "create_sig": r["create_sig"], "creator": r["creator"],
               "create_user": r["create_user"], "fee_payer": r.get("fee_payer")}
        slot0 = sorted({(x["sig"], x["sub"], x["user"], x["buy"]): x for x in map(normalize_trade, raw0)
                        if x["mcap"] > 0}.values(), key=lambda x: (x["slot"], x["txi"], x["sub"]))
        s0 = D.slot0_structure(rec, slot0) if comp0 else None
        if not s0:
            kinds["slot0_unreadable"] += 1
            continue
        if not s0["bundled"]:
            kinds["not_bundled"] += 1
            continue
        if s0["eos0_mcap"] >= 0.98 * comp:
            kinds["graduated_in_slot0"] += 1
            continue
        kinds["bundled_on_curve"] += 1
        tr = sorted({(x["sig"], x["sub"], x["user"], x["buy"]): x for x in map(normalize_trade, raw0 + pages)
                     if x["mcap"] > 0 and x["ts"] <= t0 + 900}.values(), key=lambda x: (x["slot"], x["txi"], x["sub"]))
        tok = {"mint": m, "create_slot": r["create_slot"], "create_time": t0, "mayhem": False,
               "covered_until": t0 + 900, "s0": s0}
        res = B.simulate(tok, tr, noidle)
        lo.append(res)
        coin = r.get("coin") or {}
        ath_sol = (coin.get("ath_market_cap") or 0) / 107.4
        ath_dt = (coin.get("ath_market_cap_timestamp") or 0) / 1000 - t0
        up = res
        if res.get("entered") and not res["hit"] and 0 <= ath_dt <= 900 and ath_sol >= 2.02 * s0["eos0_mcap"]:
            up = {**res, "hit": True, "pnl_sol": 0.99 * base.size_sol}
        hi.append(up)
    out: dict = {"censored_non_mayhem": len(cens), "probed": sum(kinds.values()), "kinds": dict(kinds),
                 "api": api.stats}
    wres = [x for x in (B.simulate(t, tr, base) for t, tr in window) if x.get("entered")]
    scale = len(cens) / max(1, sum(kinds.values()))
    for name, R in (("lower", lo), ("upper", hi)):
        s = B.summarize(R, base.size_sol)
        n_all = len(wres) + scale * s["n_trades"]
        hits = sum(x["hit"] for x in wres) + scale * s["tp_hits"]
        pnl = sum(x["pnl_sol"] for x in wres) + scale * sum(x["pnl_sol"] for x in R if x.get("entered"))
        out[name] = {"censored": {k: s[k] for k in ("n_trades", "tp_hits", "hit_rate", "hit_ci95", "expectancy_pct",
                                                     "expectancy_ci95")},
                     "window_reweighted": {"n": round(n_all), "hit_rate": round(hits / n_all, 4),
                                           "expectancy_pct": round(100 * pnl / n_all / base.size_sol, 1)}}
    return out


def spot_check(window: list, n: int, seed: int = 1414) -> list[dict]:
    """Coins-v2 ATH and DexScreener price vs the cached trade path of n random sample tokens."""
    import requests
    s = requests.Session()
    s.headers.update({"accept": "application/json", "origin": "https://pump.fun", "user-agent": "Mozilla/5.0 rugsnipe-verify"})
    out = []
    for tok, tr in random.Random(seed).sample(window, n):
        m = tok["mint"]
        c = s.get(f"https://frontend-api-v3.pump.fun/coins-v2/{m}", timeout=30).json()
        dx = (s.get(f"https://api.dexscreener.com/latest/dex/tokens/{m}", timeout=30).json().get("pairs") or [{}])[0]
        time.sleep(0.5)
        su = st.median([t["sol_usd"] for t in tr if t.get("sol_usd")])
        mx = max(tr, key=lambda t: t["mcap"])
        out.append({"mint": m, "trade_max_mcap_sol": round(mx["mcap"], 3), "trade_max_dt_s": mx["ts"] - tok["create_time"],
                    "api_ath_sol": round(float(c.get("ath_market_cap") or 0) / su, 3),
                    "api_ath_dt_s": round((c.get("ath_market_cap_timestamp") or 0) / 1000 - tok["create_time"]),
                    "api_mcap_sol_now": round(float(c.get("usd_market_cap") or 0) / su, 2),
                    "dexscreener_mcap_sol_now": round(float(dx.get("priceNative") or 0) * 1e9, 2),
                    "dexscreener_dex": dx.get("dexId")})
        log(f"spot {out[-1]}")
    return out


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--probe", type=int, default=0, help="censored launches to probe (<= 2 swap-api pages each, max 48)")
    p.add_argument("--spot", type=int, default=0, help="sample tokens to re-check on coins-v2 + DexScreener")
    args = p.parse_args()
    bt = _bt_script()
    sys.argv = sys.argv[:1]
    a = bt.parse_args()
    api = bt.CacheOnlyApi()
    window, _ = bt.load_window(a, api)
    taskA = [x for x in bt.load_taskA(a, api) if x[0]["s0"]["bundled"]]
    base = B.Strategy()
    res: dict = {"generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                 "units": check_units(window),
                 "curve_and_fees": check_curve_and_fees({t["mint"] for t, _ in window}),
                 "lookahead_features": check_lookahead(window + taskA),
                 "time": check_time(window), "ath": check_ath(window),
                 "censoring": check_censoring(window, taskA, base), "payoffs": check_payoffs(window, base)}
    for k, v in res.items():
        log(f"{k}: {v}")
    if args.probe:
        res["probe"] = probe_censored(min(args.probe, 48), base, window)
        log(f"probe: {res['probe']}")
    if args.spot:
        res["spot_check"] = spot_check(window, args.spot)
    OUT.write_text(json.dumps(res, indent=1, default=str))
    log(f"wrote {OUT}")


if __name__ == "__main__":
    main()
