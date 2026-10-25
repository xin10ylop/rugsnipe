# The value of each coin for a buyer right behind the bundle (price after the last bundle buy -> the last price of
# create + k, k = 1, 3, 6, 10), from its tape. -> cell/val.json
import json, os
N = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/dec/night"
R = json.load(open("rows.json")); ops = json.load(open(N + "/ops.json"))
out = {}
for r in R:
    p = f"{N}/coins/{r['mint']}.json"
    if not os.path.exists(p): continue
    t = json.load(open(p)); cs = t["cs"]; ws = set(ops[r["op"]][r["mint"]]["wallets"])
    ev = sorted(((row[0], row[1], e) for row in t["rows"] for e in row[3] if e[4] > 0 and e[5] > 0), key=lambda x: (x[0], x[1]))
    if not ev: continue
    b = [x for x in ev if x[2][0] in ws and x[2][1]]
    if not b: continue
    lb = max(b, key=lambda x: (x[0], x[1]))
    p0 = lb[2][4] / lb[2][5]
    v = {}
    for k in (1, 3, 6, 10):
        last = [x for x in ev if x[0] <= cs + k]
        pk = last[-1][2][4] / last[-1][2][5]
        v[k] = round(pk / p0 - 1, 4)
    out[r["mint"]] = v
json.dump(out, open("val.json", "w")); print(len(out))
