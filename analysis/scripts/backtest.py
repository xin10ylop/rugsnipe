#!/usr/bin/env python3
"""Backtest the guru's end-of-creation-slot snipe and variants on recorded trades (Task C, part 2).

Usage (repo root):
    .venv/bin/python -I analysis/scripts/backtest.py [--quick] [--no-taskA] [--allow-fetch]

Samples:
  window    every SOL-quoted launch of the dense discovery window
            (data/window_tokens.jsonl from discover_ruggers.py); only bundled
            launches are tradable for the guru's rule
  clusters  launches in discovered clusters (window + expanded launches from
            data/rugger_clusters.json), incl. a walk-forward "follow the good
            rugger" selection
  taskA     Task A's random arm (data/tokens_sample.jsonl, Oct 12-14), a
            replication sample with a 300 s horizon

Trades are read from the disk cache (data/cache/pumpapi); --allow-fetch lets the
script pull missing pages (rate-limited).  Outputs:
  data/reports/backtest_results.json, data/backtest_trades.csv,
  docs/research/onchain-backtest.md
"""
from __future__ import annotations

import argparse
import csv
import json
import sys
import time
from collections import defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from rugsnipe_analysis import backtest as B  # noqa: E402
from rugsnipe_analysis import dataset as ds  # noqa: E402
from rugsnipe_analysis import discovery as D  # noqa: E402
from rugsnipe_analysis import report as R  # noqa: E402
from rugsnipe_analysis import window as W  # noqa: E402
from rugsnipe_analysis.cache import REPO_ROOT  # noqa: E402
from rugsnipe_analysis.pumpapi import PumpApi  # noqa: E402

DATA = REPO_ROOT / "data"
OUT_JSON = DATA / "reports" / "backtest_results.json"
OUT_CSV = DATA / "backtest_trades.csv"
OUT_MD = REPO_ROOT / "docs" / "research" / "onchain-backtest.md"


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--tokens", default=str(DATA / "window_tokens.jsonl"))
    p.add_argument("--clusters", default=str(DATA / "rugger_clusters.json"))
    p.add_argument("--no-taskA", action="store_true", help="skip the Task A random-arm replication")
    p.add_argument("--horizon", type=int, default=900)
    p.add_argument("--allow-fetch", action="store_true", help="fetch trade pages missing from the cache")
    p.add_argument("--api-rps", type=float, default=2.0)
    p.add_argument("--quick", action="store_true", help="small grid (smoke test)")
    p.add_argument("--min-n", type=int, default=30, help="min trades for a variant to be ranked")
    return p.parse_args()


# --------------------------------------------------------------------------- loading
class CacheOnlyApi(PumpApi):
    """PumpApi that refuses network calls: a cache miss raises CacheMiss (the launch is skipped),
    so a missing page can never masquerade as 'end of history'."""

    def get_json(self, url, params=None, cache_key=None):  # type: ignore[override]
        if cache_key:
            hit = self.cache.get(cache_key)
            if hit is not None:
                self.stats["cache_hits"] += 1
                return hit["v"]
        self.stats["misses"] = self.stats.get("misses", 0) + 1
        raise CacheMiss(cache_key or url)


class CacheMiss(LookupError):
    pass


def safe_trades(fn, *a, **kw) -> dict | None:
    try:
        return fn(*a, **kw)
    except CacheMiss:
        return None


def token_from_row(row: dict, trades_res: dict, sample: str) -> tuple[dict, list[dict]] | None:
    trades = trades_res["trades"]
    if not trades or not trades_res.get("complete"):
        return None
    rec = {"create_slot": row["create_slot"], "create_sig": row["create_sig"], "creator": row["creator"],
           "create_user": row["signer"], "fee_payer": row.get("fee_payer") or row["signer"]}
    trades = [t for t in trades if t["slot"] >= row["create_slot"]]
    if not trades:
        return None
    tok = {"mint": row["mint"], "sample": sample, "create_slot": row["create_slot"],
           "create_sig": row["create_sig"], "creator": row["creator"], "create_user": row["signer"],
           "fee_payer": rec["fee_payer"], "create_time": row["create_time"], "mayhem": bool(row.get("mayhem")),
           "covered_until": trades_res["covered_until"], "signer": row["signer"],
           "s0": D.slot0_structure(rec, trades)}
    return tok, trades


