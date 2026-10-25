# His own results on the coins outside the rule vs the rule coins (closed trades, Oct 23 and Oct 24 00-12).
import json, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
D = {}
for day in (23, 24):
    for r in json.load(open(SP + f"/pm/dataset_{day}.json")):
        r["day"] = day; r["per"] = "23a" if day == 23 and r["hour"] < 12 else "23b" if day == 23 else ("24a" if r["hour"] < 12 else "24b"); D[r["mint"]] = r
T = [t for t in json.load(open(SP + "/month/AHwJ_T.json")) if t["mint"] in D and D[t["mint"]]["per"] != "24b" and t["stake"] >= 0.1]
g = collections.defaultdict(list)
for t in T:
    r = D[t["mint"]]; k = "rule" if r["host"] in TOOLS and TEMPLATE(r["dev"]) else ("outside: tool host" if r["host"] in TOOLS else "outside: " + ("ipfs.io" if r["host"] == "ipfs.io" else "other host"))
    g[(k, r["per"])].append(t); g[(k, "all")].append(t)
for k in sorted(g):
    v = g[k]; ret = [t["pnl"] / t["stake"] for t in v]
    print(f"{k[0]:22s} {k[1]:4s} trades {len(v):4d} mean {100*sum(ret)/len(ret):+6.2f}%  P&L {sum(t['pnl'] for t in v):+7.2f} SOL  win {100*sum(x>0 for x in ret)/len(ret):.0f}%")
