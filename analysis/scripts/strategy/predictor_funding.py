"""G1: signer funding for table rows that no earlier cache covers (deep / fresh), same method as the
window (rugsnipe_analysis.window.creator_funding: one signature page before the create, the oldest 3 txs
decoded). Writes data/cache/predictor/funding_extra.jsonl keyed by wallet. RPC <= 3.5 req/s.

  .venv/bin/python -I analysis/scripts/strategy/predictor_funding.py
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
from rugsnipe_analysis.rpc import RpcClient  # noqa: E402
from rugsnipe_analysis.window import creator_funding  # noqa: E402

OUT = REPO / "data/cache/predictor/funding_extra.jsonl"


def main():
    rows = [json.loads(line) for line in open(REPO / "data/cache/predictor/table.jsonl")]
    done = set()
    if OUT.exists():
        done = {json.loads(line)["wallet"] for line in open(OUT)}
    todo = {}
    for r in rows:
        if r["fund_src"] == "none" and r["signer"] and r["signer"] not in done:
            todo.setdefault(r["signer"], r["sig"])
    print("wallets", len(todo), flush=True)
    rpc = RpcClient(endpoints=("https://api.mainnet-beta.solana.com",), max_rps=3.5)
    with open(OUT, "a") as f:
        for i, (w, sig) in enumerate(todo.items()):
            try:
                res = creator_funding(rpc, w, sig)
            except Exception as e:  # noqa: BLE001
                res = {"status": "error", "err": str(e)[:200]}
            res["wallet"] = w
            res["as_of_sig"] = sig
            f.write(json.dumps(res) + "\n")
            f.flush()
            if i % 20 == 0:
                print(i, res.get("status"), rpc.stats, flush=True)


if __name__ == "__main__":
    main()
