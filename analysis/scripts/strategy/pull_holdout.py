"""Pull the first 150 s of trades for the light-sample launches (2026-10-12..14) that are in neither the
gap-3 nor the deep sample: non-mayhem, SOL-quoted. These form an untouched holdout for the policies
frozen in data/reports/strategy/frozen_policies.json.

Run: .venv/bin/python -I analysis/scripts/strategy/pull_holdout.py [rps=1.15]
Output: data/cache/strategy/holdout_trades.jsonl (gap-3 tuple format), resumable.
"""
import json
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
from rugsnipe_analysis.pumpapi import PumpApi, normalize_trade  # noqa: E402

rps = float(sys.argv[1]) if len(sys.argv) > 1 else 1.15
api = PumpApi(max_rps=rps)
deep = {json.loads(l)["mint"] for l in open(REPO / "data/tokens_sample.jsonl")}
g3 = {json.loads(l)["mint"] for l in open(REPO / "data/cache/gap3/trades.jsonl")}
rows = [json.loads(l) for l in open(REPO / "data/tokens_light.jsonl")]
todo = [r for r in rows if r.get("sol_quoted") and not r["is_mayhem_mode"] and r["mint"] not in deep and r["mint"] not in g3]
out = REPO / "data/cache/strategy/holdout_trades.jsonl"
done = set()
if out.exists():
    done = {json.loads(l)["mint"] for l in open(out)}
todo = [r for r in todo if r["mint"] not in done]
print("to pull", len(todo), "done", len(done), flush=True)
lock = threading.Lock()
fo = open(out, "a")


def go(r):
    mint, bt, slot = r["mint"], r["create_time"], r["create_slot"]
    seen = {}
    complete = False
    hor = None
    for W, mp in ((150, 8), (60, 6)):
        raw, comp = api.trades_before(mint, (bt + W) * 1000 + 999, max_pages=mp)
        for t in raw:
            n = normalize_trade(t)
            seen[(n["sig"], n["sub"], n["user"])] = n
        if comp:
            complete, hor = True, W
            break
    tr = sorted(seen.values(), key=lambda x: (x["slot"], x["txi"], x["sub"]))
    tr = [t for t in tr if t["slot"] >= slot and t["ts"] <= bt + (hor or 0)]
    res = {"mint": mint, "complete": complete, "hor": hor, "slot": slot, "bt": bt, "sig": r["create_sig"],
           "creator": r["creator"], "user": r["create_user"], "fee_payer": r["fee_payer"],
           "trades": [[t["slot"], t["txi"], t["sub"], t["ts"], t["user"], int(t["buy"]), round(t["sol"], 9),
                       round(t["tok"], 6), t["mcap"], t["sig"], t["prog"]] for t in tr]}
    with lock:
        fo.write(json.dumps(res, separators=(",", ":")) + "\n")
        fo.flush()
        done.add(mint)
        if len(done) % 50 == 0:
            print(time.strftime("%H:%M:%S"), len(done), api.stats, flush=True)


with ThreadPoolExecutor(3) as ex:
    for f in [ex.submit(go, r) for r in todo]:
        try:
            f.result()
        except Exception as e:  # noqa: BLE001
            print("fail", e, flush=True)
print("done", len(done), api.stats, flush=True)
