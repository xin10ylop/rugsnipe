# Ex ante, Oct 23 12-24 + Oct 24 00-12: the follow rule keyed on the bundle wallet alone (live now) against the rule
# keyed on (bundle wallet, the coin's fee routing: the dev keeps the creator fees or a third wallet gets them).
import os, json, collections
__file__ = os.path.abspath("final.py")
src = open("final.py").read().rsplit('if __name__ == "__main__":', 1)[0]
exec(compile(src, "final5", "exec"))
def devfees(m):
    c = cr[m]; return c["creator"] == c["signer"] or c["creator"] == c["user"]
def run_key(K, R, keyed):
    n = collections.Counter(); h = collections.Counter(); out = {}
    for s, i, m in coins:
        r = D[m]; cl = devfees(m) if keyed else None
        ks = [(w, cl) for w in BW[m]["bw"]]
        out[m] = any(h[k] >= K and h[k] / n[k] >= R for k in ks)
        for k in ks: n[k] += 1; h[k] += r["he"]
    return out
for K, R in ((2, 0.5), (2, 0.3)):
    print(f"\n== K >= {K}, R >= {R}")
    for keyed in (False, True):
        fol = run_key(K, R, keyed)
        sel = collections.Counter(); hit = collections.Counter(); tot = collections.Counter(); V = collections.defaultdict(list)
        for m, f in fol.items():
            r = D[m]
            if r["per"] == "23a" or cls(r) == "low" or not BW[m]["bw"]: continue
            tot[r["per"]] += r["he"]
            if f:
                sel[r["per"]] += 1; hit[r["per"]] += r["he"]
                if (BW[m]["pb"] or 99) <= 40:
                    v = pol(m)
                    if v is not None: V[r["per"]].append(v); V["test"].append(v)
        lab = "(wallet, fee routing)" if keyed else "wallet only (live)  "
        print(f"   {lab} " + " | ".join(f"{p}: followed {sel[p]:4d} his rate {100*hit[p]/max(1,sel[p]):3.0f}% cover {100*hit[p]/max(1,tot[p]):3.0f}%" for p in ("23b", "24a")) +
              f" | value (pb<=40) {BT.fmt(BT.stats(V['test'], 0.012))} sum {100*sum(x - 0.012 for x in V['test']):+.0f}")
