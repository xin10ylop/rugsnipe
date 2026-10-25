import json, collections
exec(open("react.py").read().split("def show(")[0])
for r in rows:
    b = his_buy[r["m"]]; r["sol"] = b["fill"][0] / 1e9
def sb(s): return "a <0.7" if s < 0.7 else "b 0.7-1.0" if s < 1.0 else "c 1.0-1.5" if s < 1.5 else "d >=1.5"
g = collections.defaultdict(list); gt = collections.defaultdict(list)
for r in rows:
    if not r["es"] or not r["ex_h"]: continue
    v = pnl(r["es"], r["ex_h"])
    if v is None: continue
    g[(sb(r["sol"]), r["big"])].append(v); g[(sb(r["sol"]), "all")].append(v)
    gt[(sb(r["sol"]), r["tip"] >= 4)].append(r["tip"])
print("== copy his landed buys by HIS BUY SIZE (what the trade itself shows), next slot, sell one slot before his sell")
for k in sorted(g, key=str): summ(f"his buy {k[0]:10} {'pb > 40' if k[1] is True else 'pb <= 40' if k[1] is False else 'all'}", g[k])
print("\n   share of his tip >= 4M buys by size:", {k: f"{sum(1 for r in rows if sb(r['sol']) == k and r['tip'] >= 4)}/{sum(1 for r in rows if sb(r['sol']) == k)}" for k in ("a <0.7", "b 0.7-1.0", "c 1.0-1.5", "d >=1.5")})
