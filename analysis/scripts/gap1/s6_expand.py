"""Stage 6: expand wallet-keyed operators with the coins API (/coins?creator=).

  python s6_expand.py counts   -> wallet_counts.json: exact #coins per wallet in the 16-day window
                                  (all wallets in multi-launch components + random singletons)
  python s6_expand.py outcomes -> expanded.jsonl: outcome rows for up to K coins per wallet with
                                  >= 5 launches (uniform subsample per wallet), same entry model as s3
"""
import json
import random
import sys
import threading
from collections import Counter, defaultdict
from concurrent.futures import ThreadPoolExecutor

from common import OUT, PumpApi, log
from rugsnipe_analysis.pumpapi import FRONTEND, normalize_trade

api = PumpApi(max_rps=0.9)
api_sw = PumpApi(max_rps=4.0)
meta = json.load(open(OUT / "index.json"))
ANCHOR = meta["anchor"]
T0, T1 = ANCHOR - 16 * 86400, ANCHOR - 6 * 3600
del meta


def coins_of(w, max_pages=30):
    """All coins of creator w created at/after T0 (limit=50 pages; limit>50 is silently capped at 70)."""
    out = []
    for p in range(max_pages):
        page = api.get_json(f"{FRONTEND}/coins", {"offset": p * 50, "limit": 50, "sort": "created_timestamp",
                                                 "order": "DESC", "includeNsfw": "true", "creator": w},
                            cache_key=f"creatorcoins50:{w}:{p}") or []
        out += page
        if len(page) < 50 or (page and page[-1]["created_timestamp"] / 1000 < T0):
            return out, True
    return out, False


