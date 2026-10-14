"""S3 data: complete the per-operator launch histories needed by operator_walkforward.py.

Two pulls, both cached under data/cache/strategy/ (page cache in data/cache/strategy/pumpapi/):

  A  The gap-1 expanded wallets that bundle (>= 3 multi-wallet bundled launches among their scored
     coins). gap-1 s6 skipped every coin with ATH < $5.5k, which is an OUTCOME filter; without those
     coins an operator's history would be biased upward. We pull their first seconds of trades.
  B  New operators: the creator (signer) wallets of gap-3 multi-wallet bundled launches with >= 5
     launches in the gap-1 1.27 % sample (>= ~390 launches in 16 days) that neither gap-1 s6 nor the
     gap-3 operator pull covered. Coins are sampled across the 16-day window from /coins?creator=
     pages (offsets aimed at 4 points of the window from the listing rate), then up to K per wallet.

Each coin: trades_before(create + 25 s, 1 page); if that page is not complete, trades_before(create
+ 4 s, 1 page). Policies that need longer horizons are not scored on these tapes.

HARD CAP: 1,500 HTTP requests in total (both hosts, retries included). swap-api <= 1.3 req/s
(78/min), frontend <= 0.8 req/s.

Run: .venv/bin/python -I analysis/scripts/strategy/operator_pull.py [A|B|AB] [n_ops_B=40] [K=14]
Output: data/cache/strategy/op_tapes.jsonl (one row per coin), op_listing.json (wallet -> listing meta).
"""
from __future__ import annotations

import json
import random
import sys
import threading
import time
from collections import Counter, defaultdict
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))
from rugsnipe_analysis.cache import DEFAULT_CACHE, DiskCache  # noqa: E402
from rugsnipe_analysis.pumpapi import FRONTEND, PumpApi, normalize_trade  # noqa: E402

CAP = 1500
OUTD = REPO / "data" / "cache" / "strategy"
OUT = OUTD / "op_tapes.jsonl"
LIST = OUTD / "op_listing.json"
ANCHOR = 1791968330
T0, T1 = ANCHOR - 16 * 86400, ANCHOR - 6 * 3600
F_SAMPLE = 9947 / 781123
SOLQ = (None, "", "11111111111111111111111111111111", "So11111111111111111111111111111111111111112")

sw = PumpApi(max_rps=1.3, cache_root=str(OUTD))
fe = PumpApi(max_rps=0.8, cache_root=str(OUTD))
old = DiskCache(DEFAULT_CACHE, "pumpapi")  # gap-1 listing cache (read only)
lock = threading.Lock()


CNT = OUTD / "op_requests.txt"
PRIOR = int(CNT.read_text()) if CNT.exists() else 0


def used() -> int:
    n = PRIOR + sw.stats["calls"] + fe.stats["calls"]
    CNT.write_text(str(n))
    return n


class CapReached(Exception):
    pass


def guard():
    if used() >= CAP:
        raise CapReached()


