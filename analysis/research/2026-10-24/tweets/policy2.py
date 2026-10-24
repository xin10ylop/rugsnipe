# Pre-registered entry rules on the template+tool universe (post-bundle <= 40 SOL), first outside buyer's realized value at
# entry + H, stratum weights; TRAIN h0-7 / TEST h8-11 / per hour.   RS_DAY=24 python3 -I univ/policy2.py H
import json, os, sys, random, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
DAYN = int(os.environ.get("RS_DAY", "24")); H = int(sys.argv[1]) if len(sys.argv) > 1 else 8
R = json.load(open(SP + ("/univ/dataset.json" if DAYN == 24 else f"/univ/dataset_{DAYN}.json")))
rows = json.load(open(SP + f"/univ/rows_{DAYN}.json"))
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
U = [r for r in R if r["dev"] >= 0.95 and TEMPLATE(r["dev"]) and r["host"] in TOOLS]
good = lambda r: bool(r["sid"] and r["delay"] is not None and r["delay"] < 3600 and r["tweet_rank"] == 1)
NU = collections.Counter((bool(r["he"]), good(r)) for r in U); NS = collections.Counter((bool(x["he"]), x["first"] and x["fresh"]) for x in rows)
for x in rows: x["g"] = x["first"] and x["fresh"]; x["w"] = NU[(bool(x["he"]), x["g"])] / NS[(bool(x["he"]), x["g"])]
RULES = {
  "A  tweet-first & fresh": lambda x: x["g"],
  "B  A + he attempted this creator earlier today": lambda x: x["g"] and (x["cph"] or 0) > 0,
  "B2 A + he attempted this creator 2+ times today": lambda x: x["g"] and (x["cph"] or 0) >= 2,
  "C  A + creator new to him today": lambda x: x["g"] and not (x["cph"] or 0),
  "D  he attempted this creator earlier (no tweet check)": lambda x: (x["cph"] or 0) > 0,
  "E  not A (other template+tool coins)": lambda x: not x["g"],
}
cap = lambda x: x["pb_sol"] is None or x["pb_sol"] <= 40
def stats(G, cost):
    G = [(w, v - cost) for w, v in G]
    if len(G) < 5: return None
    sw = sum(w for w, v in G); m = sum(w * v for w, v in G) / sw; win = sum(w for w, v in G if v > 0) / sw
    random.seed(5); bs = sorted(sum(w * v for w, v in s) / sum(w for w, v in s) for s in ([random.choice(G) for _ in G] for _ in range(400)))
    return m, bs[10], bs[389], win, len(G), sw
fmt = lambda s: f"{100*s[0]:+6.2f}% [{100*s[1]:+6.2f},{100*s[2]:+6.2f}] win {100*s[3]:3.0f}% n {s[4]:3d}" if s else "n<5".ljust(44)
print(f"Oct {DAYN}, hold {H} slots after entry, post-bundle <= 40 SOL, net of our cost 1.77%")
print(f"   {'rule':52s} | {'TRAIN h0-7':44s} | {'TEST h8-11':44s} | buys/h")
for name, f in RULES.items():
    o = []
    for sel in (lambda x: x["hour"] < 8, lambda x: x["hour"] >= 8):
        o.append(fmt(stats([(x["w"], x[f"v{H}"]) for x in rows if sel(x) and cap(x) and f(x) and x.get(f"v{H}") is not None], 0.0177)))
    nb = sum(x["w"] for x in rows if cap(x) and f(x) and x["d"] is not None) / 12
    print(f"   {name:52s} | {o[0]} | {o[1]} | {nb:5.1f}")
for name in ("A  tweet-first & fresh", "B  A + he attempted this creator earlier today"):
    print(f"\n   {name} by hour:")
    for h in range(12):
        s = stats([(x["w"], x[f"v{H}"]) for x in rows if x["hour"] == h and cap(x) and RULES[name](x) and x.get(f"v{H}") is not None], 0.0177)
        print(f"      hour {h:2d}: {fmt(s)}")
