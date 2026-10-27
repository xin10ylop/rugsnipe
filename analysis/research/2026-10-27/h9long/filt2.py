# Filters on the tape model (sim2: real premium at our landing): cells of up to two features picked on P1 (without
# its best 2, at least 40 copies), scored on P2. A price bound refuses the copy when our premium is above it (a refused
# buy costs 0.000015). Costs: fee a transaction, bias 0.0155 a fill.   python3 -I filt2.py WALLET STAKE LAND FEE
import sys, json, collections, datetime, itertools
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W, STAKE, LAND, FEE = sys.argv[1], sys.argv[2], sys.argv[3], float(sys.argv[4]); BIAS = 0.0155
R = [r for r in json.load(open(f"{SP}/h9long/{W[:8]}_sim2.json")) if str(r["stake"]) == STAKE and r["where"] == LAND and r["exit"] == "before"]
F = {x["m"]: x for x in json.load(open(f"{SP}/h9long/{W[:8]}_feat.json"))}
# time since its previous buy (any coin) and its size against its last 20 buys
TT = json.load(open(f"{SP}/h9long/{W[:8]}_trips.json"))
buys = sorted((x["tr"][0]["bt"], x["tr"][0]["v"] or 0, x["m"]) for x in TT if x["tr"] and x["tr"][0]["buy"])
gap, rel = {}, {}
for k, (t, v, m) in enumerate(buys):
    gap[m] = t - buys[k - 1][0] if k else 9999
    prev = sorted(b[1] for b in buys[max(0, k - 20):k]) or [v]
    rel[m] = v / max(prev[len(prev) // 2], 0.01)
def per(t): return "P1" if datetime.datetime.fromtimestamp(t, datetime.timezone.utc).day <= 22 else "P2"
def val(r, bound):
    if bound is not None and r["prem"] > bound: return -0.000015
    return r["net_curve"] - 2 * FEE - BIAS
feats = {
    "kind": lambda r: r["kind"][6:],
    "size": lambda r: ("<0.9", "0.9-1.2", "1.2-1.5", ">=1.5")[(r["its_sol"] >= .9) + (r["its_sol"] >= 1.2) + (r["its_sol"] >= 1.5)],
    "stage": lambda r: ("<10M", "10-80M", "80-160M", ">=160M")[(r["sold_before"] >= 10) + (r["sold_before"] >= 80) + (r["sold_before"] >= 160)],
    "route": lambda r: r["ix"],
    "quote": lambda r: r["quote"] if r["quote"] in ("via SOL", "WBTC", "PUMP", "TTWO") else "other",
    "tip": lambda r: "0" if not r["tip"] else "tip",
    "ahead": lambda r: ("0 ahead", "1 ahead", "2+ ahead")[min(2, r["n_ahead"])],
    "gap": lambda r: ("<10 s", "10-60 s", "60-300 s", ">=300 s")[(gap[r["m"]] >= 10) + (gap[r["m"]] >= 60) + (gap[r["m"]] >= 300)],
    "rel": lambda r: ("<0.8x its usual", "0.8-1.2x", "1.2-1.6x", ">=1.6x")[(rel[r["m"]] >= .8) + (rel[r["m"]] >= 1.2) + (rel[r["m"]] >= 1.6)],
}
def score(rs, bound):
    out = {}
    for p in ("P1", "P2"):
        v = sorted((val(r, bound) for r in rs if per(r["bt"]) == p), reverse=True)
        out[p] = (len(v), sum(v), sum(v[2:]))
    return out
print(f"stake {STAKE}, land {LAND}, fee {FEE}, bias {BIAS}; exit a step ahead of its own")
for bound in (None, 0.02, 0.05, 0.10):
    s = score(R, bound)
    print(f"  all, bound {bound}: P1 {s['P1'][0]} {s['P1'][1]:+.2f} ({s['P1'][2]:+.2f})  P2 {s['P2'][0]} {s['P2'][1]:+.2f} ({s['P2'][2]:+.2f})")
cand = []
for bound in (None, 0.05):
    for k in (1, 2):
        for fs in itertools.combinations(feats, k):
            g = collections.defaultdict(list)
            for r in R: g[tuple(feats[f](r) for f in fs)].append(r)
            for key, rs in g.items():
                sc = score(rs, bound)
                if sc["P1"][0] >= 40: cand.append((sc["P1"][2], bound, fs, key, sc))
cand.sort(key=lambda c: -c[0])
print("best cells picked on P1 (without its best 2) -> P2:")
for v, bound, fs, key, sc in cand[:20]:
    print(f"   bound {str(bound):5s} {'/'.join(fs):12s} {str(key):34s} P1 {sc['P1'][0]:4d} {sc['P1'][1]:+7.2f} ({sc['P1'][2]:+7.2f})   P2 {sc['P2'][0]:4d} {sc['P2'][1]:+7.2f} ({sc['P2'][2]:+7.2f})")
