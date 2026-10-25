# 6anjqMe, Oct 21 -> now: his round trips on SOL-priced pump coins from the trade events (he pays through a WSOL
# account, so his native balance does not show the trade): cost = SOL in + pump fees + tx fee + tips, proceeds = SOL
# out - pump fees - tx fee. Per day and by the slices a copy could key on: the curve level at his buy (v before; a
# fresh curve is 30 SOL), his tip, his size, his hold, a creator he bought >= 2 times in the previous 24 h.
import json, collections, time, statistics as st, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = "6anjqMeWigQFo5wpciARAunVQUXTqoat8w8fFWarCkpc"
rows = {}
for p in (SP + "/month/6anjqMeW.jsonl", SP + "/six/6anjqMeW_recent.jsonl"):
    try:
        for l in open(p):
            r = json.loads(l)
            if (r.get("bt") or 0) >= 1792540800: rows[r["sig"]] = r
    except FileNotFoundError: pass
rows = sorted(rows.values(), key=lambda r: (r["s"], r["i"] or 0))
print(f"{len(rows)} txs, {time.strftime('%m-%d %H:%M', time.gmtime(rows[0]['bt']))} -> {time.strftime('%m-%d %H:%M', time.gmtime(rows[-1]['bt']))}")
coins = collections.defaultdict(lambda: {"buys": [], "sells": [], "fails": 0, "fail_fee": 0})
for r in rows:
    mine = [t for t in r["tr"] if t[1] == W]
    if r["err"]:
        if r["mints"]: c = coins[r["mints"][0]]; c["fails"] += 1; c["fail_fee"] += r["fee"] or 0
        continue
    for k, t in enumerate(mine):
        mint, user, is_buy, sol, tok, v, vt, real, creator, fee, cfee, ixn = t
        txc = ((r["fee"] or 0) + (r["tip"] or 0)) if k == 0 else 0
        rec = dict(slot=r["s"], idx=r["i"], bt=r["bt"], sol=sol, tok=tok, v_after=v, creator=creator, tip=r["tip"] or 0, cost=txc, fee=fee + cfee,
                   sol_quote=abs((v - real) - 30_000_000_000) < 1_000_000_000)
        coins[mint]["buys" if is_buy else "sells"].append(rec)
trips = []; other = set()
for m, c in coins.items():
    if not c["buys"]: continue
    b0 = c["buys"][0]
    if not b0["sol_quote"]:
        other.add(m); continue
    bought = sum(b["tok"] for b in c["buys"]); sold = sum(s["tok"] for s in c["sells"])
    closed = sold >= 0.999 * bought
    spent = sum(b["sol"] + b["fee"] + b["cost"] for b in c["buys"])
    got = sum(s["sol"] - s["fee"] - s["cost"] for s in c["sells"])
    trips.append(dict(m=m, bt=b0["bt"], day=time.strftime("%m-%d", time.gmtime(b0["bt"])), stake=spent / 1e9,
                      pnl=(got - spent - c["fail_fee"]) / 1e9 if closed else None, closed=closed, tip=b0["tip"] / 1e6,
                      v_before=(b0["v_after"] - b0["sol"]) / 1e9, creator=b0["creator"], n_buys=len(c["buys"]),
                      hold_s=(c["sells"][-1]["bt"] - b0["bt"]) if c["sells"] else None,
                      hold_slots=(c["sells"][0]["slot"] - b0["slot"]) if c["sells"] else None))
trips.sort(key=lambda t: t["bt"]); seen = collections.defaultdict(list)
for t in trips:
    p = seen[t["creator"]]; t["rep"] = sum(1 for x in p if x >= t["bt"] - 86400); p.append(t["bt"])
json.dump(trips, open(SP + "/six/trips.json", "w"))
print(f"coins priced in another token (not tradable by the bot, left out): {len(other)}")
C = [t for t in trips if t["closed"]]
days = sorted({t["day"] for t in C})
print(f"{len(trips)} coins bought, {len(C)} closed; his net on them {sum(t['pnl'] for t in C):+.2f} SOL")
print(f"median stake {st.median(t['stake'] for t in C):.3f} SOL, median hold {st.median(t['hold_slots'] for t in C if t['hold_slots'] is not None)} slots, "
      f"median curve level at his buy {st.median(t['v_before'] for t in C):.1f} SOL (a fresh curve: 30)")
def show(lab, f):
    s = [t for t in C if f(t)]
    byd = collections.defaultdict(float)
    for t in s: byd[t["day"]] += t["pnl"]
    tot = sum(t["pnl"] for t in s)
    print(f"   {lab:40} n {len(s):5d} net {tot:+7.2f}  /buy {tot / max(len(s), 1):+.4f}  win {100 * sum(t['pnl'] > 0 for t in s) / max(len(s), 1):3.0f}%  " + " ".join(f"{d}:{byd[d]:+.2f}" for d in days))
print("== by day, all"); show("all", lambda t: True)
print("== curve level at his buy (v before, SOL)")
for lo, hi in ((0, 32), (32, 36), (36, 45), (45, 60), (60, 90), (90, 1e9)):
    show(f"{lo}-{hi if hi < 1e8 else 'up'}", lambda t, lo=lo, hi=hi: lo <= t["v_before"] < hi)
print("== his tip (M)")
for lo, hi in ((0, 0.5), (0.5, 1.5), (1.5, 2.5), (2.5, 4), (4, 8), (8, 1e9)):
    show(f"{lo}-{hi if hi < 1e8 else 'up'}", lambda t, lo=lo, hi=hi: lo <= t["tip"] < hi)
print("== his size (SOL)")
for lo, hi in ((0, 0.3), (0.3, 0.6), (0.6, 1.0), (1.0, 2.0), (2.0, 1e9)):
    show(f"{lo}-{hi if hi < 1e8 else 'up'}", lambda t, lo=lo, hi=hi: lo <= t["stake"] < hi)
print("== his hold (slots to his first sell)")
for lo, hi in ((0, 2), (2, 5), (5, 10), (10, 30), (30, 1e9)):
    show(f"{lo}-{hi if hi < 1e8 else 'up'}", lambda t, lo=lo, hi=hi: t["hold_slots"] is not None and lo <= t["hold_slots"] < hi)
print("== the coin's creator bought by him before (previous 24 h)")
for k in (0, 1, 2, 5):
    show(f">= {k}", lambda t, k=k: t["rep"] >= k)
