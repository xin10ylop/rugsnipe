#!/usr/bin/env python3
"""Rugger discovery on a dense window of pump.fun launches (Task C, part 1).

Usage (repo root):
    .venv/bin/python -I analysis/scripts/discover_ruggers.py [--hours 6] [--stages ...]

Stages (all cached / resumable; re-runs take seconds):
  decode   every successful create in [end - hours, end) via RPC getTransaction
  coins    coin snapshots (ATH, complete) via POST /coins/mints
  trades   swap-api trades for SOL-quoted launches that could hold a >= 1 SOL bundle
           (cheap probe first; the full 15-min path only for bundled launches)
  funding  first funder + amount of each create signer (fresh-wallet test)
  cluster  signals a/b/c/d -> clusters (>= 3 launches) -> data/rugger_clusters.json
  expand   pull more launches of the top clusters across the --expand-hours window
           (signal d via /coins?creator=, signal a via the mother wallet's transfers)

Outputs: data/window_tokens.jsonl, data/rugger_clusters.json,
         data/reports/discovery_summary.json
"""
from __future__ import annotations

import argparse
import json
import random
import sys
import time
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from rugsnipe_analysis import discovery as D  # noqa: E402
from rugsnipe_analysis import expand as X  # noqa: E402
from rugsnipe_analysis import window as W  # noqa: E402
from rugsnipe_analysis.cache import REPO_ROOT  # noqa: E402
from rugsnipe_analysis.dataset import INDEX_PATH, log, pmap  # noqa: E402
from rugsnipe_analysis.pumpapi import PumpApi  # noqa: E402
from rugsnipe_analysis.rpc import RpcClient  # noqa: E402

DATA = REPO_ROOT / "data"
WCACHE = DATA / "cache" / "window"
TOKENS_PATH = DATA / "window_tokens.jsonl"
CLUSTERS_PATH = DATA / "rugger_clusters.json"
SUMMARY_PATH = DATA / "reports" / "discovery_summary.json"


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--stages", default="decode,coins,trades,funding,cluster,expand")
    p.add_argument("--hours", type=float, default=6.0, help="dense window length (h)")
    p.add_argument("--end", type=int, default=None, help="window end (unix s); default = create-index anchor")
    p.add_argument("--horizon", type=int, default=900, help="seconds of trades kept per launch")
    p.add_argument("--rpc-rps", type=float, default=8.0)
    p.add_argument("--api-rps", type=float, default=3.0)
    p.add_argument("--workers", type=int, default=8)
    p.add_argument("--funding-scope", choices=("tradable", "all"), default="tradable",
                   help="trace funding for launches that could be bundled (default) or all SOL-quoted")
    p.add_argument("--min-cluster", type=int, default=3)
    p.add_argument("--expand-top", type=int, default=30, help="clusters to expand")
    p.add_argument("--expand-per", type=int, default=25, help="max extra launches per cluster")
    p.add_argument("--expand-hours", type=float, default=72.0)
    p.add_argument("--max-minutes", type=float, default=38.0, help="stop network stages after this")
    return p.parse_args()


class Deadline:
    def __init__(self, minutes: float) -> None:
        self.t_end = time.time() + minutes * 60

    def left(self) -> float:
        return self.t_end - time.time()

    def over(self) -> bool:
        return self.left() <= 0


def load_index() -> dict:
    return json.loads(INDEX_PATH.read_text())


def stage_decode(args, rpc: RpcClient) -> list[dict]:
    idx = load_index()
    end = args.end or idx["anchor_time"]
    sigs = W.window_sigs(idx, end - int(args.hours * 3600), end)
    log(f"decode: {len(sigs)} successful creates in [{time.strftime('%m-%d %H:%M', time.gmtime(end - args.hours * 3600))}, "
        f"{time.strftime('%m-%d %H:%M', time.gmtime(end))}) UTC")
    recs = W.decode_window(rpc, sigs, workers=args.workers)
    W.write_jsonl(WCACHE / "creates.jsonl", recs)
    log(f"decode: {len(recs)} pump.fun creates decoded")
    return recs


def stage_coins(api: PumpApi, recs: list[dict]) -> None:
    W.attach_coins(api, recs)
    W.write_jsonl(WCACHE / "creates.jsonl", recs)
    log(f"coins: {sum(1 for r in recs if r.get('coin'))}/{len(recs)} snapshots")


