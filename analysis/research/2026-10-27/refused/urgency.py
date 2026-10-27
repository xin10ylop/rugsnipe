# Can the ghost tell a keyed buy of his on chain? It cannot see his tip, but it can see his wallet's lamports fall
# (tip + priority fee + his SOL + pump fees + rent). Key on his urgency spend U = tip + priority fee instead of the
# tip: the ghost's value on his buys by U, keyed or not (ghost4.json, P_AFTER 0.8).
import json, collections
exec(open("/home/user/rugsnipe/analysis/research/2026-10-27/refused/eng.py").read())
G = {x["m"]: x for x in json.load(open(SP + "/refused/ghost4.json"))}
def g(x, cap): return x[str(cap)] - BIAS * x[f"{cap}:add"] - x["react"]
SC = 1109 / max(sum(1 for x in G.values() if not x["keyed"]), 1)
rowsU = []
for r in rows:
    x = G.get(r["m"])
    if not x: continue
    b = his_buy[r["m"]]; U = r["tip"] + max(b.get("fee", 5000) - 5000, 0)
    rowsU.append((U, x))
print(f"ghost value at post-bundle + 10 pct (SOL over 5 days; his other buys scaled x{SC:.2f})")
for lo, hi in ((0, 1e6), (1e6, 1.5e6), (1.5e6, 2e6), (2e6, 3e6), (3e6, 5e6), (5e6, 1e9)):
    k = [x for U, x in rowsU if lo <= U < hi and x["keyed"]]; o = [x for U, x in rowsU if lo <= U < hi and not x["keyed"]]
    print(f"   U {lo/1e6:4.1f}-{hi/1e6:5.1f}M  keyed {len(k):4d} ghost {sum(x['0.1'] - BIAS * x['0.1:add'] for x in k):+6.2f} (today {sum(x['react'] for x in k):+.2f})"
          f"   other {len(o):4d} ghost {sum(x['0.1'] - BIAS * x['0.1:add'] for x in o) * SC:+6.2f}")
for thr in (2e6, 3e6, 4e6, 6e6):
    k = [x for U, x in rowsU if U >= thr and x["keyed"]]; o = [x for U, x in rowsU if U >= thr and not x["keyed"]]
    kn = [x for U, x in rowsU if U < thr and x["keyed"]]
    tot = sum(g(x, 0.1) for x in k) + sum(x['0.1'] - BIAS * x['0.1:add'] for x in o) * SC
    print(f"   ghost only when U >= {thr/1e6:.0f}M: keyed covered {len(k)}/{len(k) + len(kn)}; gain over today {tot:+.2f} SOL "
          f"(keyed {sum(g(x, 0.1) for x in k):+.2f}, other buys filled {sum(x['0.1'] - BIAS * x['0.1:add'] for x in o) * SC:+.2f})")
