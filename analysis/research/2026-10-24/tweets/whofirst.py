# On the verified rule's coins (A + his landed trade on the creator in 7 days, post-bundle <= 40): the realized value by who
# took the first spot (us / him / a rival), both days, before costs; and our own bot's chain-true trades on those coins.
import json, sys, collections, bisect, statistics as St, math
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
H = int(sys.argv[1]) if len(sys.argv) > 1 else 10
T = json.load(open(SP + "/month/AHwJ_T.json")); land = collections.defaultdict(list)
for t in T:
    if t["stake"] >= 0.1: land[t["creator"]].append(t["bt"])
for v in land.values(): v.sort()
def n_in(c, lo, hi): v = land.get(c, []); return bisect.bisect_left(v, hi) - bisect.bisect_left(v, lo)
def ci(v):
    if len(v) < 3: return f"n={len(v)}"
    m = St.mean(v); h = 1.96 * St.pstdev(v) / math.sqrt(len(v)); return f"n={len(v):4d} {100*m:+6.2f}% [{100*(m-h):+6.2f},{100*(m+h):+6.2f}] med {100*St.median(v):+6.2f}%"
for day in (23, 24):
    R = {r["mint"]: r for r in json.load(open(SP + ("/univ/dataset.json" if day == 24 else f"/univ/dataset_{day}.json")))}
    rows = json.load(open(SP + f"/univ/rows_{day}.json"))
    G = collections.defaultdict(list)
    for x in rows:
        t = R[x["mint"]]["bt"]
        if not (x["first"] and x["fresh"] and (x["pb_sol"] is None or x["pb_sol"] <= 40) and n_in(x["creator"], t - 7 * 86400, t) >= 1): continue
        if x.get(f"v{H}") is None: continue
        G[x["who"]].append(x[f"v{H}"]); G["all"].append(x[f"v{H}"])
    print(f"Oct {day}: rule coins with a first outside buy, value at entry + {H} (before costs)")
    for k in ("all", "rival", "him", "us"): print(f"   first = {k:5s}: {ci(G[k])}")
