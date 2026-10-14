"""G1 step 0: fetch the metadata JSON (token uri) of every launch in the predictor table, with latency.

  .venv/bin/python -I analysis/scripts/strategy/predictor_meta.py [workers]

Each result is cached in data/cache/predictor/meta/<mint>.json as {"uri", "ms", "status", "json"}.
A study fetch long after the launch: the latency recorded is what a live bot would see only if the host
serves the JSON at create time (it must, the pump.fun UI reads it), but cold IPFS gateways are slower.
"""
from __future__ import annotations

import json
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import requests

REPO = Path(__file__).resolve().parents[3]
OUTD = REPO / "data/cache/predictor/meta"
OUTD.mkdir(parents=True, exist_ok=True)


def fetch(mint: str, uri: str) -> dict:
    p = OUTD / f"{mint}.json"
    if p.exists():
        return json.load(open(p))
    res = {"uri": uri, "ms": None, "status": None, "json": None, "tries": 0}
    for attempt in range(2):
        res["tries"] = attempt + 1
        t0 = time.perf_counter()
        try:
            r = requests.get(uri, timeout=12, headers={"User-Agent": "Mozilla/5.0"})
            res["ms"] = round((time.perf_counter() - t0) * 1000)
            res["status"] = r.status_code
            if r.status_code == 200:
                try:
                    res["json"] = r.json()
                except ValueError:
                    res["json"] = None
                break
        except requests.RequestException as e:
            res["ms"] = round((time.perf_counter() - t0) * 1000)
            res["status"] = type(e).__name__
        time.sleep(1.0)
    with open(p, "w") as f:
        json.dump(res, f)
    return res


GATEWAYS = ("https://ipfs.filebase.io/ipfs/", "https://4everland.io/ipfs/")


def refetch_ipfs(mint: str) -> dict:
    """ipfs.io answers 403/429 to this box: fetch the same CID from another public gateway (study only;
    live, metadata would come from our own IPFS node or not at all)."""
    p = OUTD / f"{mint}.json"
    d = json.load(open(p))
    if d.get("json") is not None or "/ipfs/" not in d["uri"] or d.get("gw_tried"):
        return d
    cid = d["uri"].split("/ipfs/", 1)[1]
    for gw in GATEWAYS:
        t0 = time.perf_counter()
        try:
            r = requests.get(gw + cid, timeout=15, headers={"User-Agent": "Mozilla/5.0"})
            ms = round((time.perf_counter() - t0) * 1000)
            if r.status_code == 200:
                try:
                    d["json"] = r.json()
                    d["gw"], d["gw_ms"] = gw, ms
                    break
                except ValueError:
                    pass
        except requests.RequestException:
            pass
    d["gw_tried"] = True
    with open(p, "w") as f:
        json.dump(d, f)
    return d


def main():
    workers = int(sys.argv[1]) if len(sys.argv) > 1 else 8
    rows = [json.loads(line) for line in open(REPO / "data/cache/predictor/table.jsonl")]
    todo = [(r["mint"], r["uri"]) for r in rows if r.get("uri") and r["uri"].startswith("http")]
    print("to fetch", len(todo), flush=True)
    t0 = time.time()
    done = 0
    with ThreadPoolExecutor(workers) as ex:
        for res in ex.map(lambda a: fetch(*a), todo):
            done += 1
            if done % 200 == 0:
                print(done, f"{time.time()-t0:.0f}s", flush=True)
    print("done", done, f"{time.time()-t0:.0f}s")
    with ThreadPoolExecutor(4) as ex:
        res = list(ex.map(refetch_ipfs, [m for m, _ in todo]))
    print("after gateway retry: json ok", sum(1 for d in res if d.get("json") is not None), "of", len(res))


if __name__ == "__main__":
    main()
