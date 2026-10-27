# Live refused copies (pump 6042 at our real position), every wallet: what each mechanism would have made, at our real
# position and the wallet's exit. Bands of the premium, sizes (0.5 / 0.2 SOL), exits (create + 3 .. + 20), a
# re-entry when a slot closes within x of the wallet's price before it sells; per wallet. Early half picks, late half
# scores; 0.0155 off every added fill; each half without its best 2 coins in brackets.
import json, collections, itertools
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
R = json.load(open(SP + "/refused/live_recs.json")); BIAS = 0.0155; REF = -0.000015
ref = [r for r in R if r["status"] == "refused"]
ts = sorted(r["bt"] for r in ref); MID = ts[len(ts) // 2] if ts else 0
print(f"{len(R)} live copies, {len(ref)} refused; halves split at {MID}")
print("   by wallet: refused, premium at our position (median), what filling them adds at its exit [without the best 2]")
def add(xs, key):
    return [(r["v"].get(key) - BIAS - REF) for r in xs if r["v"].get(key) is not None]
for who in ("him", "3yrJE1A8", "JBokzLFR", "H9kuqGKN", "CxyKYmQo", "D5BDDkBt"):
    xs = [r for r in ref if r["who"] == who]
    if not xs: continue
    a = sorted(add(xs, "fill"), reverse=True)
    ps = sorted(r["prem"] for r in xs)
    print(f"   {who:9} {len(xs):3d}  median premium {ps[len(ps) // 2]:.0%}  fill adds {sum(a):+.3f} [{sum(a[2:]):+.3f}]   "
          + " ".join(f"{r['prem']:.0%}:{(r['v']['fill'] or 0) - BIAS:+.3f}" for r in sorted(xs, key=lambda r: r["prem"])))
BANDS = [(lo, hi) for lo in (0.05, 0.10, 0.15, 0.20, 0.25, 0.30) for hi in (0.10, 0.15, 0.20, 0.30, 0.45, 0.60, 1.0) if hi > lo]
KEYS = ["fill", "fill02", "c3", "c5", "c7", "c12", "c15", "c20", "re0.0", "re0.03", "re0.05"]
grid = []
for who in ("all", "him", "3yrJE1A8", "JBokzLFR", "H9kuqGKN"):
    for (lo, hi), k in itertools.product(BANDS, KEYS):
        xs = [r for r in ref if (who == "all" or r["who"] == who) and (k.startswith("re") or lo < r["prem"] - (r["bound"] - 0.05) <= hi)]
        e = sorted(add([r for r in xs if r["bt"] < MID], k), reverse=True); l = sorted(add([r for r in xs if r["bt"] >= MID], k), reverse=True)
        if not e and not l: continue
        grid.append((sum(e), sum(l), sum(e[2:]), sum(l[2:]), who, lo, hi, k, len(e), len(l)))
grid.sort(reverse=True)
print(f"\n== {len(grid)} settings (wallet x premium band over its own bound x size/exit/re-entry), the 12 best on the early half:")
print("   early  late   e-2    l-2    wallet     band       key      n early/late")
for g in grid[:12]:
    print(f"   {g[0]:+.3f} {g[1]:+.3f} {g[2]:+.3f} {g[3]:+.3f} {g[4]:9} {g[5]:.0%}-{g[6]:.0%}  {g[7]:8} {g[8]}/{g[9]}")
pe = [g for g in grid if g[2] > 0]
print(f"   positive early without its best 2: {len(pe)}; of those also positive late without its best 2: {sum(1 for g in pe if g[3] > 0)}")