def load_window(args, api: PumpApi) -> tuple[list[tuple[dict, list[dict]]], dict]:
    path = Path(args.tokens)
    if not path.exists():
        ds.log(f"window: {path} missing (run discover_ruggers.py first)")
        return [], {}
    rows = W.read_jsonl(path)
    for r in rows:  # compact rows omit creator / fee_payer when equal to the signer
        r.setdefault("creator", r["signer"])
        r.setdefault("fee_payer", r["signer"])
    stats = {"rows": len(rows), "bundled_rows": sum(1 for r in rows if r.get("bundled"))}
    out = []
    for r in rows:
        if not r.get("bundled"):
            continue  # not tradable for the guru's rule (bundle proven absent or < 1 SOL)
        rec = {"mint": r["mint"], "create_time": r["create_time"], "create_slot": r["create_slot"],
               "create_sig": r["create_sig"], "creator": r["creator"], "create_user": r["signer"],
               "fee_payer": r.get("fee_payer")}
        res = safe_trades(W.staged_trades, api, rec, lambda p: True, horizon_s=args.horizon)
        if res is None:
            stats["cache_miss"] = stats.get("cache_miss", 0) + 1
            continue
        t = token_from_row(r, res, "window")
        if t:
            t[0]["ath_usd"] = r.get("ath_usd")
            out.append(t)
    stats["loaded"] = len(out)
    ds.log(f"window: {stats}")
    return out, stats


def load_clusters(args, api: PumpApi, window_tokens: list[tuple[dict, list[dict]]]):
    path = Path(args.clusters)
    if not path.exists():
        return {}, {}
    cl = json.loads(path.read_text())["clusters"]
    by_mint = {t["mint"]: (t, tr) for t, tr in window_tokens}
    members: dict[str, list[tuple[dict, list[dict]]]] = {}
    extra = 0
    for c in cl:
        lst = []
        for m in c["mints"]:
            if m in by_mint:
                lst.append(by_mint[m])
        for et in (c.get("expanded") or {}).get("tokens", []) or []:
            if not et.get("s0") or not et["s0"].get("bundled"):
                continue
            rec = {"mint": et["mint"], "create_time": et["create_time"], "create_slot": et["create_slot"],
                   "create_sig": et["create_sig"], "creator": et["creator"], "create_user": et["signer"],
                   "fee_payer": et["signer"]}
            res = safe_trades(W.staged_trades, api, rec, lambda p: True, horizon_s=args.horizon)
            if res is None:
                continue
            row = {**et, "fee_payer": et["signer"]}
            t = token_from_row(row, res, "expanded")
            if t:
                lst.append(t)
                extra += 1
        members[c["cluster"]] = sorted(lst, key=lambda x: x[0]["create_time"])
    meta = {c["cluster"]: {k: c.get(k) for k in ("score", "n_tokens", "edges", "funder_kinds", "top_funders",
                                                  "bundle_shapes", "fresh_share", "bundled_share")} for c in cl}
    for c in cl:
        meta[c["cluster"]]["type"] = cluster_type(c)
        for m in c["mints"]:
            MINT_CLUSTER[m] = (c["cluster"], meta[c["cluster"]]["type"])
    ds.log(f"clusters: {len(cl)} clusters, {sum(len(v) for v in members.values())} tradable members "
           f"({extra} from expansion)")
    return members, meta


MINT_CLUSTER: dict[str, tuple[str, str]] = {}


def cluster_type(c: dict) -> str:
    """Dominant evidence of a cluster: serial (same signer), mother, exchange+shape, bundle wallets, shape."""
    e = c.get("edges") or {}
    if not e:
        return "none"
    k = max(e, key=e.get)
    if k == "d" and c.get("fresh_share", 0) < 0.5:
        return "serial_signer"
    return {"d": "rotating_signer", "a_mother": "mother_funded", "a_cex_shape": "cex_funded_same_shape",
            "c": "shared_bundle_wallets", "b_strong": "same_bundle_shape",
            "b_weak": "same_round_bundle_shape"}.get(k, k)


