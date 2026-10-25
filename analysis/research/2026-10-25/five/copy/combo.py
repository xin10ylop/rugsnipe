import json, collections, datetime
exec(open("react.py").read().split("def show(")[0])
SPX = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
rep = {json.loads(l)["mint"]: json.loads(l) for l in open(SPX + "/ext/replay/out.jsonl")}
T0 = int(datetime.datetime(2026, 10, 23, tzinfo=datetime.timezone.utc).timestamp())
for r in rows: r["sol"] = his_buy[r["m"]]["fill"][0] / 1e9; r["bt"] = his_buy[r["m"]]["bt"]
for lab, f in (("his buy >= 0.7 SOL", lambda r: r["sol"] >= 0.7), ("his buy >= 1.0 SOL", lambda r: r["sol"] >= 1.0),
               (">= 0.7 SOL above 40 vSOL, or >= 1.0 SOL", lambda r: (r["sol"] >= 0.7 and r["big"]) or r["sol"] >= 1.0)):
    sel = [r for r in rows if r["bt"] >= T0 and f(r)]
    for where, g in (("not fired by the newest bot", [r for r in sel if not (rep.get(r["m"]) or {}).get("fired")]), ("also fired by it", [r for r in sel if (rep.get(r["m"]) or {}).get("fired")])):
        xs = []; byd = collections.defaultdict(float)
        for r in g:
            if not r["es"] or not r["ex_h"]: continue
            v = pnl(r["es"], r["ex_h"])
            if v is None: continue
            xs.append(v); byd[r["day"]] += v
        summ(f"{lab}, {where}", xs)
        print("        by day: " + "  ".join(f"{k} {v:+.2f}" for k, v in sorted(byd.items())))
