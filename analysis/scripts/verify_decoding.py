#!/usr/bin/env python3
"""Cross-check swap-api trades against on-chain TradeEvents for a few tokens.

Picks tokens from data/tokens_sample.jsonl: small ones whose whole history fits
in --max-txs transactions (so final/ATH market caps can be compared with the
coins API) plus bundled ones (so slot-0 measurements are checked).
Writes data/reports/decoder_verification.json.
"""
from __future__ import annotations

import argparse
import json
import random
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from rugsnipe_analysis import verify  # noqa: E402
from rugsnipe_analysis.cache import REPO_ROOT  # noqa: E402
from rugsnipe_analysis.dataset import log  # noqa: E402
from rugsnipe_analysis.pumpapi import PumpApi  # noqa: E402
from rugsnipe_analysis.rpc import RpcClient  # noqa: E402

DEEP = REPO_ROOT / "data" / "tokens_sample.jsonl"
LIGHT = REPO_ROOT / "data" / "tokens_light.jsonl"
OUT = REPO_ROOT / "data" / "reports" / "decoder_verification.json"


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("--n-small", type=int, default=5)
    p.add_argument("--n-bundled", type=int, default=5)
    p.add_argument("--max-txs", type=int, default=30)
    p.add_argument("--seed", type=int, default=7)
    a = p.parse_args()
    deep = [json.loads(l) for l in DEEP.read_text().splitlines() if l.strip()]
    light = {r["mint"]: r for r in (json.loads(l) for l in LIGHT.read_text().splitlines() if l.strip())}
    rng = random.Random(a.seed)
    small = [r for r in deep if r["coverage"] == "full" and 5 <= r["n_trades_fetched"] <= a.max_txs - 5
             and r.get("api_ath_usd")]
    bundled = [r for r in deep if r["bundled"] and r["slot0_other_wallets"] >= 2 and not r["instant_graduation"]]
    picks = rng.sample(small, min(a.n_small, len(small))) + rng.sample(bundled, min(a.n_bundled, len(bundled)))
    rpc, api = RpcClient(max_rps=4.0, max_retries=14), PumpApi()
    results = []
    for r in picks:
        rec = dict(light.get(r["mint"]) or {})
        rec.update({k: r[k] for k in ("mint", "bonding_curve", "create_slot", "create_time")})
        res = verify.compare_token(rpc, api, rec, a.max_txs)
        res["kind"] = "small" if r in small else "bundled"
        if r.get("create_txi") is not None:
            res["tx_index_check"] = verify.check_tx_index(rpc, r["create_slot"], r["create_sig"], r["create_txi"])
        log(f"{r['mint'][:8]} {res['kind']}: rpc={res['rpc_trades']} matched={res['matched']} "
            f"mismatch={res['value_mismatch']} miss_api={res['missing_in_api']} "
            f"miss_rpc={res['api_trades_missing_in_rpc_window']} slot0 rpc/api="
            f"{res['slot0_buys_rpc']}/{res['slot0_buys_api']} mcap_err={res.get('api_mcap_rel_err')} "
            f"ath_err={res.get('api_ath_rel_err')} txi={res.get('tx_index_check')}")
        results.append(res)
    summary = {
        "tokens": len(results),
        "rpc_trades": sum(x["rpc_trades"] for x in results),
        "matched": sum(x["matched"] for x in results),
        "value_mismatch": sum(x["value_mismatch"] for x in results),
        "missing_in_api": sum(x["missing_in_api"] for x in results),
        "api_missing_in_rpc_window": sum(x["api_trades_missing_in_rpc_window"] for x in results),
        "slot0_agree": sum(1 for x in results if x["slot0_buys_rpc"] == x["slot0_buys_api"]),
    }
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps({"summary": summary, "tokens": results}, indent=1, default=str))
    log(f"summary {summary} -> {OUT}")


if __name__ == "__main__":
    main()
