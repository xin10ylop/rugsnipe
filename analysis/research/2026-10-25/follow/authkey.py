# Ex ante, Oct 23 12-24 + Oct 24 00-12: the follow rule keyed on (bundle wallet, fee routing) as built, on the tweet
# author alone, and on either. Precision = his rate on followed coins; cover = share of his picks followed.
import os, json, collections
__file__ = os.path.abspath("final.py")
src = open("final.py").read().rsplit('if __name__ == "__main__":', 1)[0]
exec(compile(src, "final5", "exec"))
def devfees(m):
    c = cr[m]; return c["creator"] == c["signer"] or c["creator"] == c["user"]
def keys_bw(m): return [("w", w, devfees(m)) for w in BW[m]["bw"]]
def keys_au(m):
    a = (D[m].get("author") or "").lower()
    return [("a", a)] if a and D[m].get("sid") else []
def keys_auf(m):
    a = (D[m].get("author") or "").lower()
    return [("a", a, devfees(m))] if a and D[m].get("sid") else []
def run_keys(K, R, kf):
    n = collections.Counter(); h = collections.Counter(); out = {}
    for s, i, m in coins:
        ks = kf(m)
        out[m] = any(h[k] >= K and h[k] / n[k] >= R for k in ks)
        for k in ks: n[k] += 1; h[k] += D[m]["he"]
    return out
SETS = {"(wallet, fees) [built]": keys_bw, "author": keys_au, "(author, fees)": keys_auf,
        "(wallet, fees) or author": lambda m: keys_bw(m) + keys_au(m),
        "(wallet, fees) or (author, fees)": lambda m: keys_bw(m) + keys_auf(m)}
for K, R in ((2, 0.5), (2, 0.3)):
    print(f"\n== K >= {K}, R >= {R}")
    for lab, kf in SETS.items():
        fol = run_keys(K, R, kf)
        sel = collections.Counter(); hit = collections.Counter(); tot = collections.Counter()
        for m, f in fol.items():
            r = D[m]
            if r["per"] == "23a" or cls(r) == "low": continue
            g = (r["per"], cls(r)); tot[g] += r["he"]
            if f: sel[g] += 1; hit[g] += r["he"]
        print(f"   {lab:34s} " + " | ".join(f"{p}/{c}: n {sel[(p,c)]:4d} prec {100*hit[(p,c)]/max(1,sel[(p,c)]):3.0f}% cover {100*hit[(p,c)]/max(1,tot[(p,c)]):3.0f}%" for p in ("23b", "24a") for c in ("rule", "out")))
