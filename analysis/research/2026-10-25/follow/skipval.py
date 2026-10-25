# Inside the followed (wallet, fee routing) cells, Oct 23 12-24 + Oct 24 00-12: the value of the coins he took vs the
# ones he skipped (live exit policy, first outside buyer, net 1.2%), and their features.
import os, json, collections, statistics as stt
__file__ = os.path.abspath("final.py")
src = open("final.py").read().rsplit('if __name__ == "__main__":', 1)[0]
exec(compile(src, "final5", "exec"))
cat = json.load(open(SP + "/dec/cell/cat.json"))
G = collections.defaultdict(list); F = collections.defaultdict(list)
for m, c in cat.items():
    r = D[m]
    if c != "followed" or r["per"] == "23a" or cls(r) == "low": continue
    v = pol(m); g = ("took" if r["he"] else "skipped", cls(r))
    if v is not None: G[g].append(v)
    F[g].append(r)
for g in sorted(G): print(g, BT.fmt(BT.stats(G[g], 0.012)), "n", len(G[g]))
for f in ("dev", "tip", "delay"):
    for g in sorted(F):
        xs = [r[f] for r in F[g] if r.get(f) is not None]
        print(f, g, f"median {stt.median(xs):.3f} mean {stt.mean(xs):.3f}" if xs else "-")
for f in ("host", "ix", "created_on", "community", "tweet_rank"):
    for g in sorted(F):
        c = collections.Counter(r.get(f) for r in F[g]); n = len(F[g])
        print(f, g, {k: f"{100*v/n:.0f}%" for k, v in c.most_common(4)})
print("\nhis take inside followed cells by the linked tweet's age at the create and its rank (first coin on the tweet = 1):")
for cl in ("rule", "out"):
    rows = F[("took", cl)] + F[("skipped", cl)]
    def rt(sel):
        a = [r for r in rows if sel(r)]; return f"{100*sum(1 for r in a if r['he'])/max(1,len(a)):5.1f}% of {len(a):4d}"
    print(f" {cl}: no tweet {rt(lambda r: not r.get('sid'))}")
    for lo, hi in ((0, 3), (3, 6), (6, 10), (10, 20), (20, 40), (40, 120), (120, 1e12)):
        print(f"   age {lo}-{hi}s: rank 1 {rt(lambda r: r.get('sid') and r.get('delay') is not None and lo <= r['delay'] < hi and r['tweet_rank'] == 1)} | rank 2+ {rt(lambda r: r.get('sid') and r.get('delay') is not None and lo <= r['delay'] < hi and r['tweet_rank'] > 1)}")
