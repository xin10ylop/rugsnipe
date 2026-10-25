# Which create-time signals explain his picks outside the rule? Fit on 23a, test on 23b and 24a.
import json, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
rows = []
for day in (23, 24):
    for r in json.load(open(SP + f"/pm/dataset_{day}.json")):
        r["day"] = day
        r["per"] = "23a" if day == 23 and r["hour"] < 12 else "23b" if day == 23 else ("24a" if r["hour"] < 12 else "24b")
        if r["per"] == "24b": continue
        if r["host"] in TOOLS and TEMPLATE(r["dev"]): continue     # the rule's own universe
        rows.append(r)
hc = lambda r: "ipfs.io" if r["host"] == "ipfs.io" else ("tool" if r["host"] in TOOLS else r["host"] if r["host"] in ("pump.mypinata.cloud", "gg.sc", "fast-ipfs.com", "ipfs.launchblitz.ai") else "other")
KEYS = {
    "host class + dev 3dp": lambda r: (hc(r), round(r["dev"], 3)),
    "host + dev 3dp": lambda r: (r["host"], round(r["dev"], 3)),
    "host + dev 3dp + ix": lambda r: (r["host"], round(r["dev"], 3), r["ix"]),
    "host + dev 3dp + ix + tip>0": lambda r: (r["host"], round(r["dev"], 3), r["ix"], bool(r["tip"])),
    "dev 3dp": lambda r: round(r["dev"], 3),
}
P = {p: [r for r in rows if r["per"] == p] for p in ("23a", "23b", "24a")}
for p, g in P.items(): print(p, "coins outside the rule", len(g), "his", sum(r["he"] for r in g))
def table(g, key, nmin, rmin):
    st = collections.defaultdict(lambda: [0, 0])
    for r in g:
        k = key(r); st[k][0] += 1; st[k][1] += r["he"]
    return {k for k, (n, h) in st.items() if n >= nmin and h / n >= rmin}
def score(g, sel):
    s = [r for r in g if sel(r)]; he = sum(r["he"] for r in g)
    hit = sum(r["he"] for r in s)
    return f"sel {len(s):5d} his {hit:4d} prec {100*hit/max(1,len(s)):5.1f}% recall {100*hit/max(1,he):5.1f}%"
print("\n== fingerprint tables fit on 23a, scored on 23b / 24a (and refit on 23 all -> 24a)")
for name, key in KEYS.items():
    for nmin, rmin in ((3, 0.3), (5, 0.2), (5, 0.5)):
        T = table(P["23a"], key, nmin, rmin)
        T2 = table(P["23a"] + P["23b"], key, nmin, rmin)
        print(f"{name:28s} n>={nmin} rate>={rmin:.1f} keys {len(T):3d} | 23a(in) {score(P['23a'], lambda r: key(r) in T)} | 23b {score(P['23b'], lambda r: key(r) in T)} | 24a(fit 23) {score(P['24a'], lambda r: key(r) in T2)}")
print("\n== deployer history (known at create: his month trades on this creator before, the day's earlier picks)")
for lab, f in (("his month trades on the creator >= 1", lambda r: r["month_n"] >= 1),
               ("creator_prior_he >= 1 (today)", lambda r: r["creator_prior_he"] >= 1),
               ("either", lambda r: r["month_n"] >= 1 or r["creator_prior_he"] >= 1),
               ("creator_prior >= 1 (deployed today before)", lambda r: r["creator_prior"] >= 1)):
    print(f"{lab:45s}", " | ".join(f"{p} {score(g, f)}" for p, g in P.items()))
