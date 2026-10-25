# "Enter only auctions we can win fairly": the signer's clearing bids on its previous coins (known from the chain before
# the next create) against our ~4M bid. Where the signer's coins usually clear below our bid we win them whatever their
# value, so our wins should not be the rejects; where they usually clear above it we only win the rejects.
import json, collections, statistics as st
AU = json.load(open("big/auction.json")); AU.update(json.load(open("big/auction40.json")))
R = json.load(open("an_rows.json"))
ms = sorted(AU, key=lambda m: R[m]["cs"]); hist = collections.defaultdict(list); out = []
for m in ms:
    x = AU[m]; s = R[m]["signer"]; h = hist[s]
    med = st.median(h[-8:]) if len(h) >= 3 else None
    out.append(dict(m=m, med=med, n=len(h), **x))
    hist[s].append(x["win_bid"])
def mb(v): return "none (<3 coins)" if v is None else "a <2M" if v < 2 else "b 2-4M" if v < 4 else "c 4-8M" if v < 8 else "d 8-20M" if v < 20 else "e >=20M"
for lab, f in (("<=40", lambda x: x["pb"] <= 40), (">40", lambda x: x["pb"] > 40)):
    g = collections.defaultdict(list)
    for x in out:
        if f(x): g[mb(x["med"])].append(x)
    print(f"\n== {lab}: the signer's median clearing bid over its last 8 coins (before this one)")
    print(f"{'signer clears at':16} {'coins':>5} {'v10 all':>8} {'med':>6} {'we won':>6} {'v10 of our wins':>15} {'our live':>8} {'per win':>8} {'he won':>6} {'v10 his':>7}")
    for k in sorted(g):
        xs = g[k]; w = [x for x in xs if x["win"] == "US"]; hw = [x for x in xs if x["win"] == "HIM"]
        lw = [x for x in w if x["us_ok"]]
        print(f"{k:16} {len(xs):5d} {100*st.mean(x['v10'] for x in xs):+7.1f}% {100*st.median(x['v10'] for x in xs):+5.1f}% {len(w):6d} "
              f"{100*st.mean(x['v10'] for x in w) if w else 0:+14.1f}% {sum(x['us_net'] for x in lw):+8.3f} {(sum(x['us_net'] for x in lw)/len(lw)) if lw else 0:+8.4f} {len(hw):6d} {100*st.mean(x['v10'] for x in hw) if hw else 0:+6.1f}%")
