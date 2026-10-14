#!/usr/bin/env python3
"""Pull the pump.fun token sample and compute first-slot statistics.

Usage (from repo root):
    .venv/bin/python -I analysis/scripts/pull_dataset.py [--stages index,light,deep,stats]

Every network response is cached under data/cache/, so the script is resumable:
re-running skips everything already fetched. Outputs:
    data/tokens_sample.jsonl               one row per deep-sampled token (metrics)
    data/tokens_light.jsonl                light sample (creation + coin snapshot)
    data/reports/firstslot_stats.json      aggregate statistics
    docs/research/onchain-firstslot-stats.md   readable report
"""
from __future__ import annotations

import argparse
import json
import random
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from rugsnipe_analysis import dataset as ds  # noqa: E402
from rugsnipe_analysis import firstslot as fs  # noqa: E402
from rugsnipe_analysis.cache import REPO_ROOT  # noqa: E402
from rugsnipe_analysis.pumpapi import PumpApi  # noqa: E402
from rugsnipe_analysis.rpc import RpcClient  # noqa: E402

DATA = REPO_ROOT / "data"
LIGHT_PATH = DATA / "tokens_light.jsonl"
DEEP_PATH = DATA / "tokens_sample.jsonl"
STATS_PATH = DATA / "reports" / "firstslot_stats.json"
REPORT_PATH = REPO_ROOT / "docs" / "research" / "onchain-firstslot-stats.md"
INTERESTING_USD = 20_000


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--stages", default="index,light,deep,stats")
    p.add_argument("--hours", type=float, default=48.0, help="population window (h)")
    p.add_argument("--min-age-hours", type=float, default=2.0, help="exclude tokens younger than this")
    p.add_argument("--n-light", type=int, default=1500, help="light sample size")
    p.add_argument("--n-random", type=int, default=400, help="deep random-arm size")
    p.add_argument("--n-interesting", type=int, default=120, help="deep interesting-arm target")
    p.add_argument("--early-secs", type=int, default=300)
    p.add_argument("--early-pages", type=int, default=20)
    p.add_argument("--latest-pages", type=int, default=6)
    p.add_argument("--seed", type=int, default=20261014)
    p.add_argument("--rpc-rps", type=float, default=8.0)
    p.add_argument("--api-rps", type=float, default=4.0)
    p.add_argument("--refresh-index", action="store_true")
    return p.parse_args()


def stage_light(args, rpc: RpcClient, api: PumpApi) -> list[dict]:
    idx = ds.build_create_index(rpc, args.hours, refresh=args.refresh_index)
    picks = ds.sample_creates(idx, args.n_light, args.min_age_hours, args.seed)
    ds.log(f"light: decoding {len(picks)} creation txs via RPC")
    recs = ds.pmap(lambda s: ds.creation_record(rpc, s["sig"]), picks, workers=6, label="create-tx")
    rows = []
    for rank, (pick, rec) in enumerate(zip(picks, recs)):
        if rec is None:
            continue
        rec["sample_rank"] = rank
        rows.append(rec)
    ds.log(f"light: {len(rows)}/{len(picks)} decoded as pump.fun creates; fetching coin snapshots")
    coins = api.get_coins([r["mint"] for r in rows])
    ds.log(f"light: {len(coins)} coin snapshots")
    for r in rows:
        r["coin"] = ds.compact_coin(coins.get(r["mint"]))
        r["coin_fetched_at"] = api.coin_fetched_at(r["mint"])
        r["arm"] = "light"
    n = ds.write_jsonl(LIGHT_PATH, rows)
    ds.log(f"light: wrote {n} rows -> {LIGHT_PATH}")
    return rows


def load_light() -> list[dict]:
    return [json.loads(l) for l in LIGHT_PATH.read_text().splitlines() if l.strip()]


def interesting_from_graduated(args, rpc: RpcClient, api: PumpApi, idx: dict,
                               exclude: set[str]) -> list[dict]:
    """Graduated coins created inside the window (top-up for the interesting arm).

    The list endpoint is live, so the first result is snapshotted to
    data/cache/graduated_window.json and reused (keeps the sample reproducible).
    """
    snap = ds.CACHE_DIR / "graduated_window.json"
    if snap.exists():
        found = json.loads(snap.read_text())
        ds.log(f"interesting: reusing snapshot of {len(found)} graduated coins")
        return [c for c in found if c["mint"] not in exclude]
    lo = (idx["anchor_time"] - int(args.hours * 3600)) * 1000
    hi = (idx["anchor_time"] - int(args.min_age_hours * 3600)) * 1000
    found: list[dict] = []
    for off in range(0, 1000, 50):
        page = api.list_coins(offset=off, limit=50, complete=True)
        if not page:
            break
        found += [c for c in page if lo <= c["created_timestamp"] <= hi and c["mint"] not in exclude
                  and c.get("quote_mint", "11111111111111111111111111111111") == "11111111111111111111111111111111"]
        if page[-1]["created_timestamp"] < lo:
            break
    ds.log(f"interesting: {len(found)} graduated coins created in window from /coins?complete=true")
    snap.write_text(json.dumps(found))
    return found


