# Policy backtest on the template+tool universe, stratum-weighted (his attempts are all sampled, the rest partly):
#   gate     : buy only tweet-first & fresh coins, hold H (assumes the tweet check is free in time)
#   spec     : buy every template+tool coin at once; tweet-first & fresh -> hold H, else sell after X slots
#   none     : buy every template+tool coin, hold H
# value = the first outside buyer's realized position (before tips/tx fees), minus a round-trip cost.
#   RS_DAY=24 python3 -I univ/policy.py [H] [X]
import json, os, sys, random, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
DAYN = int(os.environ.get("RS_DAY", "24")); H = int(sys.argv[1]) if len(sys.argv) > 1 else 8; X = int(sys.argv[2]) if len(sys.argv) > 2 else 2
R = json.load(open(SP + ("/univ/dataset.json" if DAYN == 24 else f"/univ/dataset_{DAYN}.json")))
rows = json.load(open(SP + f"/univ/rows_{DAYN}.json"))
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
U = [r for r in R if r["dev"] >= 0.95 and TEMPLATE(r["dev"]) and r["host"] in TOOLS]
good = lambda r: bool(r["sid"] and r["delay"] is not None and r["delay"] < 3600 and r["tweet_rank"] == 1)
stratum = lambda r, g: (bool(r["he"]), g)
NU = collections.Counter(stratum(r, good(r)) for r in U)
NS = collections.Counter(stratum(x, x["first"] and x["fresh"]) for x in rows)
for x in rows: x["g"] = x["first"] and x["fresh"]; x["w"] = NU[stratum(x, x["g"])] / NS[stratum(x, x["g"])]
print(f"Oct {DAYN}: template+tool creates {len(U)} = {len(U)/12:.0f}/h; tweet-first & fresh {sum(good(r) for r in U)} ({sum(good(r) for r in U)/len(U):.0%}); "
      f"rows {len(rows)}; weights { {f'{k[0] and 'his' or 'rest'}/{k[1] and 'first' or 'other'}': round(NU[k]/NS[k],2) for k in NU if NS[k]} }")
def pol(name, x):
    if x["d"] is None: return None
    vh, vx = x.get(f"v{H}"), x.get(f"v{X}")
    if name == "none": return vh
    if name == "gate": return vh if x["g"] else "skip"
    if name == "spec": return vh if x["g"] else vx
CAPS = {"no cap": lambda x: True, "post-bundle <= 40 SOL": lambda x: x["pb_sol"] is None or x["pb_sol"] <= 40}
def stats(G, cost):
    G = [(w, v - cost) for w, v in G]
    if len(G) < 5: return None
    sw = sum(w for w, v in G); m = sum(w * v for w, v in G) / sw; win = sum(w for w, v in G if v > 0) / sw
    random.seed(5); bs = sorted(sum(w * v for w, v in s) / sum(w for w, v in s) for s in ([random.choice(G) for _ in G] for _ in range(400)))
    return m, bs[10], bs[389], win, len(G), sw
def fmt(s): return f"{100*s[0]:+6.2f}% [{100*s[1]:+6.2f},{100*s[2]:+6.2f}] win {100*s[3]:3.0f}% n {s[4]:3d}" if s else "n<5".ljust(44)
for cost, lab in ((0.0177, "our cost 1.77%"), (0.0075, "his cost 0.75%")):
    print(f"\n== hold {H} slots (spec exits others after {X}); net of {lab}")
    print(f"   {'policy':26s} {'cap':22s} | {'TRAIN h0-7':44s} | {'TEST h8-11':44s} | buys/h")
    for cap, cf in CAPS.items():
        for name in ("none", "gate", "spec"):
            o = []; nb = 0
            for sel in (lambda x: x["hour"] < 8, lambda x: x["hour"] >= 8):
                G = []
                for x in rows:
                    if not sel(x) or not cf(x): continue
                    v = pol(name, x)
                    if v is None or v == "skip": continue
                    G.append((x["w"], v))
                o.append(fmt(stats(G, cost)))
            nb = sum(x["w"] for x in rows if cf(x) and pol(name, x) not in (None, "skip")) / 12
            print(f"   {name:26s} {cap:22s} | {o[0]} | {o[1]} | {nb:5.1f}")
print(f"\n== spec, post-bundle <= 40, by hour (our cost)")
for h in range(12):
    G = [(x["w"], pol("spec", x)) for x in rows if x["hour"] == h and (x["pb_sol"] is None or x["pb_sol"] <= 40) and pol("spec", x) not in (None, "skip")]
    print(f"   hour {h:2d}: {fmt(stats(G, 0.0177))}")
print(f"\n== within spec (post-bundle <= 40): tweet-first&fresh vs other, and by his prior attempts on the creator (our cost)")
for lab, f in (("first&fresh", lambda x: x["g"]), ("other", lambda x: not x["g"]), ("first&fresh, creator he-attempted before", lambda x: x["g"] and (x["cph"] or 0) > 0),
               ("first&fresh, creator new to him", lambda x: x["g"] and not (x["cph"] or 0)), ("first&fresh, he attempted THIS coin", lambda x: x["g"] and x["he"]),
               ("first&fresh, he skipped this coin", lambda x: x["g"] and not x["he"])):
    G = [(x["w"], pol("spec", x)) for x in rows if f(x) and (x["pb_sol"] is None or x["pb_sol"] <= 40) and pol("spec", x) not in (None, "skip")]
    print(f"   {lab:42s}: {fmt(stats(G, 0.0177))}")
