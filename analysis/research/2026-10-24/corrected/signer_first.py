# Can the deployer's own history stand in for the tweet check (known at create time, no fetch)?
# For each deployer-list coin: the signer's earlier coins in the data (tool coins with a tweet), and how many were the
# first coin on a fresh tweet. Buy at full speed only when that past first-rate is high; value hold 10, net 1.77%.
import json, glob, collections, bisect, random
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
def stats(v):
    v = [x - 0.0177 for x in v]
    if len(v) < 5: return f"n {len(v)}"
    m = sum(v) / len(v); random.seed(5); bs = sorted(sum(random.choice(v) for _ in v) / len(v) for _ in range(1000))
    return f"{100*m:+6.2f}% [{100*bs[25]:+6.2f},{100*bs[974]:+6.2f}] n {len(v):3d}"
for day, path in ((23, "/pm/bak/dataset_23.json"), (24, "/univ/dataset.json")):
    D = json.load(open(SP + path)); D.sort(key=lambda r: (r["slot"]))
    rows = {x["mint"]: x for x in json.load(open(SP + (f"/pm/bak/rows_23.json" if day == 23 else "/univ/rows_24.json")))}
    hist = collections.defaultdict(lambda: [0, 0])  # signer -> [tweet coins seen, first-fresh]
    out = collections.defaultdict(list); out_all = []
    for r in D:
        c = cr.get(r["mint"])
        if not c: continue
        sg = c["signer"]
        if r["host"] in TOOLS and r["dev"] >= 0.95:
            x = rows.get(r["mint"])
            if x and TEMPLATE(r["dev"]) and (x["pb_sol"] is None or x["pb_sol"] <= 40) and x.get("v10") is not None and \
               n_in(DC, c["creator"], r["bt"] - 7 * 86400, r["bt"]) + n_in(DS, sg, r["bt"] - 7 * 86400, r["bt"]) > 0:
                n, k = hist[sg]
                b = "no history" if n < 3 else ("first-rate >= 80%" if k / n >= 0.8 else "50-80%" if k / n >= 0.5 else "under 50%")
                out[b].append((x["v10"], good(r))); out_all.append(x["v10"])
            if r["sid"]:
                hist[sg][0] += 1; hist[sg][1] += good(r)
    print(f"== Oct {day} 00-12 UTC, deployer-list coins, buy at full speed, hold 10 (unweighted)")
    print(f"   all                    : {stats(out_all)}")
    for b in ("first-rate >= 80%", "50-80%", "under 50%", "no history"):
        v = out[b]
        if v: print(f"   signer {b:18s}: {stats([a for a, g in v])}   of them truly first: {100*sum(g for a, g in v)/len(v):3.0f}%")
