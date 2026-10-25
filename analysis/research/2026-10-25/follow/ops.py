# Operators as bundle-wallet clusters (wallets that share a bundle are one operator; union-find, hub wallets excluded).
import json, collections, glob
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
B = json.load(open(SP + "/dec/bw.json"))
import os
for p in glob.glob(SP + "/dec/bwr/*.json"):
    m = os.path.basename(p)[:-5]
    if m not in B:
        try: B[m] = dict(src="bwr", bw=None, path=p)
        except Exception: pass
D = {}
for day in (23, 24):
    for r in json.load(open(SP + f"/pm/dataset_{day}.json")): r["day"] = day; D[r["mint"]] = r
P = {}
def find(x):
    while P.setdefault(x, x) != x: P[x] = P[P[x]]; x = P[x]
    return x
fan = collections.Counter(w for x in B.values() if x["bw"] for w in set(x["bw"]))
for m, x in B.items():
    ws = [w for w in set(x["bw"] or []) if fan[w] <= 400]
    for w in ws[1:]: P[find(ws[0])] = find(w)
op = {}
for m, x in B.items():
    ws = [w for w in set(x["bw"] or []) if fan[w] <= 400]
    if ws: op[m] = find(ws[0])
S = collections.defaultdict(lambda: collections.Counter())
for m, o in op.items():
    r = D.get(m)
    if not r: continue
    k = "rule" if r["host"] in TOOLS and TEMPLATE(r["dev"]) else "out"
    S[o][k] += 1; S[o][k + "_he"] += r["he"]
big = sorted(S.items(), key=lambda kv: -(kv[1]["rule_he"] + kv[1]["out_he"]))
print("operators", len(S), "coins with a bundle", len(op))
print(f"{'operator':10s} {'rule coins':>10s} {'his':>5s} {'out coins':>10s} {'his':>5s}")
for o, c in big[:30]:
    print(f"{o[:8]:10s} {c['rule']:10d} {100*c['rule_he']/max(1,c['rule']):4.0f}% {c['out']:10d} {100*c['out_he']/max(1,c['out']):4.0f}%")
