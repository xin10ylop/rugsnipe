# Him vs us today: per-trade results, costs, stake sizing, and the launches where both fired.
import json, statistics as St, math, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
T = json.load(open(SP + "/today/trades.json"))
LU = json.load(open(SP + "/today/legs_us.json")); LH = json.load(open(SP + "/today/legs_him.json"))
def ci(v):
    if len(v) < 2: return f"n={len(v)}"
    m = St.mean(v); h = 1.96 * St.pstdev(v) / math.sqrt(len(v))
    return f"n={len(v):3d} wins {sum(1 for x in v if x > 0):3d}  mean {100*m:+6.2f}% [{100*(m-h):+6.2f},{100*(m+h):+6.2f}]  median {100*St.median(v):+6.2f}%"
for who, L in (("us", LU), ("him", LH)):
    fb = [x for x in L if x["kind"] == "buy" and x["err"]]
    lb = [x for x in L if x["kind"] == "buy" and not x["err"]]
    print(f"{who:3s}: buy txs {len(lb) + len(fb)} (landed {len(lb)}, failed on chain {len(fb)}, failed cost {-sum(x['delta'] for x in fb)/1e9:.4f} SOL = "
          f"{-sum(x['delta'] for x in fb)/max(len(fb),1):.0f} lamports each)")
H = [r for r in T if r["him"] and r["him"]["closed"]]; U = [r for r in T if r["us"] and r["us"]["closed"]]
print("\n== landed trades, chain-true after the landed txs' fees and tips")
print("  us :", ci([r["us"]["net"] / r["us"]["cost"] for r in U]), f" SOL {sum(r['us']['net'] for r in U):+.4f}  stake median {St.median([r['us']['cost'] for r in U]):.2f}")
print("  him:", ci([r["him"]["net"] / r["him"]["cost"] for r in H]), f" SOL {sum(r['him']['net'] for r in H):+.4f}  stake median {St.median([r['him']['cost'] for r in H]):.2f}")
# his costs per landed round trip (fees + tips + priority of his landed buy and sells), vs ours
def txcost(L, mints):
    c = collections.defaultdict(float)
    for x in L:
        if x["mint"] in mints and x["kind"] in ("buy", "sell") and not x["err"]:
            c[x["mint"]] += (x["fee"] + x["tip"]) / 1e9
    return c
cu = txcost(LU, {r["mint"] for r in U}); ch = txcost(LH, {r["mint"] for r in H})
print(f"  tx fees + tips per round trip: us median {1000*St.median(cu.values()):.2f} mSOL ({100*St.median([cu[r['mint']]/r['us']['cost'] for r in U]):.2f}% of stake),"
      f" him median {1000*St.median(ch.values()):.2f} mSOL ({100*St.median([ch[r['mint']]/r['him']['cost'] for r in H]):.2f}% of stake)")
print("\n== his trades by stake (he sizes by conviction)")
for lo, hi in ((0, 0.55), (0.55, 1.0), (1.0, 1.7), (1.7, 99)):
    G = [r for r in H if lo <= r["him"]["cost"] < hi]
    if G: print(f"  stake {lo:.2f}-{hi:.2f} SOL: {ci([r['him']['net'] / r['him']['cost'] for r in G])}  SOL {sum(r['him']['net'] for r in G):+.4f}")
# the launches he landed: did we try? and what did he make there
ours_try = collections.defaultdict(list)
for x in LU:
    if x["kind"] == "buy" and x["mint"]: ours_try[x["mint"]].append(x)
print("\n== the launches HE landed, by what we did")
for lab, sel in (("we also fired (and lost the race)", lambda r: r["mint"] in ours_try and not r["us"]), ("we did not fire", lambda r: r["mint"] not in ours_try)):
    G = [r for r in H if sel(r)]
    print(f"  {lab:34s} {ci([r['him']['net'] / r['him']['cost'] for r in G])}  SOL {sum(r['him']['net'] for r in G):+.4f}")
# his bid on the races he won against us
won = [r for r in H if r["mint"] in ours_try and not r["us"]]
b = [(x["tip"] / 1e6, (x["cup"] or 0) * (x["cul"] or 0) / 1e12) for r in won for x in LH if x["mint"] == r["mint"] and x["kind"] == "buy" and not x["err"]]
if b: print(f"  his winning bid against us: tip median {St.median([t for t, p in b]):.2f}M, priority median {St.median([p for t, p in b]):.3f}M (n={len(b)})")
