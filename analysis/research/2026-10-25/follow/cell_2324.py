# Oct 23-24: operators (bundle-wallet components), cells (operator, fee routing), his take, and the coin's public
# features, to find what separates his takes from his skips inside a cell. -> cell/c2324.json
import json, glob, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
D = {}
for day in (23, 24):
    for r in json.load(open(SP + f"/pm/dataset_{day}.json")): D[r["mint"]] = r
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
bw = json.load(open(SP + "/dec/bwall.json"))
P = {}
def find(x):
    while P.setdefault(x, x) != x: P[x] = P[P[x]]; x = P[x]
    return x
for m, b in bw.items():
    ws = b["bw"]
    for w in ws[1:]: P[find(w)] = find(ws[0])
rows = []
for m, b in bw.items():
    if not b["bw"] or m not in D or m not in cr: continue
    r = dict(D[m]); c = cr[m]
    r["op"] = find(b["bw"][0]); r["pb"] = b["pb"]; r["nbw"] = len(b["bw"])
    r["fees"] = int(c["creator"] in (c["signer"], c["user"]))
    r["he"] = int(bool(r["he"]))
    rows.append(r)
json.dump(rows, open(SP + "/dec/cell/c2324.json", "w"))
print(len(rows), "coins", sum(r["he"] for r in rows), "taken", len({r["op"] for r in rows}), "operators")
