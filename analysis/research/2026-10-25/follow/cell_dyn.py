# Night: does his take of an operator's next launch follow his own P&L on its last one, or the demand it drew?
import json, os, collections
N = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/dec/night"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"
L = json.load(open("launch.json")); ops = json.load(open(N + "/ops.json"))
dem = {}
for r in L:
    p = f"{N}/coins/{r['mint']}.json"
    if not os.path.exists(p): continue
    t = json.load(open(p)); cs = t["cs"]; ws = set(ops[r["op"]][r["mint"]]["wallets"])
    dev = t["create"]["user"]
    d = 0; n = 0
    for row in t["rows"]:
        for e in row[3]:
            if e[1] and e[0] not in ws and e[0] != A and e[0] != dev and row[0] <= cs + 10: d += e[2]; n += 1
    dem[r["mint"]] = (d / 1e9, n)
cell = collections.defaultdict(list)
for r in L: cell[(r["op"], r.get("dev_fees"))].append(r)
T = collections.Counter(); TD = collections.defaultdict(lambda: [0, 0])
for k, v in cell.items():
    v.sort(key=lambda r: r["slot"])
    n = len(v); h = sum(r["he"] for r in v)
    if n < 8 or h == 0: continue
    for a, b in zip(v, v[1:]):
        if a["he"]:
            res = "prev win" if (a["pnl"] or 0) > 0 else ("prev loss" if a["pnl"] is not None else "prev failed buy")
        else: res = "prev skipped"
        T[(res, b["he"])] += 1
        da = dem.get(a["mint"], (0, 0))[0]
        g = "hi" if da >= 3 else ("mid" if da >= 1 else "lo")
        TD[(a["he"], g)][0] += 1; TD[(a["he"], g)][1] += b["he"]
for res in ("prev win", "prev loss", "prev failed buy", "prev skipped"):
    y = T[(res, 1)]; n = T[(res, 0)] + y
    print(f"{res:16s} -> takes next {100*y/max(1,n):5.1f}% of {n}")
print("by the previous launch's outside demand (SOL bought by others in 10 slots):")
for (he, g), (n, y) in sorted(TD.items()): print(f"   prev {'taken' if he else 'skipped'} demand {g:3s}: next taken {100*y/n:5.1f}% of {n}")
