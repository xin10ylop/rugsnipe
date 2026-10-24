# A copy-skip the bot can run at create time with no tweet feed: skip a rule coin whose name or ticker was already used
# by any pump.fun create in the last W seconds. Measured on the corrected rows: how many it skips, how many of those are
# copies, and the value of the skipped and the kept coins, for all first buyers and where WE were the first buyer.
import json, glob, re, collections, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
allc = []
for f in glob.glob(SP + "/univ/creates_*.json"): allc += json.load(open(f))
allc.sort(key=lambda c: (c["slot"], c["idx"] or 0))
U = {x["mint"]: x for x in json.load(open(SP + "/pm/bt_rows.json"))}
norm = lambda s: re.sub(r"[^a-z0-9]", "", (s or "").lower())
v = lambda x, h: x["vC"].get(str(h))
def m(g, h=10):
    vs = [v(x, h) for x in g if v(x, h) is not None]
    return f"{100*(sum(vs)/len(vs) - 0.012):+6.2f}% n {len(vs):4d}" if len(vs) >= 5 else f"n {len(vs)}"
print(f"creates scanned: {len(allc)}; rule rows: {len(U)}")
for W in (60, 300, 900, 3600):
    seen = {}; rep = {}
    for c in allc:
        keys = [k for k in (("s", norm(c["symbol"])), ("n", norm(c["name"]))) if len(k[1]) >= 2]
        hit = any(k in seen and c["bt"] - seen[k] <= W for k in keys)
        if c["mint"] in U: rep[c["mint"]] = hit
        for k in keys: seen[k] = c["bt"]
    R = [x for x in U.values() if x["mint"] in rep]
    sk = [x for x in R if rep[x["mint"]]]; kp = [x for x in R if not rep[x["mint"]]]
    cp = lambda g: 100 * sum(not x["first_fresh"] for x in g) / max(1, len(g))
    us = lambda g: [x for x in g if x["fo_who"] == "us"]
    allcp = [x for x in R if not x["first_fresh"]]
    print(f"\nwindow {W:4d}s: skips {len(sk):4d} of {len(R)} ({100*len(sk)/len(R):3.0f}%), of them copies {cp(sk):3.0f}% (copies in all: {cp(R):3.0f}%); "
          f"catches {100*sum(not x['first_fresh'] for x in sk)/max(1,len(allcp)):3.0f}% of the copies and {100*sum(x['first_fresh'] for x in sk)/max(1,sum(x['first_fresh'] for x in R)):3.0f}% of the first coins")
    print(f"   skipped: all first buyers H10 {m(sk)} | we first H10 {m(us(sk))}  H3 {m(us(sk),3)}")
    print(f"   kept   : all first buyers H10 {m(kp)} | we first H10 {m(us(kp))}  H3 {m(us(kp),3)}")
