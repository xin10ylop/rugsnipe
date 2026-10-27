# By the size of its buy: the long copy's rule (TP 2.0 / SL 0.8 / 1790 s, its sell placed ahead of ours within a slot) on
# its routable buys (route_cls.py) of LO <= SOL value < HI of coins priced in another token, at STAKE SOL (or "xF": F
# times its buy), under scale.py's price models (v: the recorded path shifted by our tokens; q: by our quote; LIFT=1:
# its exits fire on the price with our tokens in).   python3 -I band.py LO HI STAKE MODEL LIFT [HAIR]
# scale.py's notes on the models:
#   v: the recorded path shifted by our tokens (others trade fixed token amounts: exits_pess.py's model). At a bigger
#      stake the later buyers pay more for the same tokens and that flows to us: gains grow faster than the stake.
#   q: the recorded path shifted by our quote (others spend fixed SOL amounts, as copy bots do): our own tokens cost
#      us on the way out.
# LIFT=1: the wallet's own exits fire on the price with our tokens in (it watches the real curve): its 2.5x sale comes
# earlier by our lift, (1 + qin * v)^2, and lands ahead of our 2.0x once the lift passes 1.25.
#   python3 -I scale.py STAKE MODEL LIFT [HAIR]      e.g. python3 -I scale.py 2 q 1
import sys, json, datetime, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = "H9kuqGKNYgPFYCA17uUTkaqXGum7MK8y1GwbS7cxsjDg"
LO, HI, SK, MODEL, LIFT = float(sys.argv[1]), float(sys.argv[2]), sys.argv[3], sys.argv[4], sys.argv[5] == "1"
HAIR = float(sys.argv[6]) if len(sys.argv) > 6 else 0.0
sys.path.insert(0, __import__("os").path.dirname(__import__("os").path.abspath(__file__)))
from route_cls import cls
TP, SL, FEE, BIAS = 2.0, 0.8, 0.0095, 0.0155
ITS_TP, ITS_SL = 2.5, 0.7
P = json.load(open(f"{SP}/h9long/{W[:8]}_fpaths.json"))
T = {x["m"]: x for x in json.load(open(f"{SP}/h9long/{W[:8]}_trips.json"))}
COST = {"pump, other quote": 0.9487, "pump, SOL quote": 0.9713}
def per(t): return "P1" if datetime.datetime.fromtimestamp(t, datetime.timezone.utc).day <= 22 else "P2"
ALL, early = [], 0
for m, v in P.items():
    if not v or "path" not in v: continue
    x = T[m]
    if len(x["tr"]) != 2: continue
    b, s = x["tr"]; its = b["v"] or 0
    if not LO <= its < HI or "other" not in x["kind"] or cls(x) not in "ABC": continue
    STAKE = its * float(SK[1:]) if SK.startswith("x") else float(SK)
    vbb, vab = b["vt"] + b["tok"], b["vt"]; Qi = 1 / vab - 1 / vbb
    path = [r for r in v["path"] if (r[0], r[1]) > (b["s"], b["i"] or 0)]
    last = b["s"] + 1
    ahead = [r for r in path if r[0] <= last]; va = ahead[-1][5] if ahead else vab
    qin = Qi * STAKE / its; tok = va - 1 / (1 / va + qin)
    rest = [r for r in path if r[0] > last]
    def real(vr):                                  # the curve's token reserve with our position in it
        return 1 / (1 / vr + qin) if MODEL == "q" else vr - tok
    def sell(vr, hair=0.0):
        vx = real(vr)
        if vx <= 0: return -STAKE
        g = (1 / vx - 1 / (vx + tok)) / qin
        return STAKE * (g * COST[x["kind"]] * (1 - hair) - 1) - 2 * FEE - BIAS
    # its sale: as recorded, or earlier when its own TP fires on the price with our tokens in
    s_slot, s_after, s_bt = s["s"], s["vt"], s.get("bt")
    if LIFT:
        for r in rest:
            if r[0] >= s["s"]: break
            if ((vab / real(r[5])) ** 2 >= ITS_TP) or (r[6] and r[6] - b["bt"] >= 1800):
                if (vab / real(r[5])) ** 2 >= ITS_TP:
                    s_slot, s_after, s_bt = r[0], r[5] + b["tok"], r[6]; early += 1
                break
    res = tx = None
    for k, r in enumerate(rest):
        if r[0] > s_slot: break
        px = (vab / r[5]) ** 2
        kind = "tp" if px >= TP else "sl" if px <= SL else "time" if (r[6] and r[6] - b["bt"] >= 1790) else None
        if not kind: continue
        if kind == "time":
            nxt = [q for q in rest[k + 1:] if q[0] == r[0] + 1]; res = sell(nxt[-1][5] if nxt else r[5]); tx = b["bt"] + 1790; break
        if s_slot <= r[0] + 1:
            res = sell(s_after, HAIR); tx = s_bt or r[6]; break
        nxt = [q for q in rest[k + 1:] if q[0] >= r[0] + 1]
        res = sell(nxt[0][5] if nxt else s_after, HAIR); tx = r[6] or s_bt; break
    if res is None:
        if s_bt and s_bt - b["bt"] >= 1790 and s_slot == s["s"]:
            res = sell(s["vt"] - s["tok"]); tx = b["bt"] + 1790
        else:
            res = sell(s_after, HAIR); tx = s_bt
    ALL.append((b["bt"], tx or b["bt"] + 1800, res, per(b["bt"])))
print(f"its buys {LO}-{HI} SOL, stake {SK}, model {MODEL}, its exits see our tokens {LIFT}, haircut {HAIR:.0%}: "
      f"{len(ALL)} copies, its TP earlier by our lift on {early}")
for cap in (3, 5, 99):
    openx, got = [], {"P1": [], "P2": []}
    for bt, tx, res, p in sorted(ALL):
        openx = [e for e in openx if e > bt]
        if len(openx) >= cap: continue
        openx.append(tx); got[p].append(res)
    print(f"   at most {cap} open: " + "  ".join(f"{p} {len(v)} {sum(v):+.2f} ({sum(sorted(v, reverse=True)[2:]):+.2f})" for p, v in got.items()))
