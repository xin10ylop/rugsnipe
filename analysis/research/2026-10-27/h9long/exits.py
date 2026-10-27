# Our own exits on the full paths (full_paths.py; its buys >= 1.2 SOL): we land at the end of its slot or the next,
# then walk every trade to its sell. A trigger (TP / SL on the price against its post-buy price, a trailing stop, a
# time stop) sells at the end of the next slot (trades of that slot ahead of us). Paths stop at its sell: a rule that
# has not fired by then sells right before its sell (the state its 30-min stop / TP / SL leaves before its own trade).
# Costs as sim2.   python3 -I exits.py WALLET STAKE LAND FEE
import sys, json, collections, datetime, itertools
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W, STAKE, LAND, FEE = sys.argv[1], sys.argv[2], sys.argv[3], float(sys.argv[4]); BIAS = 0.0155
P = json.load(open(f"{SP}/h9long/{W[:8]}_fpaths.json"))
T = {x["m"]: x for x in json.load(open(f"{SP}/h9long/{W[:8]}_trips.json"))}
COST = {"pump, other quote": 0.9487, "pump, SOL quote": 0.9713}
def per(t): return "P1" if datetime.datetime.fromtimestamp(t, datetime.timezone.utc).day <= 22 else "P2"
trips = []
for m, v in P.items():
    if not v or "path" not in v: continue
    x = T[m]; b, s = x["tr"]
    its = b["v"] or 0
    if its <= 0.05: continue
    st = its if STAKE == "its" else float(STAKE)
    vbb, vab = b["vt"] + b["tok"], b["vt"]; Qi = 1 / vab - 1 / vbb
    path = [r for r in v["path"] if (r[0], r[1]) > (b["s"], b["i"] or 0)]
    last = b["s"] if LAND == "same" else b["s"] + 1
    ahead = [r for r in path if r[0] <= last]
    va = ahead[-1][5] if ahead else vab
    qin = Qi * st / its; tok = va - 1 / (1 / va + qin)
    rest = [r for r in path if r[0] > last]
    trips.append(dict(m=m, kind=x["kind"], bt=b["bt"], st=st, vab=vab, va=va, tok=tok, qin=qin, rest=rest, end=s["vt"] - s["tok"], its=x["net"]))
def sell_at(t, vx):
    vxu = vx - t["tok"]
    if vxu <= 0: return -t["st"]
    g = (1 / vxu - 1 / vx) / t["qin"]
    return t["st"] * (g * COST[t["kind"]] - 1) - 2 * FEE - BIAS
def run(t, tp, sl, tmax, trail):
    peak = 1.0
    rest = t["rest"]
    for k, r in enumerate(rest):
        px = (t["vab"] / r[5]) ** 2            # price against its post-buy price (without our own tokens)
        peak = max(peak, px)
        fire = (tp and px >= tp) or (sl and px <= sl) or (tmax and r[6] and r[6] - t["bt"] >= tmax) or \
               (trail and peak >= trail[0] and px <= peak * (1 - trail[1]))
        if fire:
            nxt = [q for q in rest[k + 1:] if q[0] == r[0] + 1]
            vx = nxt[-1][5] if nxt else r[5]
            return sell_at(t, vx)
    return sell_at(t, t["end"])
rules = {"ahead of its own (2.45x / 0.71x / 1790 s)": (2.45, 0.71, 1790, None)}
for tp, sl, tm in itertools.product((1.3, 1.5, 2.0, 2.45, 3.0, None), (0.6, 0.71, 0.8, 0.9, None), (120, 300, 600, 1790)):
    rules[f"tp {tp} sl {sl} time {tm}"] = (tp, sl, tm, None)
for a, b_ in itertools.product((1.2, 1.5, 2.0), (0.15, 0.25, 0.35)):
    rules[f"trail from {a}x, -{b_:.0%} off the peak, sl 0.71, 1790 s"] = (None, 0.71, 1790, (a, b_))
res = []
for name, (tp, sl, tm, tr) in rules.items():
    sc = {}
    for p in ("P1", "P2"):
        v = sorted((run(t, tp, sl, tm, tr) for t in trips if per(t["bt"]) == p), reverse=True)
        sc[p] = (len(v), sum(v), sum(v[2:]))
    res.append((name, sc))
print(f"{len(trips)} trips with full paths, stake {STAKE}, land {LAND}, fee {FEE}, bias {BIAS}")
base = res[0]
print(f"   {base[0]:52s} P1 {base[1]['P1'][0]} {base[1]['P1'][1]:+7.2f} ({base[1]['P1'][2]:+7.2f})  P2 {base[1]['P2'][0]} {base[1]['P2'][1]:+7.2f} ({base[1]['P2'][2]:+7.2f})")
print("   best on P1 (without best 2) -> P2:")
for name, sc in sorted(res[1:], key=lambda r: -r[1]["P1"][2])[:15]:
    print(f"   {name:52s} P1 {sc['P1'][0]} {sc['P1'][1]:+7.2f} ({sc['P1'][2]:+7.2f})  P2 {sc['P2'][0]} {sc['P2'][1]:+7.2f} ({sc['P2'][2]:+7.2f})")
