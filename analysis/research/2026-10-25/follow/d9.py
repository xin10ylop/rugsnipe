import json, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
M = sorted(json.load(open(SP + "/month/AHwJ_T.json")), key=lambda t: t["bt"])
byc = collections.defaultdict(list)
for t in M: byc[t["creator"]].append(t)
rows = []
for day in (23, 24):
    for r in json.load(open(SP + f"/pm/dataset_{day}.json")):
        r["day"] = day; r["per"] = "23a" if day == 23 and r["hour"] < 12 else "23b" if day == 23 else ("24a" if r["hour"] < 12 else "24b")
        if r["per"] == "24b" or (r["host"] in TOOLS and TEMPLATE(r["dev"])) or r["dev"] < 0.25: continue
        r["n7"] = sum(1 for t in byc[r["creator"]] if r["bt"] - 7 * 86400 <= t["bt"] < r["bt"])
        rows.append(r)
he = sum(r["he"] for r in rows); print("outside, dev>=0.25:", len(rows), "his", he)
c = collections.defaultdict(lambda: [0, 0])
for r in rows: k = min(r["n7"], 10); k = k if k < 5 else (">=5" if k < 10 else ">=10"); c[k][0] += 1; c[k][1] += r["he"]
for k, (n, h) in sorted(c.items(), key=lambda kv: str(kv[0])): print(f"   his trades on the creator, last 7 days {str(k):5s} n {n:6d} his {h:4d} {100*h/n:5.1f}%  share of his {100*h/he:4.1f}%")
