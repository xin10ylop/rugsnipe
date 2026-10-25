# Which identity explains the outside picks the bundle wallets miss? Identities per coin: bundle wallets (b), the
# creator and the create signer (c). The same ex-ante rule on each (>= K of his attempts at rate >= R, before the coin).
import json, os, sys, collections
exec(open(os.path.dirname(os.path.abspath(__file__)) + "/ev.py").read().split('if __name__ == "__main__":')[0])
ALL = sorted((cr[m]["slot"], cr[m]["idx"] or 0, m) for m in D if m in cr)   # every create, bundled or not, taped or not
def ids(m, kinds):
    out = set()
    if "b" in kinds and m in BW: out |= {("w", w) for w in BW[m]["bw"]}
    if "c" in kinds: out |= {("w", cr[m]["creator"]), ("w", cr[m]["signer"])}
    return out
def run2(K, R, kinds):
    n = collections.Counter(); h = collections.Counter(); res = collections.defaultdict(collections.Counter)
    for s, i, m in ALL:
        r = D[m]; I = ids(m, kinds)
        fol = any(h[k] >= K and h[k] / n[k] >= R for k in I)
        c = cls(r)
        if c != "low":
            res[(r["per"], c, fol)]["n"] += 1; res[(r["per"], c, fol)]["he"] += r["he"]
        for k in I: n[k] += 1; h[k] += r["he"]
    return res
for kinds in ("b", "c", "bc"):
    for K, R in ((1, 0.5), (2, 0.5), (2, 0.3)):
        res = run2(K, R, kinds); line = []
        for per in ("23b", "24a"):
            for c in ("out", "rule"):
                f = res[(per, c, True)]; nf = res[(per, c, False)]; tot = f["he"] + nf["he"]
                line.append(f"{per} {c}: fol {f['n']:4d} his {100*f['he']/max(1,f['n']):3.0f}% cover {100*f['he']/max(1,tot):3.0f}%")
        print(f"ids {kinds:2s} K>={K} R>={R}: " + " | ".join(line))
