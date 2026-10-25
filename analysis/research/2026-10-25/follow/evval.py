# Ex ante, Oct 23 12-24 + Oct 24 00-12: following (bundle wallet, fee routing) cells by their recent coins' value for a
# buyer behind the bundle (vC at +10 of the cell's last 3 coins whose +10 slot passed before this create), against the
# built rule on his attempts, and combined. Precision = his rate; value = the live policy, net 1.2%.
import os, json, collections, statistics as stt
__file__ = os.path.abspath("final.py")
src = open("final.py").read().rsplit('if __name__ == "__main__":', 1)[0]
exec(compile(src, "final5", "exec"))
def devfees(m):
    c = cr[m]; return c["creator"] == c["signer"] or c["creator"] == c["user"]
VC = {}
def v10(m):
    if m in VC: return VC[m]
    v = None
    for d in ("full", "fp"):
        p = f"{SP}/{d}/{m}.json"
        if os.path.exists(p):
            x = BT.coin(cr[m], json.load(open(p))); v = (x.get("vC") or {}).get(10); break
    VC[m] = v; return v
def run_rules(K=2, R=0.3, T=0.10, NV=3):
    n = collections.Counter(); h = collections.Counter(); vals = collections.defaultdict(list); pend = []
    out = {}
    for s, i, m in coins:
        while pend and pend[0][0] + 10 < s:
            _, ks, v = pend.pop(0)
            for k in ks: vals[k].append(v)
        ks = [(w, devfees(m)) for w in BW[m]["bw"]]
        att = any(h[k] >= K and h[k] / n[k] >= R for k in ks)
        vv = [x for k in ks for x in vals[k][-NV:]]
        val = len(vv) >= 2 and stt.mean(vv) >= T
        out[m] = (att, val)
        for k in ks: n[k] += 1; h[k] += D[m]["he"]
        v = v10(m)
        if v is not None and ks: pend.append((s, ks, v)); pend.sort()
    return out
for T in (0.05, 0.10, 0.20):
    fol = run_rules(T=T)
    print(f"\n== value threshold {T:+.2f} (last 3 coins at +10), attempts K 2 R 0.3")
    for lab, sel in (("attempts (built)", lambda a, v: a), ("value", lambda a, v: v), ("both", lambda a, v: a and v), ("either", lambda a, v: a or v), ("value, not attempts", lambda a, v: v and not a), ("attempts, not value", lambda a, v: a and not v)):
        nn = hh = 0; V_ = []; tot = 0
        for m, (a, v) in fol.items():
            r = D[m]
            if r["per"] == "23a" or cls(r) == "low": continue
            tot += r["he"]
            if not sel(a, v): continue
            nn += 1; hh += r["he"]
            if (BW[m]["pb"] or 99) <= 40:
                x = pol(m)
                if x is not None: V_.append(x)
        print(f"   {lab:22s} coins {nn:5d} his rate {100*hh/max(1,nn):4.0f}% cover {100*hh/max(1,tot):4.0f}% | value {BT.fmt(BT.stats(V_, 0.012))} n {len(V_)} sum {100*sum(x - 0.012 for x in V_):+.0f}")
print("\nhis stake on the coins he bought, by the cell's recent value (T +0.10 rule state):")
ST = {t["mint"]: t["stake"] for t in M}
fol = run_rules(T=0.10)
G = collections.defaultdict(list)
for m, (a, v) in fol.items():
    if m in ST and D[m]["per"] != "23a":
        G[("followed" if a else "not followed", "recent value >= +10%" if v else "recent value below")].append(ST[m])
for k, xs in sorted(G.items()): print(f"   {k[0]:13s} {k[1]:22s} trades {len(xs):4d} stake mean {stt.mean(xs):.3f} median {stt.median(xs):.3f}")
