# Filters on its own trips with the copy model of sim1 (a flat entry premium e for every copy; exit before its sell,
# its TP / SL / 30-min levels), picked on P1 and scored on P2, without each period's best 2. All costs and the bias.
#   python3 -I filt.py WALLET e stake fee
import sys, json, collections, datetime, itertools
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = sys.argv[1]; E = float(sys.argv[2]); STAKE = sys.argv[3]; FEE = float(sys.argv[4]); BIAS = 0.0155
T = {x["m"]: x for x in json.load(open(f"{SP}/h9long/{W[:8]}_trips.json"))}
F = json.load(open(f"{SP}/h9long/{W[:8]}_feat.json"))
COST = {"other quote": 0.9487, "SOL quote": 0.9713}
def copy(x, f):
    b, s = T[x["m"]]["tr"]
    its = b["v"] or 0
    st = its if STAKE == "its" else float(STAKE)
    if its <= 0.05: return None
    vbb, vab = b["vt"] + b["tok"], b["vt"]; Qi = 1 / vab - 1 / vbb
    va = vab / (1 + E) ** 0.5; qin = Qi * st / its
    tok = va - 1 / (1 / va + qin)
    vx = s["vt"] - s["tok"]; vxu = vx - tok
    if vxu <= 0: return None
    g = (1 / vxu - 1 / vx) / qin
    return st * (g * COST[x["kind"]] - 1) - 2 * FEE - BIAS
for x in F: x["c"] = copy(x, None)
F = [x for x in F if x["c"] is not None]
def score(rs):
    out = {}
    for p in ("P1", "P2"):
        v = sorted((x["c"] for x in rs if x["per"] == p), reverse=True)
        out[p] = (len(v), sum(v), sum(v[2:]))
    return out
feats = {
    "kind": lambda x: x["kind"],
    "size": lambda x: ("<0.9", "0.9-1.2", "1.2-1.5", ">=1.5")[(x["sol"] >= .9) + (x["sol"] >= 1.2) + (x["sol"] >= 1.5)],
    "stage": lambda x: ("<10M", "10-80M", "80-160M", ">=160M")[(x["sold"] >= 10) + (x["sold"] >= 80) + (x["sold"] >= 160)],
    "route": lambda x: x["ix"],
    "quote": lambda x: x["quote"] if x["quote"] in ("SOL", "via SOL", "WBTC", "PUMP", "TTWO") else "other",
    "tip": lambda x: "0" if not x["tip"] else "tip",
}
print(f"entry premium {E:.1%}, stake {STAKE}, fee {FEE} a tx, bias {BIAS}")
s = score(F); print(f"all: P1 {s['P1'][0]} {s['P1'][1]:+.2f} ({s['P1'][2]:+.2f})  P2 {s['P2'][0]} {s['P2'][1]:+.2f} ({s['P2'][2]:+.2f})")
# every cell of up to two features; pick on P1 (w/o best 2 > 0 and n >= 40), report P2
cand = []
for r in (1, 2):
    for fs in itertools.combinations(feats, r):
        g = collections.defaultdict(list)
        for x in F: g[tuple(feats[f](x) for f in fs)].append(x)
        for k, rs in g.items():
            sc = score(rs)
            if sc["P1"][0] >= 40: cand.append((sc["P1"][2], fs, k, sc))
cand.sort(key=lambda c: -c[0])
print("best cells on P1 (without its best 2), and the same cell on P2:")
for v, fs, k, sc in cand[:25]:
    print(f"   {'/'.join(fs):12s} {str(k):40s} P1 {sc['P1'][0]:4d} {sc['P1'][1]:+7.2f} ({sc['P1'][2]:+7.2f})   P2 {sc['P2'][0]:4d} {sc['P2'][1]:+7.2f} ({sc['P2'][2]:+7.2f})")
