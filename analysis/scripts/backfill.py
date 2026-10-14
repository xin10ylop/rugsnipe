"""Historical backfill: signature index -> seeded sample -> getTransaction (+ getBlock) -> analytics.db.

Examples
--------
    # smoke: 2 days at 0.5 %, with the slot s and s+1 blocks of each sampled create
    .venv/bin/python -I analysis/scripts/backfill.py --local --days 2 --pct 0.5 --blocks 2

    # the study set (section 9.1): 28 days at 5 % plus every create of some candidate creators
    .venv/bin/python -I analysis/scripts/backfill.py --local --days 28 --pct 5 \\
        --candidates data/candidates.txt --seed-gap1

Everything is resumable: the index lives in data/cache/backfill/, sample membership and
progress in analytics.db (``samples``), transactions in the RPC disk cache.
"""
from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from rugsnipe_analysis import backfill as B  # noqa: E402
from rugsnipe_analysis import store  # noqa: E402
from rugsnipe_analysis.rpc import ARCHIVE_ENDPOINTS, DEFAULT_ENDPOINTS, RpcClient  # noqa: E402
from rugsnipe_analysis.rpcbudget import RpcBudget, load_config, validate  # noqa: E402


def read_candidates(arg: str | None) -> list[str]:
    if not arg:
        return []
    p = Path(arg)
    if p.exists():
        return [ln.split()[0] for ln in p.read_text().splitlines() if ln.strip() and not ln.startswith("#")]
    return [x.strip() for x in arg.split(",") if x.strip()]


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--local", action="store_true", help="write to data/analytics.db (default /var/lib/rugsnipe)")
    ap.add_argument("--db")
    ap.add_argument("--config", help="TOML with [rpc_budget] (role 'backfill')")
    ap.add_argument("--days", type=float, default=2.0)
    ap.add_argument("--pct", type=float, default=0.5, help="uniform sample, percent of creates")
    ap.add_argument("--seed", default="1")
    ap.add_argument("--candidates", help="file (one pubkey per line) or comma list of candidate creators")
    ap.add_argument("--blocks", type=int, default=0, help="also getBlock(accounts) for slots s..s+N-1")
    ap.add_argument("--seed-gap1", action="store_true", help="initialise an empty index from data/cache/gap1/index.json")
    ap.add_argument("--no-head", action="store_true", help="do not extend the index to the chain head")
    ap.add_argument("--index-only", action="store_true")
    ap.add_argument("--limit", type=int, help="fetch at most N pending samples this run")
    ap.add_argument("--workers", type=int, default=4)
    ap.add_argument("--publicnode", action="store_true",
                    help="round-robin getTransaction over publicnode too (only useful for < 2 day old txs)")
    a = ap.parse_args()

    cfg = load_config(a.config) if a.config else load_config()
    errs = validate(cfg)
    if errs:
        sys.exit("config error: " + "; ".join(errs))
    budget = RpcBudget.from_config("backfill", cfg)
    budget.method_caps.setdefault("getBlock", 1.0)
    rpc = RpcClient(endpoints=DEFAULT_ENDPOINTS if a.publicnode else ARCHIVE_ENDPOINTS, budget=budget)
    con = store.connect(a.db or (store.LOCAL_DB if a.local else store.OHIO_DB))
    t0 = time.time()

    idx = B.SigIndex()
    if idx.state["newest"] is None and a.seed_gap1 and B.GAP1_INDEX.exists():
        n = idx.seed(B.load_gap1_index(), "gap1/index.json")
        B.log(f"index seeded with {n} gap-1 signatures")
    head_pages = 0 if a.no_head else idx.extend_head(rpc)
    stop = int((idx.state["newest"] or {}).get("bt") or time.time()) - int(a.days * 86400)
    back_pages = idx.extend_back(rpc, stop)
    st = idx.state
    B.log(f"index: {len(idx.rows())} sigs, newest {B._age(st['newest']['bt'])} ago, oldest "
          f"{B._age(st['oldest']['bt'])} ago (+{head_pages} head / +{back_pages} back pages)")
    out: dict = {"index": {"sigs": len(idx.rows()), "head_pages": head_pages, "back_pages": back_pages,
                           "newest": st["newest"], "oldest": st["oldest"]}}
    if not a.index_only:
        bf = B.Backfill(con, rpc, a.days, a.pct / 100.0, a.seed, index=idx, blocks=a.blocks, workers=a.workers)
        drawn = bf.draw(read_candidates(a.candidates))
        B.log(f"design {bf.design}: {drawn}")
        res = bf.fetch(limit=a.limit)
        status = dict(con.execute("SELECT status, COUNT(*) FROM samples WHERE design=? GROUP BY status",
                                  (bf.design,)).fetchall())
        regimes = dict(con.execute("SELECT c.regime, COUNT(*) FROM samples s JOIN creates c ON c.sig=s.sig "
                                   "WHERE s.design=? GROUP BY c.regime", (bf.design,)).fetchall())
        out.update(design=bf.design, drawn=drawn, fetch=res, sample_status=status, regimes=regimes,
                   anchor=bf.anchor())
        out["cost_full_B"] = B.full_cost_estimate(drawn["window_sigs"], a.days, 28, 0.05, max(a.blocks, 2))
    out["rpc_calls_by_method"] = budget.calls_by_method()
    out["rpc_client"] = rpc.stats
    out["seconds"] = round(time.time() - t0, 1)
    con.close()
    print(json.dumps(out, indent=1, default=str))


if __name__ == "__main__":
    main()
