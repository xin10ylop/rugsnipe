# Where the wallet's result comes from: each token's P&L (ledger.py) by kind of token, entry stage, size, day and
# 6-hour block, and without each period's 2 best coins.   python3 -I groups.py WALLET
import sys, json, collections, datetime
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = sys.argv[1]
L = json.load(open(f"{SP}/h9long/{W[:8]}_ledger.json"))
rows = [json.loads(l) for l in open(f"{SP}/h9long/{W[:8]}.jsonl")]
PAMM = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
first = {}; venue = collections.defaultdict(set); quote = {}
for r in sorted(rows, key=lambda r: (r["s"], r["i"] or 0)):
    for t in r["tr"]:
        if t[1] != W: continue
        m = t[0]; venue[m].add("curve")
        quote.setdefault(m, "SOL" if t[5] > 0 else "other")
        if t[2] and m not in first: first[m] = dict(s=r["s"], bt=r["bt"], vt=t[6], vs=t[5], sol=t[3], tip=r["tip"], ix=t[11])
    if PAMM in r["progs"]:
        for m in r["tok"]: venue[m].add("pumpswap")
BIG = {"pumpCmXqMfrsAkQ5r49WcJnRayYRqmXz6ae8H7H9Dfn": "PUMP", "3NZ9JMVBmGAqocybic2c7LQCJScmgsAZ6vQqTDzcqmJh": "WBTC",
       "7vfCXTUXx5WJV5JADk17DUJ4ksgau7utNKj4b963voxs": "WETH", "TTWofwAge91oFhZs7kpQdyrVRkmevgM88xijGvQFbKo": "TTWO"}
def kind(m):
    if m in BIG: return "big: " + BIG[m]
    if m.startswith("Xs"): return "big: xStock"
    if "curve" in venue[m]:
        return "pump, SOL quote" if quote.get(m) == "SOL" else "pump, other quote"
    if "pumpswap" in venue[m]: return "pumpswap only"
    return "other token"
def stage(m):
    f = first.get(m)
    if not f: return "no curve buy"
    prog = (1_073_000_000e6 - f["vt"]) / 793_100_000e6
    return "launch (< 5% sold)" if prog < 0.05 else "early (5-30%)" if prog < 0.30 else "late (30%+)"
BZ = datetime.timezone.utc
def blk(t): d = datetime.datetime.fromtimestamp(t, BZ); return f"{d:%m-%d} {d.hour // 6 * 6:02d}h"
def day(t): return datetime.datetime.fromtimestamp(t, BZ).strftime("%m-%d")
P = [dict(p, net=p["real"] + p["open"], kind=kind(p["m"]), stage=stage(p["m"])) for p in L["pos"] if p["first"]]
def table(title, key, xs=P):
    g = collections.defaultdict(list)
    for p in xs: g[key(p)].append(p["net"])
    print(f"\n== {title}")
    for k in sorted(g, key=lambda k: -sum(g[k])):
        v = g[k]; s = sorted(v, reverse=True)
        print(f"   {k:28s} coins {len(v):5d}  net {sum(v):+8.2f}  won {sum(1 for x in v if x > 0):4d}  without best 2 {sum(s[2:]):+8.2f}  best {s[0]:+.2f}")
print(f"total {sum(p['net'] for p in P):+.2f} SOL on {len(P)} tokens (realized {sum(p['real'] for p in P):+.2f}, open {sum(p['open'] for p in P):+.2f}); sold from before the window {sum(p['pre'] for p in L['pos']):+.2f}")
table("by kind", lambda p: p["kind"])
table("pump coins by entry stage", lambda p: p["kind"] + " / " + p["stage"], [p for p in P if p["kind"].startswith("pump")])
table("by day (first buy, UTC)", lambda p: day(p["first"]))
table("by 6-hour block (UTC)", lambda p: blk(p["first"]))
json.dump(P, open(f"{SP}/h9long/{W[:8]}_tokens.json", "w"))