def by_group(tokens, strat: B.Strategy) -> list[dict]:
    """Guru base rule split by the launch's cluster type (window sample)."""
    groups: dict[str, list] = defaultdict(list)
    for tok, tr in tokens:
        groups[MINT_CLUSTER.get(tok["mint"], ("-", "no_cluster"))[1]].append((tok, tr))
    out = []
    for g, lst in sorted(groups.items(), key=lambda x: -len(x[1])):
        s = B.summarize(run(lst, strat), strat.size_sol)
        out.append({"group": g, **{k: s[k] for k in ("n_trades", "tp_hits", "hit_rate", "hit_ci95", "expectancy_sol",
                                                       "expectancy_pct", "expectancy_ci95")}})
    return out


def load_taskA(args, api: PumpApi) -> list[tuple[dict, list[dict]]]:
    deep = [r for r in W.read_jsonl(DATA / "tokens_sample.jsonl") if r["arm"] == "random"]
    light = {r["mint"]: r for r in W.read_jsonl(DATA / "tokens_light.jsonl")}
    out = []
    for r in deep:
        if r.get("instant_graduation"):
            continue
        res = safe_trades(ds.pull_trades, api, r["mint"], r["create_time"], 300, 20, 6)
        if res is None:
            continue
        trades = res["trades"]
        if not trades or res["coverage"] == "partial":
            continue
        if res["coverage"] == "full":
            cov = r["create_time"] + args.horizon
        else:
            cov = r["create_time"] + 300
            trades = [t for t in trades if t["ts"] <= cov]
        lr = light.get(r["mint"], {})
        row = {"mint": r["mint"], "create_slot": r["create_slot"], "create_sig": r["create_sig"],
               "creator": r["creator"], "signer": r["create_user"], "fee_payer": r["fee_payer"],
               "create_time": r["create_time"], "mayhem": lr.get("is_mayhem_mode", r.get("is_mayhem_mode"))}
        t = token_from_row(row, {"trades": trades, "complete": True, "covered_until": cov}, "taskA")
        if t:
            out.append(t)
    ds.log(f"taskA random arm: {len(out)} tokens with a contiguous trade prefix "
           f"({sum(1 for t, _ in out if t['s0']['bundled'])} bundled)")
    return out


# --------------------------------------------------------------------------- grids
def grid(quick: bool) -> list[B.Strategy]:
    base = B.Strategy()
    out = []
    tps = (1.0, 1.5, 2.0)
    sls = (None, 0.3, 0.5)
    idles = (150, None)
    bs = (None, 3.0, 5.0, 10.0)
    entries = ("eos0", "eos1")
    if quick:
        tps, sls, idles, bs = (1.0,), (None, 0.5), (150,), (None, 5.0)
    for e in entries:
        for tp in tps:
            for sl in sls:
                for idle in idles:
                    for b in bs:
                        out.append(B.variant(base, entry=e, tp=tp, sl=sl, idle_s=idle, max_bundle_sol=b))
    # special exits on the guru base (eos0, TP100, floor stop, idle 150)
    for arm, drop in ((0.3, 0.2), (0.5, 0.3), (0.5, 0.2), (1.0, 0.3)):
        out.append(B.variant(base, tp=None, trail_arm=arm, trail_drop=drop))
        out.append(B.variant(base, tp=1.0, trail_arm=arm, trail_drop=drop))
    for n in (2.0, 5.0, 10.0):
        out.append(B.variant(base, big_buy_sol=n))
    for o in ("max", "pre_dump"):
        out.append(B.variant(base, oracle=o))
        out.append(B.variant(base, oracle=o, entry="eos1"))
    for lat in (0, 2):
        out.append(B.variant(base, exit_latency_slots=lat))
    for prem in (0.05, 0.15, 0.30):
        out.append(B.variant(base, entry_premium=prem))
    out.append(B.variant(base, skip_mayhem=True))
    for mc in (35.0, 45.0, 60.0, 140.0):
        out.append(B.variant(base, max_entry_mcap=mc))
    for bmin in (3.0, 5.0, 10.0):
        out.append(B.variant(base, min_bundle_sol=bmin))
    return out


def sensitivity(strats: list[B.Strategy]) -> list[B.Strategy]:
    out = []
    for s in strats:
        for size in (0.1, 0.25, 0.5, 1.0):
            for cost in (0.002, 0.01):
                out.append(B.variant(s, size_sol=size, attempt_cost=cost))
    return out


# --------------------------------------------------------------------------- evaluation
def run(tokens: list[tuple[dict, list[dict]]], strat: B.Strategy) -> list[dict]:
    res = []
    for tok, trades in tokens:
        r = B.simulate(tok, trades, strat)
        r["mint"] = tok["mint"]
        r["t"] = tok["create_time"]
        res.append(r)
    return res


