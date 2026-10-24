# If the tweet decided the buy: on the rule's coins (first coin on a fresh tweet + deployer list + template + tool + cap),
# when did the first outside buy land (slots after the create), and what did that first buyer make (hold 10, net 1.77%)?
# A verdict at slot +2..+3 can only buy the coins nobody bought before it.
import json, glob, collections, bisect, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
DC = collections.defaultdict(list); DS = collections.defaultdict(list)
for t in json.load(open(SP + "/month/AHwJ_T.json")):
    if t["stake"] < 0.1: continue
    DC[t["creator"]].append(t["bt"])
    if t["mint"] in cr: DS[cr[t["mint"]]["signer"]].append(t["bt"])
for d in (DC, DS):
    for v in d.values(): v.sort()
n_in = lambda d, k, lo, hi: bisect.bisect_left(d.get(k, []), hi) - bisect.bisect_left(d.get(k, []), lo)
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
for day, hours in [(int(a.split(":")[0]), a.split(":")[1]) for a in (sys.argv[1:] or ["23:0-12", "24:0-12"])]:
    lo, hi = map(int, hours.split("-"))
    rows = json.load(open(SP + f"/univ/rows_{day}.json"))
    bt = {r["mint"]: r["bt"] for r in json.load(open(SP + ("/univ/dataset.json" if day == 24 else f"/univ/dataset_{day}.json")))}
    B = collections.defaultdict(list); none = 0; n = 0
    for x in rows:
        c = cr.get(x["mint"]); t = bt.get(x["mint"])
        if not c or t is None or not (lo <= x["hour"] < hi) or x["host"] not in TOOLS or not TEMPLATE(x["dev"]): continue
        if not (x["pb_sol"] is None or x["pb_sol"] <= 40) or not (x["first"] and x["fresh"]): continue
        if n_in(DC, c["creator"], t - 7 * 86400, t) + n_in(DS, c["signer"], t - 7 * 86400, t) == 0: continue
        n += 1
        if x["d"] is None: none += 1; continue
        if x.get("v10") is None: continue
        B[min(x["d"], 4)].append(x["v10"] - 0.0177)
    print(f"Oct {day} {lo:02d}-{hi:02d} UTC: {n} rule coins ({n/(hi-lo):.1f}/h); no outside buy at all in the tape: {none}")
    for k in range(5):
        v = B[k]
        if v: print(f"   first outside buy at create {'+' + str(k) if k < 4 else '+4 or later'}: {len(v):4d} coins ({100*len(v)/n:3.0f}%)  first buyer made {100*sum(v)/len(v):+6.2f}% per trade")
    late = B[3] + B[4]; later = B[4]
    for lab, v in (("a buy at +3 (verdict in time)", late), ("a buy at +4", later)):
        if v: print(f"   -> {lab}: {len(v)} coins, {len(v)/(hi-lo):.1f}/h, {100*sum(v)/len(v):+.2f}% per trade")
