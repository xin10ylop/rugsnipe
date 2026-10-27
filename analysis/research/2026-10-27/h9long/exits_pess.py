# exits.py's passing rule with pessimistic exits: a trigger (TP / SL) fired within a slot of its own sell is filled after
# its sell (its sell lands ahead of ours: we sell on the state its sell leaves), and a trigger never fills on the very
# trade that fired it (no next-slot trade in the path: the state after the next sampled trade). HAIR takes a further
# share off the proceeds of every TP / SL fill (other copiers selling ahead).   python3 -I exits_pess.py HAIR
import sys, json, datetime
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = "H9kuqGKNYgPFYCA17uUTkaqXGum7MK8y1GwbS7cxsjDg"; STAKE, FEE, BIAS = 1.0, 0.0095, 0.0155
HAIR = float(sys.argv[1]) if len(sys.argv) > 1 else 0.0
TP = float(sys.argv[2]) if len(sys.argv) > 2 else 2.45
SL = float(sys.argv[3]) if len(sys.argv) > 3 else 0.71
ONLY = sys.argv[4] if len(sys.argv) > 4 else ""
import os
ANCHOR = os.environ.get("ANCHOR", ""); AM = json.load(open(f"{SP}/h9long/anchor.json")) if ANCHOR else {}
P = json.load(open(f"{SP}/h9long/{W[:8]}_fpaths.json"))
T = {x["m"]: x for x in json.load(open(f"{SP}/h9long/{W[:8]}_trips.json"))}
COST = {"pump, other quote": 0.9487, "pump, SOL quote": 0.9713}
def per(t): return "P1" if datetime.datetime.fromtimestamp(t, datetime.timezone.utc).day <= 22 else "P2"
Q = json.load(open(f"{SP}/h9long/{W[:8]}_quote.json")); import collections
ALL = []; out = {"P1": [], "P2": []}; byq = collections.defaultdict(list); byd = collections.defaultdict(list); why = {"tp": 0, "sl": 0, "time": 0, "end": 0}; tp_after = 0
for m, v in P.items():
    if not v or "path" not in v: continue
    x = T[m]; b, s = x["tr"]; its = b["v"] or 0
    if its <= 0.05 or (ONLY and ONLY not in x["kind"]): continue
    if ANCHOR and not AM.get(m, "").startswith(ANCHOR): continue
    vbb, vab = b["vt"] + b["tok"], b["vt"]; Qi = 1 / vab - 1 / vbb
    path = [r for r in v["path"] if (r[0], r[1]) > (b["s"], b["i"] or 0)]
    last = b["s"] + 1
    ahead = [r for r in path if r[0] <= last]; va = ahead[-1][5] if ahead else vab
    qin = Qi * STAKE / its; tok = va - 1 / (1 / va + qin)
    rest = [r for r in path if r[0] > last]
    def sell(vx, hair=0.0):
        vxu = vx - tok
        if vxu <= 0: return -STAKE
        g = (1 / vxu - 1 / vx) / qin
        return STAKE * (g * COST[x["kind"]] * (1 - hair) - 1) - 2 * FEE - BIAS
    res = None; tx = None
    for k, r in enumerate(rest):
        px = (vab / r[5]) ** 2
        kind = "tp" if px >= TP else "sl" if px <= SL else "time" if (r[6] and r[6] - b["bt"] >= 1790) else None
        if not kind: continue
        why[kind] += 1
        if kind == "time":
            nxt = [q for q in rest[k + 1:] if q[0] == r[0] + 1]; res = sell(nxt[-1][5] if nxt else r[5]); tx = b["bt"] + 1790; break
        if s["s"] <= r[0] + 1:                       # its own sell lands in our slot or before: ours fills after it
            res = sell(s["vt"], HAIR); tp_after += kind == "tp"; tx = s.get("bt") or r[6]; break
        nxt = [q for q in rest[k + 1:] if q[0] >= r[0] + 1]
        res = sell(nxt[0][5] if nxt else s["vt"], HAIR); tx = r[6] or s.get("bt"); break
    if res is None:                                   # nothing fired on a trade before its sell
        why["end"] += 1
        if s.get("bt") and s["bt"] - b["bt"] >= 1790:  # its time stop: ours fires by the clock first (its pre-sell state)
            res = sell(s["vt"] - s["tok"]); why["end_time"] = why.get("end_time", 0) + 1; tx = b["bt"] + 1790
        else:                                          # its TP / SL jumped past ours in one trade: we sell after it
            res = sell(s["vt"], HAIR); tx = s.get("bt")
    ALL.append((b["bt"], tx or b["bt"] + 1800, res, per(b["bt"])))
    out[per(b["bt"])].append(res); byq[(per(b["bt"]), Q.get(m, "SOL" if "SOL" in x["kind"] else "?"))].append(res)
    byd[datetime.datetime.fromtimestamp(b["bt"], datetime.timezone.utc).strftime("%m-%d")].append(res)
print(f"tp {TP} sl {SL} haircut {HAIR:.0%}: fires {why}, TP fills placed after its sell {tp_after}")
for p, v in out.items():
    v.sort(reverse=True); print(f"   {p} {len(v)} copies  {sum(v):+7.2f}  without best 2 {sum(v[2:]):+7.2f}")
if "-v" in sys.argv:
    print("   by quote token (copies, net):")
    for k in sorted(byq, key=lambda k: (k[0], -abs(sum(byq[k])))):
        if len(byq[k]) >= 3 or abs(sum(byq[k])) > 1: print(f"     {k[0]} {k[1]:10s} {len(byq[k]):4d} {sum(byq[k]):+7.2f}")
    print("   by day (UTC): " + "  ".join(f"{d} {len(v)} {sum(v):+.2f}" for d, v in sorted(byd.items())))
for cap in (1, 2, 3, 5, 99):
    openx, got = [], {"P1": [], "P2": []}
    for bt, tx, res, p in sorted(ALL):
        openx = [e for e in openx if e > bt]
        if len(openx) >= cap: continue
        openx.append(tx); got[p].append(res)
    print(f"   at most {cap} open: " + "  ".join(f"{p} {len(v)} {sum(v):+.2f} ({sum(sorted(v, reverse=True)[2:]):+.2f})" for p, v in got.items()))
