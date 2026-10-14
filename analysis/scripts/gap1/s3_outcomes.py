"""Stage 3: coin snapshot + creation-slot structure + post-entry max for each decoded create.

Output: data/cache/gap1/outcomes.jsonl
Per launch:
  * slot-0 structure: bundle (signer/creator/fee-payer + txs adjacent to the create, <=4 txs
    after it, gap <= 2 indexes), third-party slot-0 buyers, slot-0 sells
  * entry spot (mcap SOL) after bundle + median third-party slot-0 buy tx
  * max spot (mcap SOL) after that entry position (early trades, ATH-time trades or
    full history)
"""
import json
import math
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor

from common import OUT, PumpApi, log
from rugsnipe_analysis.pumpapi import normalize_trade

api = PumpApi(max_rps=9.0)
rows = [json.loads(l) for l in open(OUT / "creates.jsonl")]
rows = [r for r in rows if r.get("kind") == "create"]
lim = int(sys.argv[1]) if len(sys.argv) > 1 else None
rows.sort(key=lambda r: r["rank"])
if lim:
    rows = rows[:lim]
out_path = OUT / "outcomes.jsonl"
done = set()
if out_path.exists():
    for l in open(out_path):
        done.add(json.loads(l)["mint"])
log(f"{len(rows)} creates, {len(done)} done")
todo = [r for r in rows if r["mint"] not in done]
# coin snapshots in batches of 100 (POST limit 30/min)
coins = api.get_coins([r["mint"] for r in todo])
log(f"coin snapshots: {len(coins)}")
lock = threading.Lock()
fo = open(out_path, "a")