def evaluate(tokens, strats, size_key=True) -> list[dict]:
    rows = []
    for s in strats:
        res = run(tokens, s)
        rows.append({"label": s.label(), "strategy": B.as_dict(s), **B.summarize(res, s.size_sol)})
    return rows


def walk_forward(members: dict[str, list], strat: B.Strategy, min_prev: int, min_hit: float) -> dict:
    """Trade a cluster's launch only after >= min_prev earlier, already-closed snipes with hit rate >= min_hit."""
    taken = []
    clusters_used = set()
    for cid, lst in members.items():
        hist = []  # (close_time, hit)
        for tok, trades in lst:
            r = B.simulate(tok, trades, strat)
            if not r.get("entered"):
                continue
            t = tok["create_time"]
            closed = [h for (ct, h) in hist if ct < t]
            if len(closed) >= min_prev and sum(closed) / len(closed) >= min_hit:
                taken.append({**r, "cluster": cid, "mint": tok["mint"], "t": t})
                clusters_used.add(cid)
            hist.append((t + r["hold_s"], r["hit"]))
    return {"min_prev": min_prev, "min_hit": min_hit, "clusters_traded": len(clusters_used),
            **B.summarize(taken, strat.size_sol)}


def per_cluster(members: dict[str, list], strat: B.Strategy, meta: dict) -> list[dict]:
    out = []
    for cid, lst in members.items():
        res = run(lst, strat)
        s = B.summarize(res, strat.size_sol)
        out.append({"cluster": cid, "n_members_tradable": len(lst), **{k: meta.get(cid, {}).get(k) for k in
                    ("score", "n_tokens", "edges", "top_funders", "bundle_shapes")},
                    **{k: s[k] for k in ("n_trades", "tp_hits", "hit_rate", "avg_win_sol", "avg_loss_sol",
                                         "win_loss_ratio", "expectancy_sol", "expectancy_pct", "total_pnl_sol",
                                         "guru_hit_ge_33", "guru_wl_ge_3")}})
    out.sort(key=lambda x: (x["n_trades"] == 0, -x["expectancy_sol"]))  # clusters without snipes last
    return out


def heterogeneity(pc: list[dict], min_trades: int = 5, guru_hit: float = 0.33) -> dict:
    """Do clusters differ in TP hit rate more than chance?

    Under H0 every snipe hits TP with the pooled probability p.  Compares the
    observed number of clusters with hit rate >= 33 % to its expectation under H0,
    and counts clusters whose own binomial upper-tail p-value < 0.05.
    """
    from math import ceil, comb
    big = [c for c in pc if c["n_trades"] >= min_trades]
    n = sum(c["n_trades"] for c in big)
    k = sum(c["tp_hits"] for c in big)
    if not n:
        return {}
    p = k / n

    def tail(m: int, j: int) -> float:
        return sum(comb(m, i) * p ** i * (1 - p) ** (m - i) for i in range(j, m + 1))
    exp_33 = sum(tail(c["n_trades"], ceil(guru_hit * c["n_trades"])) for c in big)
    obs_33 = sum(1 for c in big if c["hit_rate"] >= guru_hit)
    sig = sum(1 for c in big if tail(c["n_trades"], c["tp_hits"]) < 0.05 and c["tp_hits"] > 0)
    return {"clusters": len(big), "trades": n, "pooled_hit_rate": round(p, 4),
            "observed_clusters_hit_ge_33": obs_33, "expected_under_chance": round(exp_33, 2),
            "clusters_with_binomial_p_lt_0.05": sig, "expected_false_positives": round(0.05 * len(big), 2)}


