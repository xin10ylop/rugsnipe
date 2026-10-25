# Repeat deployers outside the rule: does his P&L on the deployer, or the gap since its last coin, decide?
import json, collections, glob
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
M = sorted(json.load(open(SP + "/month/AHwJ_T.json")), key=lambda t: t["bt"])
byc = collections.defaultdict(list)
for t in M: byc[t["creator"]].append(t)
rows = []
for day in (23, 24):
    for r in json.load(open(SP + f"/pm/dataset_{day}.json")):
        r["day"] = day
        r["per"] = "23a" if day == 23 and r["hour"] < 12 else "23b" if day == 23 else ("24a" if r["hour"] < 12 else "24b")
        if r["per"] == "24b" or (r["host"] in TOOLS and TEMPLATE(r["dev"])): continue
        rows.append(r)
def tab(lab, g, f):
    c = collections.defaultdict(lambda: [0, 0])
    for r in g: k = f(r); c[k][0] += 1; c[k][1] += r["he"]
    print(lab)
    for k, (n, h) in sorted(c.items(), key=lambda kv: str(kv[0])):
        print(f"   {str(k):34s} n {n:6d} his {h:4d} {100*h/n:5.1f}%")
g = [r for r in rows if r["month_n"] >= 1]
def last(r):
    p = [t for t in byc[r["creator"]] if t["bt"] < r["bt"]]
    return p[-1] if p else None
tab("deployer he traded before: his P&L on the last one", g, lambda r: "win" if last(r)["pnl"] > 0 else "loss")
tab("... hours since his last trade on it", g, lambda r: min(int((r["bt"] - last(r)["bt"]) / 3600), 48) // 6 * 6)
tab("... dev >= 0.25", g, lambda r: r["dev"] >= 0.25)
tab("... same host as his last on it", g, lambda r: "n/a")
gg = [r for r in g if r["dev"] >= 0.25 and r["bt"] - last(r)["bt"] < 6 * 3600]
tab("dev>=0.25, traded <6h ago, by period", gg, lambda r: r["per"])
