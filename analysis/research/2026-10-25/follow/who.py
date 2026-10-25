# The bundle wallets that link his picks: how many coins each sits in, his rate on those coins, do they sell early?
import json, collections, glob
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
B = json.load(open(SP + "/dec/bw.json"))
D = {}
for day in (23, 24):
    for r in json.load(open(SP + f"/pm/dataset_{day}.json")): D[r["mint"]] = r
per = collections.defaultdict(list)
for m, x in B.items():
    for w in set(x["bw"]): per[w].append(m)
rows = []
for w, ms in per.items():
    he = sum(D[m]["he"] for m in ms if m in D)
    if he >= 2: rows.append((he, len(ms), w))
rows.sort(reverse=True)
print("bundle wallets on >= 2 of his picks:", len(rows))
for he, n, w in rows[:25]:
    ms = per[w]; hosts = collections.Counter(D[m]["host"][:18] for m in ms if m in D).most_common(2)
    print(f"   {w[:8]} coins {n:4d} his {he:4d} ({100*he/n:3.0f}%) hosts {hosts}")
print("distribution of coins per wallet (wallets on >=2 his picks):", collections.Counter(min(n, 50) // 10 * 10 for he, n, w in rows))