def pull_coin(w: str, coin: dict, src: str) -> dict | None:
    mint = coin["mint"]
    bt = int(coin["created_timestamp"] // 1000)
    res = None
    for W in (25, 4):
        guard()
        raw, comp = sw.trades_before(mint, (bt + W) * 1000 + 999, max_pages=1)
        if comp:
            tr = sorted((normalize_trade(t) for t in raw), key=lambda x: (x["slot"], x["txi"], x["sub"]))
            res = (tr, W)
            break
    if res is None:
        return {"mint": mint, "wallet": w, "src": src, "bt": bt, "complete": False, "trades": []}
    tr, W = res
    sol_usd = next((t["sol_usd"] for t in tr if t.get("sol_usd")), None)
    return {"mint": mint, "wallet": w, "src": src, "bt": bt, "complete": True, "hor": W,
            "ath_usd": coin.get("ath_market_cap"), "sol_usd": sol_usd, "graduated": coin.get("complete"),
            "trades": [[t["slot"], t["txi"], t["sub"], t["ts"], t["user"], int(t["buy"]), round(t["sol"], 9),
                        round(t["tok"], 6), t["mcap"], t["sig"], t["prog"]] for t in tr]}


def listing_page(w: str, off: int) -> list:
    hit = old.get(f"creatorcoins50:{w}:{off // 50}") if off % 50 == 0 else None
    if hit is not None:
        return hit["v"] or []
    guard()
    return fe.get_json(f"{FRONTEND}/coins", {"offset": off, "limit": 50, "sort": "created_timestamp", "order": "DESC",
                                             "includeNsfw": "true", "creator": w},
                       cache_key=f"creatorcoins50off:{w}:{off}") or []


def sample_listing(w: str, K: int) -> tuple[list, dict]:
    """Coins of w spread over [T0, T1]; returns (coins in window, meta)."""
    p0 = listing_page(w, 0)
    coins = {c["mint"]: c for c in p0}
    meta = {"pages": [0]}
    if len(p0) == 50:
        ts = [c["created_timestamp"] / 1000 for c in p0]
        rate = 49 / max(1.0, max(ts) - min(ts))  # coins per second near "now"
        newest = max(ts)
        for q in (0.1, 0.35, 0.6, 0.85):
            target = T1 - q * (T1 - T0)
            off = int(max(0, (newest - target) * rate) // 50 * 50)
            for _ in range(3):  # if the page is empty, the wallet has fewer coins: halve the offset
                if off in meta["pages"]:
                    break
                pg = listing_page(w, off)
                meta["pages"].append(off)
                if pg:
                    for c in pg:
                        coins[c["mint"]] = c
                    # re-aim the rate from the page we landed on
                    tp = [c["created_timestamp"] / 1000 for c in pg]
                    if max(tp) < newest:
                        rate = max(rate * 0.2, (off + len(pg)) / max(1.0, newest - min(tp)))
                    break
                off = off // 2 // 50 * 50
    inwin = [c for c in coins.values() if T0 <= c["created_timestamp"] / 1000 <= T1 and c.get("creator") == w]
    meta["n_listed_in_window"] = len(inwin)
    meta["complete_listing"] = len(p0) < 50
    return inwin, meta


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "AB"
    n_ops = int(sys.argv[2]) if len(sys.argv) > 2 else 40
    K = int(sys.argv[3]) if len(sys.argv) > 3 else 14
    done = set()
    if OUT.exists():
        done = {json.loads(l)["mint"] for l in open(OUT)}
    listing = json.load(open(LIST)) if LIST.exists() else {}
    fo = open(OUT, "a")
    items = []
    # ---------------- A
    if "A" in mode:
        rows = [json.loads(l) for l in open(REPO / "data/cache/gap1/expanded.jsonl")]
        ok = [r for r in rows if r["sol_quoted"] and not r["mayhem"]]
        byw = defaultdict(list)
        for r in ok:
            byw[r["wallet"]].append(r)
        bund = [w for w, rs in byw.items() if sum(r.get("bundle_wallets", 0) >= 1 for r in rs) >= 3]
        for w in bund:
            for r in byw[w]:
                if r.get("skip") == "low_ath" and r["mint"] not in done:
                    items.append((w, {"mint": r["mint"], "created_timestamp": r["bt"] * 1000,
                                      "ath_market_cap": r["ath_usd"], "complete": r["complete"]}, "A"))
        print(time.strftime("%H:%M:%S"), f"A: {len(bund)} bundling wallets, {len(items)} low-ATH coins to pull", flush=True)
    # ---------------- B
    if "B" in mode:
        sel = {s["mint"]: s for s in json.load(open(REPO / "data/cache/gap3/sel.json"))}
        sys.path.insert(0, str(REPO / "analysis"))
        from rugsnipe_analysis.strategy.tapes import load_gap3
        g3 = load_gap3()
        exp = {json.loads(l)["wallet"] for l in open(REPO / "data/cache/gap1/expanded.jsonl")}
        ops = {s["op"] for s in json.load(open(REPO / "data/cache/gap3/ops_sel_EhrQmJ.json"))}
        cre = [json.loads(l) for l in open(REPO / "data/cache/gap1/creates.jsonl")]
        byu = Counter(r["user"] for r in cre if r.get("kind") == "create")
        multi = Counter(sel[t.mint]["user"] for t in g3 if t.nB >= 1)
        cand = sorted((w for w in multi if w not in exp and w not in ops and byu[w] >= 5),
                      key=lambda w: (-byu[w], w))[:n_ops]
        print(time.strftime("%H:%M:%S"), f"B: {len(cand)} new operators", flush=True)
        for w in cand:
            try:
                if w in listing:
                    inwin = listing[w]["coins"]
                else:
                    inwin, meta = sample_listing(w, K)
                    inwin = [{k: c.get(k) for k in ("mint", "created_timestamp", "ath_market_cap", "complete",
                                                     "quote_mint", "mayhem_state", "creator")} for c in inwin]
                    listing[w] = {**meta, "sampled_count": byu[w], "N_hat": byu[w] / F_SAMPLE, "coins": inwin}
                    json.dump(listing, open(LIST, "w"))
            except CapReached:
                print("cap reached during listings", flush=True)
                break
            pool = [c for c in inwin if c.get("quote_mint") in SOLQ and not c.get("mayhem_state")]
            random.Random(w).shuffle(pool)
            for c in pool[:K]:
                if c["mint"] not in done:
                    items.append((w, c, "B"))
        print(time.strftime("%H:%M:%S"), f"total coins to pull {len(items)}; requests used so far {used()}", flush=True)

    def go(it):
        w, c, src = it
        try:
            r = pull_coin(w, c, src)
        except CapReached:
            return "cap"
        with lock:
            fo.write(json.dumps(r, separators=(",", ":")) + "\n")
            fo.flush()
            done.add(c["mint"])
            if len(done) % 50 == 0:
                print(time.strftime("%H:%M:%S"), len(done), "req", used(), sw.stats, flush=True)
        return "ok"

    with ThreadPoolExecutor(2) as ex:
        res = list(ex.map(go, items))
    print(time.strftime("%H:%M:%S"), "done", Counter(res), "requests", used(), sw.stats, fe.stats, flush=True)


if __name__ == "__main__":
    main()