def stage_trades(args, api: PumpApi, recs: list[dict], dl: Deadline) -> dict[str, dict]:
    """-> mint -> {s0, covered_until, complete, mode, n} (trades themselves stay in the cache)."""
    todo = [r for r in recs if W.could_be_bundled(r)]
    # non-mayhem first (the guru's model), seeded random order inside each group so that a
    # deadline cut leaves an unbiased subsample (re-runs resume from the cache)
    random.Random(20261014).shuffle(todo)
    todo.sort(key=lambda r: bool(r.get("is_mayhem_mode")))
    log(f"trades: {len(todo)}/{len(recs)} launches are SOL-quoted with ATH high enough for a >=1 SOL slot-0 bundle")
    out: dict[str, dict] = {}

    def need_full(prefix: dict, rec: dict) -> bool:
        s0 = D.slot0_structure(rec, prefix["trades"])
        return s0["bundled"]

    def one(rec: dict) -> dict | None:
        if dl.over():
            return None
        res = W.staged_trades(api, rec, lambda p: need_full(p, rec), horizon_s=args.horizon)
        s0 = D.slot0_structure(rec, res["trades"]) if res["trades"] else None
        return {"mint": rec["mint"], "s0": s0, "covered_until": res["covered_until"],
                "complete": res["complete"], "mode": res["mode"], "n": len(res["trades"])}
    for r in pmap(one, todo, workers=args.workers, label="trades", every=250):
        if r:
            out[r["mint"]] = r
    W.write_jsonl(WCACHE / "trades_meta.jsonl", out.values())
    modes = Counter(v["mode"] for v in out.values())
    log(f"trades: {len(out)} pulled; modes {dict(modes)}; bundled {sum(1 for v in out.values() if v['s0'] and v['s0']['bundled'])}")
    return out


def stage_funding(args, rpc: RpcClient, recs: list[dict], tmeta: dict[str, dict], dl: Deadline) -> dict[str, dict]:
    if args.funding_scope == "all":
        scope = [r for r in recs if r.get("sol_quoted")]
    else:
        scope = [r for r in recs if W.could_be_bundled(r)]  # independent of the trades stage
    first_by_wallet: dict[str, dict] = {}
    for r in sorted(scope, key=lambda r: r["create_time"]):
        first_by_wallet.setdefault(r["create_user"], r)
    log(f"funding: {len(first_by_wallet)} distinct signers for {len(scope)} launches")

    def one(item):
        w, r = item
        if dl.over():
            return None
        return w, W.creator_funding(rpc, w, r["create_sig"])
    res = {}
    for x in pmap(one, list(first_by_wallet.items()), workers=args.workers, label="funding", every=250):
        if x:
            res[x[0]] = x[1]
    W.write_jsonl(WCACHE / "funding.jsonl", ({"wallet": w, **f} for w, f in res.items()))
    st = Counter(f["status"] for f in res.values())
    log(f"funding: {dict(st)}; fresh {sum(1 for f in res.values() if f.get('fresh'))}")
    return res


def build_rows(recs: list[dict], tmeta: dict[str, dict], fund: dict[str, dict]) -> list[dict]:
    rows = []
    for r in recs:
        if not r.get("sol_quoted"):
            continue
        tm = tmeta.get(r["mint"])
        row = D.build_token_row(r, tm["s0"] if tm else None, fund.get(r["create_user"]))
        row["trades_mode"] = tm["mode"] if tm else "skipped_ath"
        row["covered_until"] = tm["covered_until"] if tm else None
        rows.append(row)
    return rows


def mark_exchange_like(rows: list[dict], rpc: RpcClient, min_recipients: int = 3) -> None:
    """Re-label busy unlabeled funders ('mother') as 'exchange_like' (> 200 tx/h)."""
    cnt = Counter(r["funder"] for r in rows if r.get("fresh") and r.get("funder_kind") == "mother")
    busy = {}
    for f, n in cnt.items():
        if n >= min_recipients:
            busy[f] = W.funder_rate(rpc, f, "taskc1")
    hot = {f for f, v in busy.items() if v["tph"] > W.EXCHANGE_LIKE_TPH}
    for r in rows:
        if r.get("funder") in hot and r.get("funder_kind") == "mother":
            r["funder_kind"] = "exchange_like"
    log(f"funders: {len(busy)} unlabeled funders with >= {min_recipients} fresh creators checked, "
        f"{len(hot)} exchange-like (> {W.EXCHANGE_LIKE_TPH:.0f} tx/h): "
        + ", ".join(f"{f[:6]}={busy[f]['tph']:.0f}/h" for f in sorted(hot, key=lambda x: -busy[x]['tph'])[:8]))


def compact_row(r: dict) -> dict:
    out = {k: v for k, v in r.items() if k not in ("third_wallets", "name")}
    out["third_n"] = len(r.get("third_wallets") or [])
    return out


def stage_cluster(args, rows: list[dict]) -> tuple[list[dict], dict]:
    clusters, meta = D.cluster(rows, min_size=args.min_cluster)
    by = {r["mint"]: r for r in rows}
    desc = []
    for c in clusters:
        d = D.describe_cluster(c, [by[m] for m in c.mints])
        d["tokens"] = [token_outcome(by[m]) for m in d["mints"]]
        desc.append(d)
    log(f"cluster: {len(clusters)} clusters with >= {args.min_cluster} launches; edges {meta['edges']}")
    return desc, meta


