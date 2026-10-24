# Does the copied wallet know the tweet status when he buys? On deployer-list coins (tool, template, cap, his 7-day list):
# his attempt rate on first coins on fresh tweets vs on copies, and where his buy lands (create slot or next).
import json, glob, collections, bisect
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
good = lambda r: bool(r["sid"] and r["delay"] is not None and r["delay"] < 3600 and r["tweet_rank"] == 1)
for day, path in ((23, "/pm/bak/dataset_23.json"), (24, "/univ/dataset.json")):
    D = json.load(open(SP + path))
    C = collections.Counter(); L = collections.Counter()
    for r in D:
        c = cr.get(r["mint"])
        if not c or r["host"] not in TOOLS or not TEMPLATE(r["dev"]) or r["hour"] >= 12 or not r["sid"]: continue
        if n_in(DC, c["creator"], r["bt"] - 7 * 86400, r["bt"]) + n_in(DS, c["signer"], r["bt"] - 7 * 86400, r["bt"]) == 0: continue
        k = "first coin, fresh tweet" if good(r) else ("copy" if r["tweet_rank"] > 1 else "stale tweet")
        C[(k, r["he"])] += 1
    print(f"== Oct {day} 00-12 UTC, deployer-list tweet coins (tool host, template dev buy)")
    tot_he = sum(v for (k, he), v in C.items() if he)
    for k in ("first coin, fresh tweet", "copy", "stale tweet"):
        n = C[(k, True)] + C[(k, False)]
        if n: print(f"   {k:24s}: {n:4d} coins, he attempted {C[(k, True)]:4d} ({100*C[(k, True)]/n:3.0f}%)  = {100*C[(k, True)]/max(1,tot_he):3.0f}% of his attempts")
