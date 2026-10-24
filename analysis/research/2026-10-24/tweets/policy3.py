# The rule A (tweet-first & fresh) combined with what the bot can know about his deployer list, both days.
#   B_live : he attempted this creator earlier the same day (his wallet feed, failed attempts included)
#   B_24h  : he has a landed trade on this creator in the 24 h before the create (no lag)
#   B_tab  : the same, from a table rebuilt every 4 h (his landed trades in the 24 h before the last rebuild)
#   B_7d   : a landed trade on this creator in the 7 days before
import json, os, sys, random, collections, bisect
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
H = int(sys.argv[1]) if len(sys.argv) > 1 else 8
T = json.load(open(SP + "/month/AHwJ_T.json"))
land = collections.defaultdict(list)
for t in T:
    if t["stake"] >= 0.1: land[t["creator"]].append(t["bt"])
for v in land.values(): v.sort()
def landed_in(creator, lo, hi, n=1):
    v = land.get(creator, []); i = bisect.bisect_left(v, lo); j = bisect.bisect_left(v, hi); return j - i >= n
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
good = lambda r: bool(r["sid"] and r["delay"] is not None and r["delay"] < 3600 and r["tweet_rank"] == 1)
def stats(G, cost):
    G = [(w, v - cost) for w, v in G]
    if len(G) < 5: return None
    sw = sum(w for w, v in G); m = sum(w * v for w, v in G) / sw; win = sum(w for w, v in G if v > 0) / sw
    random.seed(5); bs = sorted(sum(w * v for w, v in s) / sum(w for w, v in s) for s in ([random.choice(G) for _ in G] for _ in range(400)))
    return m, bs[10], bs[389], win, len(G), sw
fmt = lambda s: f"{100*s[0]:+6.2f}% [{100*s[1]:+6.2f},{100*s[2]:+6.2f}] n {s[4]:3d}" if s else "n<5".ljust(34)
RULES = {
  "A      tweet-first & fresh": lambda x: x["g"],
  "B_live A + he attempted creator earlier today": lambda x: x["g"] and (x["cph"] or 0) > 0,
  "B_24h  A + his landed trade on creator, last 24 h": lambda x: x["g"] and x["l24"],
  "B_tab  A + 4-hourly table of his last-24 h trades": lambda x: x["g"] and x["ltab"],
  "B_7d   A + his landed trade on creator, last 7 days": lambda x: x["g"] and x["l7d"],
  "B_any  A + (B_live or B_24h)": lambda x: x["g"] and ((x["cph"] or 0) > 0 or x["l24"]),
  "B_3d2  A + 2+ landed trades on creator, last 3 days (the box's table)": lambda x: x["g"] and x["l3d2"],
  "B_3d1  A + 1+ landed trade on creator, last 3 days": lambda x: x["g"] and x["l3d1"],
}
print(f"hold {H} slots after entry, post-bundle <= 40 SOL, net of our cost 1.77%  (TRAIN h0-7 | TEST h8-11, per day)")
out = collections.defaultdict(list)
for day in (23, 24):
    R = json.load(open(SP + ("/univ/dataset.json" if day == 24 else f"/univ/dataset_{day}.json")))
    rows = json.load(open(SP + f"/univ/rows_{day}.json"))
    U = [r for r in R if r["dev"] >= 0.95 and TEMPLATE(r["dev"]) and r["host"] in TOOLS]
    bt = {r["mint"]: r["bt"] for r in R}
    NU = collections.Counter((bool(r["he"]), good(r)) for r in U); NS = collections.Counter((bool(x["he"]), x["first"] and x["fresh"]) for x in rows)
    for x in rows:
        x["g"] = x["first"] and x["fresh"]; x["w"] = NU[(bool(x["he"]), x["g"])] / NS[(bool(x["he"]), x["g"])]
        t = bt[x["mint"]]; R4 = (t - 50 * 60) // (4 * 3600) * (4 * 3600) + 50 * 60   # rebuilds at hh:50 every 4 h
        x["l24"] = landed_in(x["creator"], t - 86400, t); x["ltab"] = landed_in(x["creator"], R4 - 86400, R4); x["l7d"] = landed_in(x["creator"], t - 7 * 86400, t)
        x["l3d2"] = landed_in(x["creator"], R4 - 3 * 86400, R4, 2); x["l3d1"] = landed_in(x["creator"], R4 - 3 * 86400, R4, 1)
    cap = lambda x: x["pb_sol"] is None or x["pb_sol"] <= 40
    for name, f in RULES.items():
        for half, sel in (("train", lambda x: x["hour"] < 8), ("test", lambda x: x["hour"] >= 8)):
            G = [(x["w"], x[f"v{H}"]) for x in rows if sel(x) and cap(x) and f(x) and x.get(f"v{H}") is not None]
            out[name].append((f"Oct {day} {half}", stats(G, 0.0177)))
        out[name].append((f"Oct {day} /h", sum(x["w"] for x in rows if cap(x) and f(x) and x["d"] is not None) / 12))
for name, cells in out.items():
    print(f"\n  {name}")
    for lab, s in cells:
        print(f"     {lab:12s}: " + (f"{s:.1f} coins/h" if isinstance(s, float) else fmt(s)))