def token_outcome(r: dict) -> dict:
    """Light outcome per launch (the full P&L comes from scripts/backtest.py)."""
    sol_usd = 107.0
    ath_sol = (r["ath_usd"] / sol_usd) if r.get("ath_usd") else None
    e = r.get("eos0_mcap")
    return {"mint": r["mint"], "t": r["create_time"], "signer": r["signer"][:8],
            "fresh": r.get("fresh"), "funder": r.get("funder_label") or (r.get("funder") or "")[:8],
            "fund_amount": r.get("fund_amount"), "bundled": r.get("bundled"),
            "dev_sol": r.get("dev_sol"), "bundle_sizes": r.get("bundle_sizes"),
            "eos0_mcap": round(e, 2) if e else None,
            "ath_usd": round(r["ath_usd"]) if r.get("ath_usd") else None,
            "ath_mult_vs_eos0": round(ath_sol / e, 2) if (ath_sol and e) else None,
            "complete": r.get("complete"), "mayhem": r.get("mayhem")}


def summary(rows: list[dict], clusters: list[dict], meta: dict, tmeta: dict, fund: dict, args) -> dict:
    n = len(rows)
    bundled = [r for r in rows if r.get("bundled")]
    fresh = [r for r in rows if r.get("fresh")]
    fk = Counter(r["funder_kind"] for r in rows if r.get("fresh"))
    in_cluster = {m for c in clusters for m in c["mints"]}
    return {
        "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "window_hours": args.hours, "n_sol_quoted_launches": n,
        "n_trades_pulled": len(tmeta), "n_bundled": len(bundled),
        "bundled_share": round(len(bundled) / n, 4) if n else 0,
        "n_signers_traced": len(fund), "n_fresh_launches": len(fresh),
        "fresh_funder_kinds": dict(fk),
        "clustering": meta,
        "n_clusters": len(clusters),
        "cluster_sizes": dict(Counter(c["n_tokens"] for c in clusters)),
        "launches_in_clusters": len(in_cluster),
        "bundled_in_clusters": sum(1 for r in bundled if r["mint"] in in_cluster),
        "clusters_by_dominant_edge": dict(Counter(max(c["edges"], key=c["edges"].get) if c["edges"] else "none"
                                                  for c in clusters)),
        "scheme": D.scheme_stats(rows),
        "signal_a_chance": [D.amount_chance_test(rows, "cex"), D.amount_chance_test(rows, "mother")],
        "args": vars(args),
    }


def main() -> None:
    args = parse_args()
    st = set(args.stages.split(","))
    dl = Deadline(args.max_minutes)
    rpc = RpcClient(max_rps=args.rpc_rps)
    api = PumpApi(max_rps=args.api_rps)
    t0 = time.time()
    if "decode" in st or not (WCACHE / "creates.jsonl").exists():
        recs = stage_decode(args, rpc)
    else:
        recs = W.read_jsonl(WCACHE / "creates.jsonl")
    if "coins" in st or not all("coin" in r for r in recs):
        stage_coins(api, recs)
    if "trades" in st:
        tmeta = stage_trades(args, api, recs, dl)
    else:
        tmeta = {r["mint"]: r for r in W.read_jsonl(WCACHE / "trades_meta.jsonl")}
    if "funding" in st:
        fund = stage_funding(args, rpc, recs, tmeta, dl)
    else:
        fund = {r.pop("wallet"): r for r in W.read_jsonl(WCACHE / "funding.jsonl")}
    rows = build_rows(recs, tmeta, fund)
    if "cluster" in st or "expand" in st:
        mark_exchange_like(rows, rpc)
    if "cluster" in st or "expand" in st:
        clusters, meta = stage_cluster(args, rows)
        W.write_jsonl(TOKENS_PATH, (compact_row(r) for r in rows))
        exp = {}
        if "expand" in st and not dl.over():
            exp = X.expand_clusters(clusters, rows, api, rpc, top=args.expand_top, per=args.expand_per,
                                    hours=args.expand_hours, horizon_s=args.horizon,
                                    deadline=dl.t_end, cache_dir=WCACHE)
        for c in clusters:
            c["expanded"] = exp.get(c["cluster"], {"status": "not_expanded"})
        CLUSTERS_PATH.write_text(json.dumps({"generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                                             "window_hours": args.hours, "clusters": clusters}, indent=1))
        s = summary(rows, clusters, meta, tmeta, fund, args)
        s["expansion"] = {k: {kk: vv for kk, vv in v.items() if kk != "tokens"} for k, v in exp.items()}
        SUMMARY_PATH.parent.mkdir(parents=True, exist_ok=True)
        SUMMARY_PATH.write_text(json.dumps(s, indent=1))
        log(f"wrote {TOKENS_PATH.name} ({len(rows)} rows), {CLUSTERS_PATH.name} ({len(clusters)} clusters), "
            f"{SUMMARY_PATH.name}")
        for c in clusters[:15]:
            hits = sum(1 for t in c["tokens"] if (t["ath_mult_vs_eos0"] or 0) >= 2)
            log(f"  {c['cluster']} score {c['score']:.2f} n={c['n_tokens']} edges={c['edges']} "
                f"bundled={c['bundled_share']:.0%} fresh={c['fresh_share']:.0%} ATH>=2x eos0: {hits}/{c['n_tokens']}")
    log(f"done in {time.time() - t0:.0f}s rpc={rpc.stats} api={api.stats}")


if __name__ == "__main__":
    main()
