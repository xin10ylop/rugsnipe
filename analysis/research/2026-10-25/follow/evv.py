# Value for us of the followed coins (first outside buyer, net 1.2%), vs the current rule, per period.
import json, glob, os, sys, collections, bisect
sys.argv = sys.argv[:1]
exec(open(os.path.dirname(os.path.abspath(__file__)) + "/ev.py").read().split('if __name__ == "__main__":')[0])
sys.path.insert(0, SP + "/pm"); import bt as BT
M = [t for t in json.load(open(SP + "/month/AHwJ_T.json")) if t["stake"] >= 0.1]
DC = collections.defaultdict(list); DS = collections.defaultdict(list)
for t in M:
    DC[t["creator"]].append(t["bt"])
    if t["mint"] in cr: DS[cr[t["mint"]]["signer"]].append(t["bt"])
for d in (DC, DS):
    for v in d.values(): v.sort()
n_in = lambda d, k, lo, hi: bisect.bisect_left(d.get(k, []), hi) - bisect.bisect_left(d.get(k, []), lo)
def ourrule(m):
    r = D[m]; c = cr[m]
    if not (r["host"] in TOOLS and TEMPLATE(r["dev"])): return False
    if (BW[m]["pb"] or 99) > 40: return False
    return n_in(DC, c["creator"], c["bt"] - 7 * 86400, c["bt"]) + n_in(DS, c["signer"], c["bt"] - 7 * 86400, c["bt"]) > 0
VAL = {}
def value(m):
    if m in VAL: return VAL[m]
    p = None
    for d in ("full", "fp"):
        if os.path.exists(f"{SP}/{d}/{m}.json"): p = f"{SP}/{d}/{m}.json"; break
    v = None
    if p:
        x = BT.coin(cr[m], json.load(open(p)))
        if x.get("vC"): v = x["vC"]
    VAL[m] = v; return v
K, R = 2, 0.5
fol = dict(run(K, R))
G = collections.defaultdict(list); NV = collections.Counter()
for m, f in fol.items():
    r = D[m]; c = cls(r)
    if c == "low": continue
    sets = []
    if ourrule(m): sets.append("A our rule")
    if f and c == "rule": sets.append("B followed, rule host+template")
    if f and c == "rule" and not ourrule(m): sets.append("B' followed, rule host, NOT in our rule")
    if f and c == "out": sets.append("C followed, outside")
    if ourrule(m) and not f: sets.append("A' our rule, NOT followed")
    for s in sets:
        v = value(m)
        if v is None: NV[(s, r["per"])] += 1; continue
        first = bool(r["sid"] and r["delay"] is not None and r["delay"] < 3600 and r["tweet_rank"] == 1)
        pol = v.get(3) if (r["host"] in TOOLS and not first) else v.get(10)
        for H, x in ((3, v.get(3)), (10, v.get(10)), ("policy", pol)):
            if x is not None: G[(s, H, r["per"])].append(x); G[(s, H, "23b+24a")].append(x) if r["per"] != "23a" else None
for s in sorted({k[0] for k in G}):
    for H in (3, 10, "policy"):
        print(f"{s:40s} {str(H):>6s} " + " | ".join(f"{p} {BT.fmt(BT.stats(G[(s, H, p)], 0.012))}" for p in ("23a", "23b", "24a", "23b+24a")))
    print(f"{'':40s} no tape yet: {dict((p, NV[(s, p)]) for p in ('23a', '23b', '24a') if NV[(s, p)])}")
