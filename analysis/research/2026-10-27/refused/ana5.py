# The 5-day refused landings of him (rec5_dec.json): every mechanism that could fill them, picked on Oct 21-23 (P1)
# and scored on Oct 24-25 (P2). What a mechanism adds over today's refusal (-0.0001 a landing): its fill's value minus
# the 0.0155 model bias, per copy (10 landings a copy, 1/10 each). Also each period without its best 2 coins.
import json, collections, itertools, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
F = json.load(open(SP + "/refused/rec5_dec.json")); R = F["recs"]; W = 1 / F["n"]
BIAS = 0.0155; REF = -0.0001
P1 = ("10-21", "10-22", "10-23"); P2 = ("10-24", "10-25")
def share(x, c): return x["cl"].get(c, 0.0) / x["sol_btw"] if x["sol_btw"] > 0 else 0.0
COMP = {"any": lambda x: True,
        "no sniper SOL": lambda x: share(x, "sniper") == 0, "sniper SOL < 50%": lambda x: share(x, "sniper") < 0.5,
        "reactor SOL >= 50%": lambda x: share(x, "reactor") >= 0.5, "reactor SOL > 0": lambda x: share(x, "reactor") > 0,
        "new wallets >= 50%": lambda x: share(x, "new") >= 0.5,
        "1-2 buys between": lambda x: x["n_btw"] <= 2, "3+ buys between": lambda x: x["n_btw"] >= 3,
        "a sell between": lambda x: x["sells_btw"] > 0, "next slot": lambda x: x["dslot"] >= 1, "his slot": lambda x: x["dslot"] == 0}
EXITS = ["anticipate", "c3", "c5", "c7", "c12", "c15", "c20", "follow"]
def added(xs, key):
    """per coin: what filling these landings adds"""
    by = collections.defaultdict(float)
    for x in xs:
        v = x["v"].get(key)
        if v is None: continue
        by[x["m"]] += W * (v - BIAS - REF)
    return by
def score(by, recs, period, drop=0):
    ms = {x["m"] for x in recs if x["day"] in period}
    g = sorted((v for m, v in by.items() if m in ms), reverse=True)
    return sum(g[drop:])
print(f"{len(R)} refused landings on {len({x['m'] for x in R})} coins; P1 {sum(1 for x in R if x['day'] in P1)}, P2 {sum(1 for x in R if x['day'] in P2)}")
# 1. every premium bucket on its own (the band idea), our position, the bot's exit
print("\n== fill only inside a premium band, sold as today (his sale - 1); SOL added: P1 / P2 / [without each period's best 2]")
for lo, hi in ((0.05, 0.10), (0.10, 0.15), (0.15, 0.20), (0.20, 0.25), (0.25, 0.35), (0.35, 0.45), (0.25, 0.45), (0.45, 1.0), (0.05, 1.0)):
    xs = [x for x in R if lo < x["prem"] <= hi]
    by = added(xs, "anticipate")
    print(f"   {lo:.0%}-{hi:.0%}  landings {len(xs):4d} (~{len(xs) * W:5.1f} fills)  P1 {score(by, xs, P1):+.2f}  P2 {score(by, xs, P2):+.2f}   "
          f"[{score(by, xs, P1, 2):+.2f} / {score(by, xs, P2, 2):+.2f}]")
# 2. the grid: band x composition x exit x size, picked on P1
grid = []
BANDS = [(lo, hi) for lo in (0.05, 0.10, 0.15, 0.20, 0.25, 0.30) for hi in (0.10, 0.15, 0.20, 0.30, 0.45, 0.60, 1.0) if hi > lo]
for (lo, hi), (cn, cf), ex, sz in itertools.product(BANDS, COMP.items(), EXITS, ("", "@0.2")):
    xs = [x for x in R if lo < x["prem"] <= hi and cf(x)]
    if not xs: continue
    by = added(xs, ex + sz)
    grid.append((score(by, xs, P1), score(by, xs, P2), score(by, xs, P1, 2), score(by, xs, P2, 2), lo, hi, cn, ex, sz or "@0.5", len(xs)))
grid.sort(reverse=True)
print(f"\n== {len(grid)} band x who-bought-between x exit x size settings; the 10 best on P1, and what each does on P2")
print("   P1     P2     P1-2   P2-2   band        between              exit        size  landings")
for g in grid[:10]:
    print(f"   {g[0]:+.2f}  {g[1]:+.2f}  {g[2]:+.2f}  {g[3]:+.2f}  {g[4]:.0%}-{g[5]:.0%}  {g[6]:20} {g[7]:11} {g[8]:5} {g[9]}")
pos1 = [g for g in grid if g[2] > 0]
print(f"   settings positive on P1 without its best 2 coins: {len(pos1)}; of those also positive on P2 without its best 2: "
      f"{sum(1 for g in pos1 if g[3] > 0)}")
# 3. re-entry when the price comes back near his
print("\n== re-entry: buy when a slot closes within x of his price, before his sale (bound 5%), sold as today")
for xr in ("re0.0", "re0.03", "re0.05"):
    for lab, cf in (("any", lambda x: True), ("sniper SOL >= 50% between", lambda x: share(x, "sniper") >= 0.5)):
        xs = [x for x in R if cf(x)]
        by = added([x for x in xs if x["v"].get(xr) is not None], xr)
        n = sum(1 for x in xs if x["v"].get(xr) is not None)
        print(f"   x = {xr[2:]:5} {lab:26} re-entries {n:4d} (~{n * W:5.1f})  P1 {score(by, xs, P1):+.2f}  P2 {score(by, xs, P2):+.2f}   "
              f"[{score(by, xs, P1, 2):+.2f} / {score(by, xs, P2, 2):+.2f}]")
