# Every attempt of his in Oct 23 12-24 + Oct 24 00-12 (the coins with a bundle-slot tape), by what the keyed follow knew.
import os, json, collections
__file__ = os.path.abspath("final.py")
src = open("final.py").read().rsplit('if __name__ == "__main__":', 1)[0]
exec(compile(src, "final5", "exec"))
def devfees(m):
    c = cr[m]; return c["creator"] == c["signer"] or c["creator"] == c["user"]
K, R = 2, 0.3
n = collections.Counter(); h = collections.Counter(); seen_w = collections.Counter()
cat = {}
for s, i, m in coins:
    ks = [(w, devfees(m)) for w in BW[m]["bw"]]
    if not ks: c = "no bundle wallets"
    elif any(h[k] >= K and h[k] / n[k] >= R for k in ks): c = "followed"
    elif any(h[k] > 0 for k in ks): c = "known, he took 1+ (below K/R)"
    elif any(n[k] > 0 for k in ks): c = "known, he never took"
    elif any(seen_w[w] > 0 for w, f in ks): c = "wallet known, other fee routing"
    else: c = "new wallets"
    cat[m] = c
    for k in ks: n[k] += 1; h[k] += D[m]["he"]
    for w, f in ks: seen_w[w] += 1
T = collections.defaultdict(lambda: [0, 0])
for m, c in cat.items():
    r = D[m]
    if r["per"] == "23a" or cls(r) == "low": continue
    T[(cls(r), c)][0] += 1; T[(cls(r), c)][1] += r["he"]
for cl in ("rule", "out"):
    tot = sum(v[1] for k, v in T.items() if k[0] == cl)
    print(f"== {cl} coins: his attempts {tot}")
    for (c2, c), (nn, hh) in sorted(T.items(), key=lambda kv: -kv[1][1]):
        if c2 == cl: print(f"   {c:34s} coins {nn:5d} his attempts {hh:4d} ({100*hh/max(1,tot):4.1f}% of his) rate {100*hh/nn:5.1f}%")
json.dump(cat, open(SP + "/dec/cell/cat.json", "w"))
