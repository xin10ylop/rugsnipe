# Ex-ante test of "he follows operators by their bundle wallets". Every coin with a tape (full/, fp/, dec/bwr/) in slot
# order; a wallet's record = the coins (before this one) it bought in the bundle of, and how many of those he attempted.
# A coin is FOLLOWED when one of its bundle wallets has >= K of his attempts at a rate >= R. Scored per period.
import json, glob, os, sys, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
D = {}
for day in (23, 24):
    for r in json.load(open(SP + f"/pm/dataset_{day}.json")):
        r["day"] = day; r["per"] = "23a" if day == 23 and r["hour"] < 12 else "23b" if day == 23 else ("24a" if r["hour"] < 12 else "24b")
        if r["per"] != "24b": D[r["mint"]] = r
CACHE = SP + "/dec/bwall.json"
BW = json.load(open(CACHE)) if os.path.exists(CACHE) else {}
for d in ("full", "fp", "dec/bwr"):
    for p in glob.glob(f"{SP}/{d}/*.json"):
        m = os.path.basename(p)[:-5]
        if p.endswith(".tmp") or m.startswith("_") or m not in cr or (m in BW and BW[m]["src"] != "dec/bwr"): continue
        if m in BW and d == "dec/bwr": continue
        try: t = json.load(open(p))
        except Exception: continue
        if not t.get("reached"): continue
        c = cr[m]; cs, ci = c["slot"], c["idx"] or 0
        cset = {c["creator"], c["user"], c["signer"]}; prev = ci; bw = []; pb = None
        seq = [(s, i, u, b, vs) for s, i, sg, evs in t["rows"] for (u, b, sol, tok, vs, vt, f) in evs]
        for s, i, u, b, vs in seq:
            if (s, i) < (cs, ci): continue
            if (s, i) == (cs, ci): pb = vs; continue
            if s == cs and u in cset and (i - prev) <= 2: prev = max(prev, i); pb = vs; continue
            if s == cs and b and u not in (A, US) and (i - prev) <= 2 and (i - ci) <= 5: prev = i; bw.append(u); pb = vs
            elif s > cs or (i - prev) > 2: break
        BW[m] = dict(src=d, bw=sorted(set(bw)), pb=pb / 1e9 if pb else None)
json.dump(BW, open(CACHE, "w"))
coins = sorted((cr[m]["slot"], cr[m]["idx"] or 0, m) for m in BW if m in D)
cls = lambda r: "rule" if r["host"] in TOOLS and TEMPLATE(r["dev"]) else ("out" if r["dev"] >= 0.25 else "low")
print("coins with a bundle-slot tape:", collections.Counter(cls(D[m]) for _, _, m in coins), "| bundled:", sum(1 for _, _, m in coins if BW[m]["bw"]))
print("his attempts covered:", collections.Counter(cls(D[m]) for _, _, m in coins if D[m]["he"]))
def run(K, R, hub=None):
    n = collections.Counter(); h = collections.Counter()
    out = []
    for s, i, m in coins:
        r = D[m]; ws = BW[m]["bw"]
        best = max(((h[w], n[w]) for w in ws), key=lambda x: (x[0] / x[1] if x[1] else 0, x[0]), default=(0, 0))
        fol = any(h[w] >= K and h[w] / n[w] >= R for w in ws)
        out.append((m, fol))
        for w in ws: n[w] += 1; h[w] += r["he"]
    return out
def score(out):
    res = collections.defaultdict(collections.Counter)
    for m, fol in out:
        r = D[m]; k = (r["per"], cls(r), fol); res[k]["n"] += 1; res[k]["he"] += r["he"]
    return res
if __name__ == "__main__":
    for K, R in ((1, 0.5), (2, 0.5), (3, 0.5), (2, 0.3), (3, 0.7), (5, 0.5)):
        res = score(run(K, R))
        line = []
        for per in ("23b", "24a"):
            for c in ("out", "rule"):
                f = res[(per, c, True)]; nf = res[(per, c, False)]
                tot = f["he"] + nf["he"]
                line.append(f"{per} {c}: fol {f['n']:4d} his {100*f['he']/max(1,f['n']):3.0f}% cover {100*f['he']/max(1,tot):3.0f}% (rest {100*nf['he']/max(1,nf['n']):4.1f}%)")
        print(f"K>={K} R>={R}: " + " | ".join(line))
