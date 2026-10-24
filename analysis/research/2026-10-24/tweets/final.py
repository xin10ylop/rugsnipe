# The verified rule, both days: hour by hour, pooled, both cost levels, and the steps that build it (ablation).
import json, sys, random, collections, bisect
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
H = int(sys.argv[1]) if len(sys.argv) > 1 else 10
T = json.load(open(SP + "/month/AHwJ_T.json")); land = collections.defaultdict(list)
for t in T:
    if t["stake"] >= 0.1: land[t["creator"]].append(t["bt"])
for v in land.values(): v.sort()
def n_in(c, lo, hi): v = land.get(c, []); return bisect.bisect_left(v, hi) - bisect.bisect_left(v, lo)
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
good = lambda r: bool(r["sid"] and r["delay"] is not None and r["delay"] < 3600 and r["tweet_rank"] == 1)
def stats(G, cost):
    G = [(w, v - cost) for w, v in G]
    if len(G) < 5: return None
    sw = sum(w for w, v in G); m = sum(w * v for w, v in G) / sw; win = sum(w for w, v in G if v > 0) / sw
    random.seed(5); bs = sorted(sum(w * v for w, v in s) / sum(w for w, v in s) for s in ([random.choice(G) for _ in G] for _ in range(1000)))
    return m, bs[25], bs[974], win, len(G), sw
fmt = lambda s: f"{100*s[0]:+6.2f}% [{100*s[1]:+6.2f},{100*s[2]:+6.2f}] win {100*s[3]:3.0f}% n {s[4]:3d}" if s else "n<5"
ALL = []
for day in (23, 24):
    R = json.load(open(SP + ("/univ/dataset.json" if day == 24 else f"/univ/dataset_{day}.json"))); bt = {r["mint"]: r["bt"] for r in R}
    rows = json.load(open(SP + f"/univ/rows_{day}.json"))
    U = [r for r in R if r["dev"] >= 0.95 and TEMPLATE(r["dev"]) and r["host"] in TOOLS]
    NU = collections.Counter((bool(r["he"]), good(r)) for r in U); NS = collections.Counter((bool(x["he"]), x["first"] and x["fresh"]) for x in rows)
    for x in rows:
        x["g"] = x["first"] and x["fresh"]; x["w"] = NU[(bool(x["he"]), x["g"])] / NS[(bool(x["he"]), x["g"])]
        t = bt[x["mint"]]; x["dep"] = n_in(x["creator"], t - 7 * 86400, t) >= 1; x["day"] = day
        x["cap"] = x["pb_sol"] is None or x["pb_sol"] <= 40
    ALL += rows
STEPS = {
  "1 tool host + template dev buy (the universe)": lambda x: True,
  "2 + post-bundle <= 40 SOL": lambda x: x["cap"],
  "3 + first coin on a fresh tweet": lambda x: x["cap"] and x["g"],
  "4 + a deployer he traded in the last 7 days (THE RULE)": lambda x: x["cap"] and x["g"] and x["dep"],
  "   deployer list without the tweet check": lambda x: x["cap"] and x["dep"],
  "   deployer list, tweet check failed": lambda x: x["cap"] and x["dep"] and not x["g"],
}
ok = lambda x: x.get(f"v{H}") is not None
print(f"hold {H} slots after entry; first outside buyer's realized value; 95% bootstrap CI; both days (Oct 23 + Oct 24, 00-12 UTC)")
for cost, lab in ((0.0177, "our cost 1.77%"), (0.0075, "his cost 0.75%")):
    print(f"\n== net of {lab}")
    for name, f in STEPS.items():
        G = [(x["w"], x[f"v{H}"]) for x in ALL if ok(x) and f(x)]
        cph = sum(x["w"] for x in ALL if f(x) and x["d"] is not None) / 24
        print(f"   {name:56s}: {fmt(stats(G, cost))}   {cph:5.1f} coins/h")
print(f"\n== THE RULE hour by hour (our cost)")
for day in (23, 24):
    cells = []
    for h in range(12):
        G = [(x["w"], x[f"v{H}"]) for x in ALL if x["day"] == day and x["hour"] == h and ok(x) and STEPS["4 + a deployer he traded in the last 7 days (THE RULE)"](x)]
        s = stats(G, 0.0177)
        cells.append(f"{100*s[0]:+5.1f}({s[4]})" if s else f"  -  ({len(G)})")
    print(f"   Oct {day}: " + " ".join(cells))
