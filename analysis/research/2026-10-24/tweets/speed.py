# Is the tweet check fast enough? On the deployer-list coins (tool + template dev buy + post-bundle <= 40 + a deployer he
# traded in the last 7 days), both days: when the first outside buy lands (create slot / next / later), the first buyer's
# value by hold, for tweet-first and other coins; our own race outcomes (Oct 24) by the first buy's slot; and each
# deployer's earlier tweet-first share (a shortcut that needs no fetch).
import json, glob, sys, random, collections, bisect, statistics as St, math
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
T = json.load(open(SP + "/month/AHwJ_T.json")); land = collections.defaultdict(list)
for t in T:
    if t["stake"] >= 0.1: land[t["creator"]].append(t["bt"])
for v in land.values(): v.sort()
def n_in(c, lo, hi): v = land.get(c, []); return bisect.bisect_left(v, hi) - bisect.bisect_left(v, lo)
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
good = lambda r: bool(r["sid"] and r["delay"] is not None and r["delay"] < 3600 and r["tweet_rank"] == 1)
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
def wmean(G, cost=0.0177):
    G = [(w, v - cost) for w, v in G if v is not None]
    if len(G) < 5: return "n<5"
    sw = sum(w for w, v in G); m = sum(w * v for w, v in G) / sw
    random.seed(5); bs = sorted(sum(w * v for w, v in s) / sum(w for w, v in s) for s in ([random.choice(G) for _ in G] for _ in range(600)))
    return f"{100*m:+6.2f}% [{100*bs[15]:+6.2f},{100*bs[584]:+6.2f}] n {len(G)}"
ALL = []
for day in (23, 24):
    R = json.load(open(SP + ("/univ/dataset.json" if day == 24 else f"/univ/dataset_{day}.json"))); bt = {r["mint"]: r["bt"] for r in R}
    rows = json.load(open(SP + f"/univ/rows_{day}.json"))
    U = [r for r in R if r["dev"] >= 0.95 and TEMPLATE(r["dev"]) and r["host"] in TOOLS]
    NU = collections.Counter((bool(r["he"]), good(r)) for r in U); NS = collections.Counter((bool(x["he"]), x["first"] and x["fresh"]) for x in rows)
    # each deployer's earlier tweet-first share, in create order over both days (signer key)
    for x in rows:
        x["g"] = x["first"] and x["fresh"]; x["w"] = NU[(bool(x["he"]), x["g"])] / NS[(bool(x["he"]), x["g"])]
        t = bt[x["mint"]]; x["day"] = day; x["bt"] = t
        x["dep"] = n_in(x["creator"], t - 7 * 86400, t) >= 1 and (x["pb_sol"] is None or x["pb_sol"] <= 40)
        x["signer"] = cr[x["mint"]]["signer"]
    ALL += rows
# deployer history from the full datasets (every tool coin with metadata, both days), by signer, in time order
hist = collections.defaultdict(list)
evs = []
for day in (23, 24):
    R = json.load(open(SP + ("/univ/dataset.json" if day == 24 else f"/univ/dataset_{day}.json")))
    for r in R:
        if r["host"] in TOOLS and r["dev"] >= 0.95 and r["mint"] in cr: evs.append((r["bt"], cr[r["mint"]]["signer"], good(r)))
evs.sort()
for t, s, g in evs: hist[s].append((t, g))
def prior(s, t):
    v = [g for tt, g in hist.get(s, []) if tt < t]
    return (len(v), sum(v) / len(v) if v else None)
L = [x for x in ALL if x["dep"] and x["d"] is not None]
print(f"deployer-list coins with a first outside buy: {len(L)} (weighted {sum(x['w'] for x in L):.0f}); tweet-first share "
      f"{sum(x['w'] for x in L if x['g'])/sum(x['w'] for x in L):.0%}")
dd = collections.Counter(); 
for x in L: dd[min(x["d"], 2)] += x["w"]
tot = sum(dd.values())
print("first outside buy lands: create slot {:.0%}, next slot {:.0%}, later {:.0%}".format(dd[0] / tot, dd[1] / tot, dd[2] / tot))
for lab, f in (("tweet-first", lambda x: x["g"]), ("not tweet-first", lambda x: not x["g"])):
    print(f"\n  {lab}:")
    for k in (1, 2, 3, 5, 10):
        print(f"     first buyer, value at entry + {k:2d} slots (net of our cost): {wmean([(x['w'], x.get(f'v{k}')) for x in L if f(x)])}")
    for d in (0, 1):
        print(f"     first buy in slot create+{d}: value at +10: {wmean([(x['w'], x.get('v10')) for x in L if f(x) and min(x['d'], 1) == d])}")
# the shortcut: the deployer's earlier coins' tweet-first share
print("\n  the deployer's earlier tool coins (both days, signer key):")
for lab, sel in (("no earlier coin", lambda n, s: n == 0), ("earlier, < 60% tweet-first", lambda n, s: n > 0 and s < 0.6),
                 ("earlier, 60-85%", lambda n, s: n > 0 and 0.6 <= s < 0.85), ("earlier, >= 85%", lambda n, s: n > 0 and s >= 0.85),
                 ("3+ earlier, >= 85%", lambda n, s: n >= 3 and s >= 0.85)):
    G = [x for x in L if sel(*prior(x["signer"], x["bt"]))]
    if not G: continue
    sw = sum(x["w"] for x in G); fr = sum(x["w"] for x in G if x["g"]) / sw
    print(f"     {lab:28s} coins {len(G):4d}  this coin tweet-first {fr:.0%}  fire on all of them, hold 10: {wmean([(x['w'], x.get('v10')) for x in G])}")
json.dump([{k: x[k] for k in ('mint', 'day', 'hour', 'g', 'w', 'd', 'who', 'v1', 'v2', 'v10', 'signer', 'bt') if k in x} for x in L], open(SP + "/univ/speed_rows.json", "w"))