mode = sys.argv[1]
if mode == "counts":
    cre = [json.loads(l) for l in open(OUT / "creates.jsonl")]
    cre = [r for r in cre if r.get("kind") == "create"]
    by_w = Counter(r["user"] for r in cre)
    multi = [w for w, c in by_w.items() if c >= 2]
    singles = [w for w, c in by_w.items() if c == 1]
    random.Random(7).shuffle(singles)
    nsing = int(sys.argv[2]) if len(sys.argv) > 2 else 400
    todo = singles[:nsing] + (multi if "--multi" in sys.argv else [])
    log(f"{len(by_w)} wallets: {len(multi)} multi, {len(singles)} singles; querying {len(todo)}")
    res = {}
    for i, w in enumerate(todo):
        coins, complete = coins_of(w, max_pages=12)
        inwin = [c for c in coins if T0 <= c["created_timestamp"] / 1000 <= T1 and c.get("creator") == w]
        res[w] = {"sampled": by_w[w], "n_window": len(inwin), "complete": complete,
                  "n_api_total": len(coins), "single": by_w[w] == 1,
                  "days": sorted({int((c["created_timestamp"] / 1000 - T0) // 86400) for c in inwin})}
        if i % 50 == 0:
            log(i, w[:8], res[w]["n_window"], complete)
            json.dump({"n_single_pool": len(singles), "n_single_q": min(nsing, len(singles)), "wallets": res},
                      open(OUT / "wallet_counts.json", "w"))
    json.dump({"n_single_pool": len(singles), "n_single_q": min(nsing, len(singles)), "wallets": res},
              open(OUT / "wallet_counts.json", "w"))
    log("done")
    sys.exit()

# ---------------------------------------------------------------- outcomes
K = int(sys.argv[2]) if len(sys.argv) > 2 else 60
cre = [json.loads(l) for l in open(OUT / "creates.jsonl")]
by_w = Counter(r["user"] for r in cre if r.get("kind") == "create")
mid = sorted(w for w, c in by_w.items() if 2 <= c <= 9)
random.Random(11).shuffle(mid)
NW = int(sys.argv[3]) if len(sys.argv) > 3 else 120
targets = mid[:NW]
log(f"{len(mid)} wallets with 2..9 sampled launches; expanding {len(targets)} (K={K})")
items = []
for w in targets:
    coins, comp = coins_of(w, max_pages=20)
    if not comp:
        log("incomplete listing", w[:8], len(coins))
    inwin = [c for c in coins if T0 <= c["created_timestamp"] / 1000 <= T1 and c.get("creator") == w]
    random.Random(w).shuffle(inwin)
    for c in inwin[:K]:
        items.append((w, c))
out_path = OUT / "expanded.jsonl"
done = set()
if out_path.exists():
    for l in open(out_path):
        done.add(json.loads(l)["mint"])
items = [it for it in items if it[1]["mint"] not in done]
log(f"{len(items)} coins to score")
lock = threading.Lock()
fo = open(out_path, "a")


def analyse(it):
    w, coin = it
    mint = coin["mint"]
    bt = coin["created_timestamp"] // 1000
    sol_q = coin.get("quote_mint") in (None, "", "11111111111111111111111111111111", "So11111111111111111111111111111111111111112")
    base = {"mint": mint, "wallet": w, "bt": bt, "sol_quoted": sol_q, "ath_usd": coin.get("ath_market_cap"),
            "ath_ts": (coin.get("ath_market_cap_timestamp") or 0) // 1000, "complete": coin.get("complete"),
            "host": (coin.get("metadata_uri") or "").split("/")[2] if "//" in (coin.get("metadata_uri") or "") else "",
            "mayhem": bool(coin.get("mayhem_state"))}
    if (coin.get("ath_market_cap") or 0) < 5500:
        res = {**base, "skip": "low_ath", "entry_mcap": None, "max_after": 0.0}
    else:
        raw, complete = api_sw.trades_before(mint, (bt + 2) * 1000 + 999, max_pages=6)
        tr = sorted((normalize_trade(t) for t in raw), key=lambda x: (x["slot"], x["txi"], x["sub"]))
        if not tr:
            res = {**base, "skip": "no_trades", "entry_mcap": 27.959, "max_after": 0.0}
        else:
            first = tr[0]
            slot0 = first["slot"]
            ctx = first["txi"] if first["user"] == w else None
            s0 = [t for t in tr if t["slot"] == slot0]
            bundle_sigs, bundle_wallets = set(), set()
            if ctx is not None:
                bundle_sigs.add(first["sig"])
                prev = ctx
                for t in s0:
                    if t["txi"] <= ctx:
                        continue
                    if t["txi"] - prev <= 2 and t["txi"] - ctx <= 5 and t["buy"]:
                        bundle_sigs.add(t["sig"]); bundle_wallets.add(t["user"]); prev = t["txi"]
                    else:
                        break
            for t in s0:
                if t["user"] == w:
                    bundle_sigs.add(t["sig"])
            third = []
            for t in s0:
                if t["sig"] in bundle_sigs or t["user"] in bundle_wallets or t["user"] == w:
                    continue
                if t["buy"] and (not third or third[-1] != t["sig"]):
                    third.append(t["sig"])
            lbp = max((i for i, t in enumerate(tr) if t["sig"] in bundle_sigs), default=-1)
            if third:
                ms = third[(len(third) + 1) // 2 - 1]
                pos = max(max(i for i, t in enumerate(tr) if t["sig"] == ms), lbp)
            else:
                pos = lbp
            entry = tr[pos]["mcap"] if pos >= 0 else 27.959
            mx = max((t["mcap"] for t in tr[pos + 1:]), default=0.0)
            sol_usd = next((t["sol_usd"] for t in tr if t.get("sol_usd")), None)
            src = "early"
            ath_ts = base["ath_ts"]
            early_end = tr[-1]["ts"]
            if sol_usd and coin.get("ath_market_cap", 0) / sol_usd >= 1.6 * entry:
                if ath_ts > early_end:
                    raw2, _ = api_sw.trades_before(mint, ath_ts * 1000 + 1999, max_pages=1)
                    if raw2:
                        mx = max(mx, max(normalize_trade(t)["mcap"] for t in raw2)); src = "ath_trades"
                elif mx < 1.6 * entry:
                    raw3, comp3 = api_sw.trades_latest(mint, max_pages=4)
                    key = (tr[pos]["slot"], tr[pos]["txi"], tr[pos]["sub"]) if pos >= 0 else (0, 0, 0)
                    t3 = [normalize_trade(t) for t in raw3]
                    mx = max([mx] + [t["mcap"] for t in t3 if (t["slot"], t["txi"], t["sub"]) > key])
                    src = "latest_full" if comp3 else "latest_partial"
            res = {**base, "entry_mcap": entry, "max_after": mx, "max_src": src, "ctx_known": ctx is not None,
                   "bundle_wallets": len(bundle_wallets), "third_n": len(third), "early_complete": complete,
                   "bundle_sol": round(sum(t["sol"] for t in s0 if t["sig"] in bundle_sigs and t["buy"]), 4)}
    with lock:
        fo.write(json.dumps(res, separators=(",", ":")) + "\n"); fo.flush()
        done.add(mint)
        if len(done) % 200 == 0:
            log(len(done), api_sw.stats)


with ThreadPoolExecutor(6) as ex:
    for f in [ex.submit(analyse, it) for it in items]:
        try:
            f.result()
        except Exception as e:  # noqa: BLE001
            log("fail", type(e).__name__, str(e)[:150])
log("done", len(done))
