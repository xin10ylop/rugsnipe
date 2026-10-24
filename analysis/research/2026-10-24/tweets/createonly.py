# Create-time-only rules (no metadata fetch) vs the tweet check, on template+tool coins, post-bundle <= 40 SOL.
# Outcome: first outside buyer's realized value at entry + H, stratum-weighted, net of our cost 1.77%.
#   RS_DAY=24 python3 -I univ/createonly.py H
import json, glob, collections, os, random, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
DAYN = int(os.environ.get("RS_DAY", "24")); H = int(sys.argv[1]) if len(sys.argv) > 1 else 8
R = json.load(open(SP + ("/univ/dataset.json" if DAYN == 24 else f"/univ/dataset_{DAYN}.json")))
creates = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): creates[c["mint"]] = c
rows = json.load(open(SP + f"/univ/rows_{DAYN}.json"))
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
good = lambda r: bool(r["sid"] and r["delay"] is not None and r["delay"] < 3600 and r["tweet_rank"] == 1)
U = [r for r in R if r["dev"] >= 0.95 and TEMPLATE(r["dev"]) and r["host"] in TOOLS]
NU = collections.Counter((bool(r["he"]), good(r)) for r in U); NS = collections.Counter((bool(x["he"]), x["first"] and x["fresh"]) for x in rows)
for x in rows: x["g"] = x["first"] and x["fresh"]; x["w"] = NU[(bool(x["he"]), x["g"])] / NS[(bool(x["he"]), x["g"])]
# create-time features: name/symbol new in the last W min (all creates), creator's earlier tool coins' first-share
norm = lambda s: "".join(ch for ch in (s or "").lower() if ch.isalnum())
allc = sorted(creates.values(), key=lambda c: (c["slot"], c["idx"] or 0)); last = {}; feat = {}
Dm = {r["mint"]: r for r in R}; hist = collections.defaultdict(list)
for c in allc:
    t = c["bt"]; ks = [k for k in (norm(c["symbol"]), norm(c["name"])) if k]
    gap = min([t - last[k] for k in ks if k in last], default=10**9)
    h = hist[c["creator"]]
    feat[c["mint"]] = dict(gap=gap, cn=len(h), cf=(sum(h) / len(h)) if h else None)
    for k in ks: last[k] = t
    r = Dm.get(c["mint"])
    if r and r["dev"] >= 0.95 and r["host"] in TOOLS: hist[c["creator"]].append(good(r))
for x in rows: x.update(feat.get(x["mint"], {}))
cap = lambda x: x["pb_sol"] is None or x["pb_sol"] <= 40
RULES = {
  "T  tweet check: tweet-first & fresh": lambda x: x["g"],
  "N2 name/symbol new in last 2 min": lambda x: x["gap"] > 120,
  "N10 name/symbol new in last 10 min": lambda x: x["gap"] > 600,
  "N2 + creator's earlier first-share >= 0.5 or new": lambda x: x["gap"] > 120 and (x["cf"] is None or x["cf"] >= 0.5),
  "N2 and T": lambda x: x["gap"] > 120 and x["g"],
  "N2 but not T": lambda x: x["gap"] > 120 and not x["g"],
  "not N2": lambda x: x["gap"] <= 120,
  "all template+tool": lambda x: True,
}
def stats(G, cost):
    G = [(w, v - cost) for w, v in G]
    if len(G) < 5: return None
    sw = sum(w for w, v in G); m = sum(w * v for w, v in G) / sw; win = sum(w for w, v in G if v > 0) / sw
    random.seed(5); bs = sorted(sum(w * v for w, v in s) / sum(w for w, v in s) for s in ([random.choice(G) for _ in G] for _ in range(400)))
    return m, bs[10], bs[389], win, len(G), sw
fmt = lambda s: f"{100*s[0]:+6.2f}% [{100*s[1]:+6.2f},{100*s[2]:+6.2f}] win {100*s[3]:3.0f}% n {s[4]:3d}" if s else "n<5".ljust(44)
print(f"Oct {DAYN}, hold {H} slots, post-bundle <= 40 SOL, net of our cost 1.77%")
print(f"   {'rule':50s} | {'TRAIN h0-7':44s} | {'TEST h8-11':44s} | buys/h")
for name, f in RULES.items():
    o = []
    for sel in (lambda x: x["hour"] < 8, lambda x: x["hour"] >= 8):
        o.append(fmt(stats([(x["w"], x[f"v{H}"]) for x in rows if sel(x) and cap(x) and f(x) and x.get(f"v{H}") is not None], 0.0177)))
    nb = sum(x["w"] for x in rows if cap(x) and f(x) and x["d"] is not None) / 12
    print(f"   {name:50s} | {o[0]} | {o[1]} | {nb:5.1f}")