def analyse(r):
    mint = r["mint"]
    coin = coins.get(mint) or {}
    bt = r["bt"]
    if coin and (coin.get("ath_market_cap") or 0) < 5500 and r["rank"] % 5 != 0:
        res = {"mint": mint, "sig": r["sig"], "skip": "low_ath", "coin_found": True,
               "ath_usd": coin.get("ath_market_cap"), "ath_ts": (coin.get("ath_market_cap_timestamp") or 0) // 1000,
               "complete": coin.get("complete"), "entry_mcap": None, "max_after": 0.0}
        with lock:
            fo.write(json.dumps(res, separators=(",", ":")) + "\n"); fo.flush(); done.add(mint)
        return
    side = {r["creator"], r["user"], r["fee_payer"], *r.get("signers", [])}
    raw, complete = api.trades_before(mint, (bt + 2) * 1000 + 999, max_pages=6)
    tr = sorted((normalize_trade(t) for t in raw), key=lambda x: (x["slot"], x["txi"], x["sub"]))
    tr = [t for t in tr if t["slot"] >= r["slot"]]
    s0 = [t for t in tr if t["slot"] == r["slot"]]
    ctx = next((t["txi"] for t in s0 if t["sig"] == r["sig"]), None)
    # bundle: creator-side wallets + adjacent txs right after the create tx
    bundle_sigs, bundle_wallets = set(), set()
    if ctx is not None:
        bundle_sigs.add(r["sig"])
        prev = ctx
        for t in s0:
            if t["txi"] <= ctx:
                continue
            if t["txi"] - prev <= 2 and t["txi"] - ctx <= 5 and t["buy"]:
                bundle_sigs.add(t["sig"]); bundle_wallets.add(t["user"]); prev = t["txi"]
            else:
                break
    for t in s0:
        if t["user"] in side:
            bundle_sigs.add(t["sig"])
    third = []  # third-party slot-0 buy txs in order
    for t in s0:
        if t["sig"] in bundle_sigs or t["user"] in side or t["user"] in bundle_wallets:
            continue
        if t["buy"] and (not third or third[-1] != t["sig"]):
            third.append(t["sig"])
    last_bundle_pos = max((i for i, t in enumerate(tr) if t["sig"] in bundle_sigs), default=-1)
    if third:
        med_sig = third[(len(third) + 1) // 2 - 1]
        pos = max(i for i, t in enumerate(tr) if t["sig"] == med_sig)
        pos = max(pos, last_bundle_pos)
    else:
        pos = last_bundle_pos
    init_mcap = 27.959 if r.get("sol_quoted") else None
    entry = tr[pos]["mcap"] if pos >= 0 else (init_mcap or None)
    eos0 = s0[-1]["mcap"] if s0 else init_mcap
    after = tr[pos + 1:]
    mx_early = max((t["mcap"] for t in after), default=0.0)
    sol_usd = next((t["sol_usd"] for t in tr if t.get("sol_usd")), None)
    ath_usd = coin.get("ath_market_cap")
    ath_ts = (coin.get("ath_market_cap_timestamp") or 0) // 1000
    mx = mx_early
    mx_src = "early"
    early_end = tr[-1]["ts"] if tr else bt
    if ath_usd and sol_usd and entry:
        ath_sol_est = ath_usd / sol_usd
        if ath_sol_est >= 1.6 * entry:
            if ath_ts > early_end:
                raw2, _ = api.trades_before(mint, ath_ts * 1000 + 1999, max_pages=1)
                if raw2:
                    m2 = max(normalize_trade(t)["mcap"] for t in raw2)
                    mx = max(mx, m2); mx_src = "ath_trades"
                else:
                    mx_src = "ath_missing"
            elif ath_ts <= early_end and mx_early < 1.6 * entry:
                raw3, comp3 = api.trades_latest(mint, max_pages=4)
                t3 = [normalize_trade(t) for t in raw3]
                end_key = (tr[pos]["slot"], tr[pos]["txi"], tr[pos]["sub"]) if pos >= 0 else (0, 0, 0)
                m3 = max((t["mcap"] for t in t3 if (t["slot"], t["txi"], t["sub"]) > end_key), default=0)
                mx = max(mx, m3); mx_src = "latest_full" if comp3 else "latest_partial"
    s0_buys = [t for t in s0 if t["buy"]]
    res = {
        "mint": mint, "sig": r["sig"], "early_complete": complete, "n_early": len(tr),
        "create_txi": ctx, "s0_n": len(s0), "s0_buy_sol": round(sum(t["sol"] for t in s0_buys), 4),
        "s0_sell_sol": round(sum(t["sol"] for t in s0 if not t["buy"]), 4),
        "bundle_n_tx": len(bundle_sigs), "bundle_wallets": len(bundle_wallets),
        "bundle_sol": round(sum(t["sol"] for t in s0 if t["sig"] in bundle_sigs and t["buy"]), 4),
        "bundle_sizes": [round(t["sol"], 3) for t in s0 if t["sig"] in bundle_sigs and t["buy"]][:8],
        "third_n": len(third), "entry_mcap": entry, "eos0_mcap": eos0,
        "s1_mcap": next((t["mcap"] for t in reversed(tr) if t["slot"] <= r["slot"] + 1), eos0),
        "max_after": mx, "max_src": mx_src, "sol_usd": sol_usd,
        "ath_usd": ath_usd, "ath_ts": ath_ts, "complete": coin.get("complete"),
        "metadata_host_api": (coin.get("metadata_uri") or "")[:80],
        "twitter": bool(coin.get("twitter")), "website": bool(coin.get("website")),
        "coin_found": bool(coin),
    }
    with lock:
        fo.write(json.dumps(res, separators=(",", ":")) + "\n"); fo.flush()
        done.add(mint)
        if len(done) % 500 == 0:
            log(f"{len(done)} outcomes; api {api.stats}")


while True:
    with ThreadPoolExecutor(8) as ex:
        for f in [ex.submit(analyse, r) for r in todo]:
            try:
                f.result()
            except Exception as e:  # noqa: BLE001
                log("fail", type(e).__name__, str(e)[:160])
    log("pass done", len(done), api.stats)
    if "--follow" not in sys.argv:
        break
    time.sleep(30)
    rows = [json.loads(l) for l in open(OUT / "creates.jsonl")]
    rows = sorted((r for r in rows if r.get("kind") == "create"), key=lambda r: r["rank"])
    todo = [r for r in rows if r["mint"] not in done]
    if not todo:
        break
    coins.update(api.get_coins([r["mint"] for r in todo]))
log("done", len(done), api.stats)