def split_half(tokens, strats, min_n: int) -> dict:
    """Pick the best variant on the first half (time-ordered), report it on the second half."""
    toks = sorted(tokens, key=lambda x: x[0]["create_time"])
    a, b = toks[:len(toks) // 2], toks[len(toks) // 2:]
    ev_a = [e for e in evaluate(a, strats) if e["n_trades"] >= max(10, min_n // 2)]
    if not ev_a:
        return {}
    best = max(ev_a, key=lambda e: e["expectancy_sol"])
    s = next(x for x in strats if x.label() == best["label"])
    ev_b = evaluate(b, [s])[0]
    return {"n_first": len(a), "n_second": len(b), "chosen": best["label"],
            "first_half": {k: best[k] for k in ("n_trades", "hit_rate", "expectancy_sol", "expectancy_pct")},
            "second_half": {k: ev_b[k] for k in ("n_trades", "hit_rate", "expectancy_sol", "expectancy_pct",
                                                 "expectancy_ci95")}}


def potential(tokens) -> dict:
    """Guru's 'gain potential': max price within the horizon >= 2x the end-of-slot-0 price (no costs)."""
    n = k = 0
    for tok, trades in tokens:
        if not tok["s0"]["bundled"]:
            continue
        e = tok["s0"]["eos0_mcap"]
        mx = max((t["mcap"] for t in trades if t["slot"] > tok["create_slot"]), default=e)
        n += 1
        k += mx >= 2 * e
    p, lo, hi = B.wilson(k, n)
    return {"n_bundled": n, "reach_2x_eos0": k, "rate": round(p, 4), "ci95": [round(lo, 4), round(hi, 4)]}


def late_hit_bound(tokens, strat: B.Strategy, sol_usd: float = 107.7) -> dict:
    """Upper bound for TPs the 900 s horizon / idle exit misses: launches whose all-time ATH
    (coin API, USD) is >= (1+tp) x eos0 but that did not hit TP in the simulation are
    re-scored as TP wins at the average simulated TP payoff."""
    res = [(tok, B.simulate(tok, tr, strat)) for tok, tr in tokens]
    res = [(t, r) for t, r in res if r.get("entered")]
    tp = [r["pnl_sol"] for _, r in res if r["hit"]]
    if not res or not tp:
        return {}
    avg_tp = sum(tp) / len(tp)
    late = [(t, r) for t, r in res if not r["hit"] and t.get("ath_usd")
            and t["ath_usd"] / sol_usd >= (1 + strat.tp) * t["s0"]["eos0_mcap"]]
    pn = [r["pnl_sol"] for _, r in res]
    adj = sum(pn) + sum(avg_tp - r["pnl_sol"] for _, r in late)
    return {"label": strat.label(), "n": len(res), "tp_in_horizon": len(tp), "late_ath_hits": len(late),
            "expectancy_sol": round(sum(pn) / len(pn), 5),
            "expectancy_if_late_hits_were_tp_sol": round(adj / len(pn), 5),
            "hit_rate_upper": round((len(tp) + len(late)) / len(res), 4)}


def main() -> None:
    args = parse_args()
    t0 = time.time()
    api = PumpApi(max_rps=args.api_rps) if args.allow_fetch else CacheOnlyApi()
    window, wstats = load_window(args, api)
    members, cmeta = load_clusters(args, api, window)
    taskA = [] if args.no_taskA else load_taskA(args, api)
    cl_tokens = [x for lst in members.values() for x in lst]
    samples = {"window": window, "clusters": cl_tokens, "taskA": taskA}
    strats = grid(args.quick)
    ds.log(f"grid: {len(strats)} variants; samples " + ", ".join(f"{k}={len(v)}" for k, v in samples.items()))
    base = B.Strategy()

    feats = lambda tok, tr: D.slot0_structure(tok, tr)  # noqa: E731
    look = B.check_no_lookahead(window + taskA + cl_tokens,
                                [base, B.variant(base, entry="eos1"), B.variant(base, max_bundle_sol=5.0)],
                                features_fn=feats)
    ds.log(f"look-ahead check: {look}")
    assert look["ok"], "entry decisions changed when the future was removed/scrambled"

    results: dict = {"generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                     "args": vars(args), "lookahead_check": look, "window_stats": wstats,
                     "sample_sizes": {k: {"tokens": len(v), "bundled": sum(1 for t, _ in v if t["s0"]["bundled"])}
                                      for k, v in samples.items()},
                     "assumptions": R.ASSUMPTIONS, "samples": {}}
    for name, toks in samples.items():
        if not toks:
            continue
        ev = evaluate(toks, strats)
        ranked = sorted((e for e in ev if e["n_trades"] >= args.min_n and not e["strategy"]["oracle"]),
                        key=lambda e: -e["expectancy_sol"])
        oracles = [e for e in ev if e["strategy"]["oracle"]]
        top = [next(s for s in strats if s.label() == e["label"]) for e in ranked[:5]]
        sens = evaluate(toks, sensitivity([base] + top[:3]))
        results["samples"][name] = {
            "potential": potential(toks),
            "base": next(e for e in ev if e["label"] == base.label()),
            "grid": ev, "top": ranked[:10], "oracles": oracles, "sensitivity": sens,
            "split_half": split_half(toks, [s for s in strats if not s.oracle], args.min_n),
        }
        b = results["samples"][name]["base"]
        ds.log(f"[{name}] base {b['label']}: n={b['n_trades']} hit={b['hit_rate']:.1%} "
               f"exp={b['expectancy_sol']:+.4f} SOL ({b['expectancy_pct']:+.1f}%)")
        for e in ranked[:5]:
            ds.log(f"[{name}]   top {e['label']}: n={e['n_trades']} hit={e['hit_rate']:.1%} "
                   f"exp={e['expectancy_sol']:+.4f} SOL ({e['expectancy_pct']:+.1f}%) CI {e['expectancy_ci95']}")
    if window and MINT_CLUSTER:
        results["by_cluster_type"] = by_group(window, base)
        for g in results["by_cluster_type"]:
            ds.log(f"[by type] {g['group']}: n={g['n_trades']} hit={g['hit_rate']:.1%} exp={g['expectancy_pct']:+.1f}%")
    if window:
        results["late_hit_bound"] = [late_hit_bound(window, s) for s in
                                     (base, B.variant(base, tp=1.5), B.variant(base, tp=2.0))]
        ds.log(f"late-hit bound: {results['late_hit_bound'][0]}")
    if members:
        wf = []
        for s in [base, B.variant(base, tp=0.5), B.variant(base, tp=1.0, sl=0.3), B.variant(base, entry="eos1")]:
            for mp, mh in ((2, 0.33), (3, 0.33), (5, 0.33), (3, 0.5)):
                wf.append({"label": s.label(), **walk_forward(members, s, mp, mh)})
        pcb = per_cluster(members, base, cmeta)
        results["clusters"] = {"per_cluster_base": pcb, "walk_forward": wf, "heterogeneity": heterogeneity(pcb)}
        ds.log(f"[clusters] heterogeneity: {results['clusters']['heterogeneity']}")
        for w in wf[:4]:
            ds.log(f"[walk-forward] {w['label']} prev>={w['min_prev']} hit>={w['min_hit']}: n={w['n_trades']} "
                   f"hit={w['hit_rate']:.1%} exp={w['expectancy_sol']:+.4f}")
    OUT_JSON.parent.mkdir(parents=True, exist_ok=True)
    slim = json.loads(json.dumps(results))
    for v in slim["samples"].values():
        v["grid"] = [{k: e[k] for k in ("label", "n_trades", "tp_hits", "hit_rate", "win_rate", "avg_win_sol",
                                         "avg_loss_sol", "win_loss_ratio", "expectancy_sol", "expectancy_ci95",
                                         "expectancy_pct", "total_pnl_sol", "max_losing_streak", "exit_reasons")}
                     for e in v["grid"]]
    OUT_JSON.write_text(json.dumps(slim, indent=1))
    with OUT_CSV.open("w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["sample", "mint", "create_time", "bundle_sol", "bundle_wallets", "eos0_mcap", "mayhem",
                    "entered", "exit_reason", "hold_s", "pnl_sol", "pnl_frac", "peak_mult"])
        for name, toks in samples.items():
            for tok, trades in toks:
                r = B.simulate(tok, trades, base)
                s0 = tok["s0"]
                w.writerow([name, tok["mint"], tok["create_time"], s0["bundle_sol"], len(s0["bundle_wallets"]),
                            round(s0["eos0_mcap"] or 0, 3), int(tok["mayhem"]), int(r["entered"]),
                            r.get("exit_reason", r.get("why")), r.get("hold_s"),
                            round(r.get("pnl_sol", 0), 5), round(r.get("pnl_frac", 0), 4),
                            round(r.get("peak_mult", 0), 3)])
    verdict, notes = R.load_narrative()
    OUT_MD.write_text(R.merge_narrative(R.render(slim), verdict, notes))
    ds.log(f"wrote {OUT_JSON}, {OUT_CSV}, {OUT_MD} in {time.time() - t0:.0f}s; api {api.stats}")


if __name__ == "__main__":
    main()
