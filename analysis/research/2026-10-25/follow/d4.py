# Richer tool-settings fingerprint from the create tx itself (host, dev buy, ix, tip, compute-unit price).
import json, collections, glob
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
rows = []
for day in (23, 24):
    for r in json.load(open(SP + f"/pm/dataset_{day}.json")):
        r["day"] = day
        r["per"] = "23a" if day == 23 and r["hour"] < 12 else "23b" if day == 23 else ("24a" if r["hour"] < 12 else "24b")
        if r["per"] == "24b" or (r["host"] in TOOLS and TEMPLATE(r["dev"])) or r["mint"] not in cr: continue
        c = cr[r["mint"]]; r["tipx"] = c["tip"]; r["cup"] = c.get("cup"); r["sgn"] = c["signer"] != c["creator"]
        rows.append(r)
P = {p: [r for r in rows if r["per"] == p] for p in ("23a", "23b", "24a")}
KEYS = {
    "host+dev": lambda r: (r["host"], round(r["dev"], 3)),
    "host+dev+tip+cup": lambda r: (r["host"], round(r["dev"], 3), r["tipx"], r["cup"]),
    "host+dev+ix+tip+cup+sgn": lambda r: (r["host"], round(r["dev"], 3), r["ix"], r["tipx"], r["cup"], r["sgn"]),
    "dev+tip+cup": lambda r: (round(r["dev"], 3), r["tipx"], r["cup"]),
}
def table(g, key, nmin, rmin):
    st = collections.defaultdict(lambda: [0, 0])
    for r in g: k = key(r); st[k][0] += 1; st[k][1] += r["he"]
    return {k for k, (n, h) in st.items() if n >= nmin and h / n >= rmin}
def score(g, sel):
    s = [r for r in g if sel(r)]; he = sum(r["he"] for r in g); hit = sum(r["he"] for r in s)
    return f"sel {len(s):4d} his {hit:3d} prec {100*hit/max(1,len(s)):5.1f}% rec {100*hit/max(1,he):5.1f}%"
for name, key in KEYS.items():
    for nmin, rmin in ((2, 0.5), (3, 0.3), (3, 0.5)):
        T = table(P["23a"], key, nmin, rmin); T2 = table(P["23a"] + P["23b"], key, nmin, rmin)
        print(f"{name:24s} n>={nmin} r>={rmin} | 23b(fit 23a) {score(P['23b'], lambda r: key(r) in T)} | 24a(fit 23) {score(P['24a'], lambda r: key(r) in T2)}")
# how unique is the settings key: his picks whose key was never seen before (in all prior data)
seen = set(); new = collections.Counter()
for r in sorted(rows, key=lambda r: r["bt"]):
    k = KEYS["host+dev+tip+cup"](r)
    if r["he"]: new[k in seen] += 1
    seen.add(k)
print("his picks whose settings key was seen before (any coin):", dict(new))
