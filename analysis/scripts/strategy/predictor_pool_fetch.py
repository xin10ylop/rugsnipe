"""G1 angle B: fetch create transactions, signer funding and metadata JSON for the operator-pool launches
(strategy-operators.md S3 tapes), so they can serve as extra training rows / operator history.
RPC <= 3.5 req/s (api.mainnet-beta), metadata via predictor_meta.fetch.

  .venv/bin/python -I analysis/scripts/strategy/predictor_pool_fetch.py
"""
from __future__ import annotations

import json
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
sys.path.insert(0, str(REPO / "analysis/scripts/strategy"))
from rugsnipe_analysis.rpc import RpcClient  # noqa: E402
from rugsnipe_analysis.strategy import predictor as P  # noqa: E402
from rugsnipe_analysis.window import creator_funding  # noqa: E402

OUTF = P.CACHE / "funding_pool.jsonl"


def main():
    pool = P.load_pool()
    print("pool multi launches", len(pool), flush=True)
    json.dump([[t.mint, s, w, t.src] for t, s, w in pool], open(P.CACHE / "pool_sigs.json", "w"))
    rpc = RpcClient(endpoints=("https://api.mainnet-beta.solana.com",), max_rps=2.5)
    t0 = time.time()
    for i, (t, s, w) in enumerate(pool):
        for _ in range(3):
            try:
                rpc.get_transaction(s)
                break
            except RuntimeError:
                time.sleep(20)
        if i % 100 == 0:
            print("tx", i, rpc.stats, f"{time.time()-t0:.0f}s", flush=True)
    done = set()
    if OUTF.exists():
        done = {json.loads(line)["as_of_sig"] for line in open(OUTF)}
    with open(OUTF, "a") as f:
        for i, (t, s, w) in enumerate(pool):
            if s in done:
                continue
            try:
                res = creator_funding(rpc, w, s)
            except Exception as e:  # noqa: BLE001
                res = {"status": "error", "err": str(e)[:200]}
            res["wallet"], res["as_of_sig"] = w, s
            f.write(json.dumps(res) + "\n")
            f.flush()
            if i % 100 == 0:
                print("funding", i, rpc.stats, f"{time.time()-t0:.0f}s", flush=True)
    # metadata
    import predictor_meta as PM
    rc = P._rpc_cache()
    todo = []
    for t, s, w in pool:
        cx = P.create_tx_features(s, rc)
        if cx.get("uri", "") and cx["uri"].startswith("http"):
            todo.append((t.mint, cx["uri"]))
    with ThreadPoolExecutor(8) as ex:
        list(ex.map(lambda a: PM.fetch(*a), todo))
    with ThreadPoolExecutor(4) as ex:
        res = list(ex.map(PM.refetch_ipfs, [m for m, _ in todo]))
    print("meta ok", sum(1 for d in res if d.get("json") is not None), "of", len(res), f"{time.time()-t0:.0f}s")


if __name__ == "__main__":
    main()
