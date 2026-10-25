# His picks among coins with no bundle wallets (Oct 23 12-24 + Oct 24 00-12): which identity did his history know?
import os, json, collections
__file__ = os.path.abspath("final.py")
src = open("final.py").read().rsplit('if __name__ == "__main__":', 1)[0]
exec(compile(src, "final5", "exec"))
cat = json.load(open(SP + "/dec/cell/cat.json"))
# ex ante identity counts, walking the coins in order: creator, signer, author -> (coins, his attempts)
n = collections.Counter(); h = collections.Counter(); know = {}
for s, i, m in coins:
    c = cr[m]; r = D[m]
    ks = [("c", c["creator"]), ("s", c["signer"])] + ([("a", (r.get("author") or "").lower())] if r.get("sid") else [])
    know[m] = {k[0]: (n[k], h[k]) for k in ks}
    for k in ks: n[k] += 1; h[k] += r["he"]
T = collections.defaultdict(lambda: [0, 0])
for m, c in cat.items():
    r = D[m]
    if c != "no bundle wallets" or r["per"] == "23a" or cls(r) == "low": continue
    kn = know[m]
    def fol(t, K=2, R=0.3): return t in kn and kn[t][1] >= K and kn[t][1] >= R * kn[t][0]
    lab = ("creator " if fol("c") else "") + ("signer " if fol("s") else "") + ("author " if fol("a") else "")
    lab = lab.strip() or ("seen, took 1" if any(v[1] >= 1 for v in kn.values()) else "unknown")
    T[(cls(r), lab)][0] += 1; T[(cls(r), lab)][1] += r["he"]
for k, (nn, hh) in sorted(T.items(), key=lambda kv: -kv[1][1]): print(f"   {k[0]:4s} {k[1]:28s} coins {nn:5d} his attempts {hh:4d} rate {100*hh/nn:5.1f}%")
