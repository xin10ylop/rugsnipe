# Can the ghost be aimed at launches where his buy will be keyed (tip >= 2M), from what is known at the create?
# Per launch signer, his keyed share over the 24 h before (no look-ahead); the ghost goes out only where that share is
# high. Value (ghost4.json, post-bundle + 10%): keyed buys there (gain over today), his other buys there (filled by the
# ghost, today none; scaled from the 300 sampled).
import json, bisect, collections
exec(open("/home/user/rugsnipe/analysis/research/2026-10-27/refused/eng.py").read())
G = {x["m"]: x for x in json.load(open(SP + "/refused/ghost4.json"))}
SC = 1109 / max(sum(1 for x in G.values() if not x["keyed"]), 1)
ev = collections.defaultdict(list)                       # signer -> [(cs, keyed)]
for r in rows:
    d = coins.get(r["m"])
    if d: ev[d["create"]["signer"]].append((r["cs"], r["tip"] >= 2e6 or r["sol"] >= 2.0))
for s in ev: ev[s].sort()
def share(sg, cs):
    v = ev[sg]; i = bisect.bisect_left(v, (cs - 150, False)); j = bisect.bisect_left(v, (cs - 216000, False))
    xs = v[j:i]
    return (sum(k for _, k in xs) / len(xs), len(xs)) if xs else (None, 0)
PER = {"P1": ("10-21", "10-22", "10-23"), "P2": ("10-24", "10-25")}
for thr, nmin in ((0.0, 0), (0.5, 2), (0.6, 3), (0.7, 3), (0.8, 3)):
    cell = {}
    for p, ds in PER.items():
        gk = go = 0.0; nk = no = 0
        for r in rows:
            x = G.get(r["m"])
            if not x or r["day"] not in ds: continue
            sh, n = share(coins[r["m"]]["create"]["signer"], r["cs"])
            if thr > 0 and (sh is None or n < nmin or sh < thr): continue
            if x["keyed"]: gk += x["0.1"] - BIAS * x["0.1:add"] - x["react"]; nk += 1
            else: go += (x["0.1"] - BIAS * x["0.1:add"]) * SC; no += 1
        cell[p] = (gk, go, nk, no)
    print(f"   signer keyed share >= {thr:.0%} (>= {nmin} buys):  " + "   ".join(f"{p}: keyed {c[2]} {c[0]:+.2f}, other {c[3]} {c[1]:+.2f} -> {c[0] + c[1]:+.2f}" for p, c in cell.items()))
