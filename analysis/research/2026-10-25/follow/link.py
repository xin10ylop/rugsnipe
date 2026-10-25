# Ex-ante identity links: a coin is linked when one of its wallets (creator, signer, bundle wallets, funder) was a
# wallet of an EARLIER coin he attempted (rule coins included). Which link explains his outside picks, at what precision?
import json, collections, glob, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
B = json.load(open(SP + "/dec/bw.json")); F = json.load(open(SP + "/dec/funders.json")); U = json.load(open(SP + "/fp/_universe.json"))
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
D = {}
for day in (23, 24):
    for r in json.load(open(SP + f"/pm/dataset_{day}.json")):
        r["day"] = day; r["per"] = "23a" if day == 23 and r["hour"] < 12 else "23b" if day == 23 else ("24a" if r["hour"] < 12 else "24b"); D[r["mint"]] = r
ctrl = set(U["ctrl"]); pick = set(U["pick"])
fan = collections.Counter(F[m]["f"][0][0] for m in F if F[m]["f"])
bwfan = collections.Counter(w for m in B for w in set(B[m]["bw"]))
def wallets(m):
    c = cr[m]; w = {("c", c["creator"]), ("c", c["signer"])}
    for x in set(B.get(m, {}).get("bw", [])):
        if bwfan[x] <= 200: w.add(("b", x))
    if m in F and F[m]["f"] and fan[F[m]["f"][0][0]] <= 40: w.add(("f", F[m]["f"][0][0]))
    return w
co = sorted((cr[m]["slot"], m) for m in set(B) | set(F) if m in D and m in cr)
seen = collections.defaultdict(set)   # wallet -> kinds of role it had on his earlier picks
res = collections.defaultdict(collections.Counter)
for s, m in co:
    r = D[m]; W = wallets(m)
    outside = not (r["host"] in TOOLS and TEMPLATE(r["dev"]))
    if outside and r["dev"] >= 0.25 and (m in B or m in F):
        role = set()
        for k, w in W:
            if (k, w) in seen or any((kk, w) in seen for kk in "cbf"): role.add(k)
        grp = "ctrl" if m in ctrl else ("pick" if m in pick else "other")
        lab = "+".join(sorted(role)) or "none"
        res[(r["per"], grp, lab)]["n"] += 1; res[(r["per"], grp, lab)]["he"] += r["he"]
    if r["he"]:
        for k, w in W: seen[(k, w)].add(m)
for per in ("23a", "23b", "24a"):
    print(per)
    labs = sorted({k[2] for k in res if k[0] == per})
    for lab in labs:
        p = res[(per, "pick", lab)]; c = res[(per, "ctrl", lab)]
        print(f"   link {lab:8s} picks-universe n {p['n']:4d} his {p['he']:4d} ({100*p['he']/max(1,p['n']):3.0f}%) | random control n {c['n']:3d} his {c['he']}")
print("tapes", len(B), "funders", len(F))

# ---- value for us: the first outside buyer's return at create + H, net 1.2% (tips, fees), by link and period
sys.path.insert(0, SP + "/pm"); import bt as BT
V = collections.defaultdict(list)
seen2 = collections.defaultdict(set)
for s, m in co:
    r = D[m]; W = wallets(m)
    outside = not (r["host"] in TOOLS and TEMPLATE(r["dev"]))
    x = B.get(m)
    if outside and r["dev"] >= 0.25 and x and x["vC"]:
        role = {k for k, w in W if any((kk, w) in seen2 for kk in "cbf")}
        lab = "linked b" if "b" in role else ("linked c/f" if role else "none")
        for H in (2, 3, 4, 6):
            v = x["vC"].get(str(H)) if isinstance(next(iter(x["vC"])), str) else x["vC"].get(H)
            if v is not None:
                V[(lab, H, r["per"])].append(v); V[(lab, H, "all")].append(v)
                if r["he"]: V[(lab + " & his", H, "all")].append(v)
    if r["he"]:
        for k, w in W: seen2[(k, w)].add(m)
print("\nfirst outside buyer, net 1.2%:")
for lab in ("linked b", "linked b & his", "linked c/f", "linked c/f & his", "none", "none & his"):
    for H in (2, 3, 4, 6):
        print(f"   {lab:18s} H{H}: " + " | ".join(f"{p} {BT.fmt(BT.stats(V[(lab, H, p)], 0.012))}" for p in ("23a", "23b", "24a", "all") if (lab, H, p) in V))