def stage_deep(args, rpc: RpcClient, api: PumpApi) -> list[dict]:
    light = load_light()
    idx = ds.build_create_index(rpc, args.hours)
    sol = sorted((r for r in light if r.get("sol_quoted")), key=lambda r: r["sample_rank"])
    ds.log(f"deep: {len(sol)}/{len(light)} light tokens are SOL-quoted (others excluded)")
    random_arm = sol[:args.n_random]
    for r in random_arm:
        r["arm"] = "random"
    light_int = [r for r in sol[args.n_random:] if r.get("coin")
                 and (r["coin"].get("ath_market_cap") or 0) >= INTERESTING_USD]
    for r in light_int:
        r["arm"] = "interesting_light"
    ds.log(f"deep: random arm {len(random_arm)}, interesting from light {len(light_int)}")
    need = max(0, args.n_interesting - len(light_int))
    grad_rows: list[dict] = []
    if need:
        exclude = {r["mint"] for r in light}
        grads = interesting_from_graduated(args, rpc, api, idx, exclude)
        random.Random(args.seed).shuffle(grads)
        grads = grads[:need]

        def build(c: dict) -> dict | None:
            sig = ds.find_creation_sig(rpc, api, c["mint"], c["created_timestamp"])
            if not sig:
                return None
            rec = ds.creation_record(rpc, sig)
            if rec is None or not rec["sol_quoted"]:
                return None
            rec["coin"] = ds.compact_coin(api.get_coin(c["mint"]))
            rec["coin_fetched_at"] = api.coin_fetched_at(c["mint"])
            rec["arm"] = "interesting_graduated"
            rec["sample_rank"] = None
            return rec
        grad_rows = [r for r in ds.pmap(build, grads, workers=4, label="grad-create") if r]
        ds.log(f"deep: {len(grad_rows)} graduated tokens resolved to creation txs")
    deep = random_arm + light_int + grad_rows
    ds.log(f"deep: pulling trades for {len(deep)} tokens")

    def trades_for(r: dict) -> dict:
        return ds.pull_trades(api, r["mint"], r["create_time"], args.early_secs,
                              args.early_pages, args.latest_pages)
    tr = ds.pmap(trades_for, deep, workers=5, label="trades", every=50)
    rows = []
    for r, t in zip(deep, tr):
        if t is None:
            continue
        m = fs.token_metrics(r, t["trades"], t["coverage"])
        m["n_trades_fetched"] = len(t["trades"])
        rows.append(m)
    n = ds.write_jsonl(DEEP_PATH, rows)
    ds.log(f"deep: wrote {n} token rows -> {DEEP_PATH}")
    return rows


def stage_stats(args) -> None:
    deep = [json.loads(l) for l in DEEP_PATH.read_text().splitlines() if l.strip()]
    light = load_light()
    stats = fs.aggregate(deep, light, meta={
        "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "args": vars(args),
    })
    STATS_PATH.parent.mkdir(parents=True, exist_ok=True)
    STATS_PATH.write_text(json.dumps(stats, indent=1))
    ds.log(f"stats -> {STATS_PATH}")
    tables = STATS_PATH.parent / "firstslot_tables.md"
    tables.write_text(fs.render_tables(stats))
    ds.log(f"tables -> {tables}")
    fs.print_summary(stats)


def main() -> None:
    args = parse_args()
    stages = set(args.stages.split(","))
    rpc = RpcClient(max_rps=args.rpc_rps)
    api = PumpApi(max_rps=args.api_rps)
    t0 = time.time()
    if "index" in stages:
        ds.build_create_index(rpc, args.hours, refresh=args.refresh_index)
    if "light" in stages:
        stage_light(args, rpc, api)
    if "deep" in stages:
        stage_deep(args, rpc, api)
    if "stats" in stages:
        stage_stats(args)
    ds.log(f"done in {time.time() - t0:.0f}s; rpc={rpc.stats} api={api.stats}")


if __name__ == "__main__":
    main()
