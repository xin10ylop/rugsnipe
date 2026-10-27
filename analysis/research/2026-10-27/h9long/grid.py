# The long copy's settings tested together under the strict fills of band.py (its sale placed ahead of ours within a
# slot, our trigger filled on the next trade, the cautious price model q + LIFT by default): exits (take profit, stop,
# trailing stop, hold), the entry premium limit, a later landing, and the result by quote token. Every grid is picked
# on Oct 19-22 (P1, without its best 2 copies) and scored on Oct 23-27 (P2); at most 3 open.
#   python3 -I grid.py exits|stops|tp|cap|mix|days|premium|land|quote [MODEL LIFT] [LO HI]   (ROUTES=BC: only the WBTC routes, A: only SOL)
import sys, json, datetime, collections, itertools, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from route_cls import cls
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = "H9kuqGKNYgPFYCA17uUTkaqXGum7MK8y1GwbS7cxsjDg"
WHAT = sys.argv[1]
MODEL = sys.argv[2] if len(sys.argv) > 2 else "q"
LIFT = (sys.argv[3] if len(sys.argv) > 3 else "1") == "1"
LO, HI = (float(sys.argv[4]), float(sys.argv[5])) if len(sys.argv) > 5 else (1.5, 9.0)
FEE, BIAS, ITS_TP = 0.0095, 0.0155, 2.5
COST = {"pump, other quote": 0.9487, "pump, SOL quote": 0.9713}
P = json.load(open(f"{SP}/h9long/{W[:8]}_fpaths.json"))
T = {x["m"]: x for x in json.load(open(f"{SP}/h9long/{W[:8]}_trips.json"))}
Q = json.load(open(f"{SP}/h9long/{W[:8]}_quote.json"))
TRIPS = []
for m, v in P.items():
    if not v or "path" not in v: continue
    x = T[m]
    if len(x["tr"]) != 2: continue
    its = x["tr"][0]["v"] or 0
    if LO <= its < HI and "other" in x["kind"] and cls(x) in os.environ.get("ROUTES", "ABC"):
        TRIPS.append((m, x, v["path"]))
def per(t): return "P1" if datetime.datetime.fromtimestamp(t, datetime.timezone.utc).day <= 22 else "P2"

KIND = {}


def one(x, path0, tp=2.0, sl=0.8, hold=1790, trail=None, land=1, pmax=None, stake=1.0):
    """(buy time, close time, result SOL) or None when the entry is refused; KIND[mint] = the exit that closed it."""
    b, s = x["tr"]; its = b["v"]
    vbb, vab = b["vt"] + b["tok"], b["vt"]; Qi = 1 / vab - 1 / vbb
    path = [r for r in path0 if (r[0], r[1]) > (b["s"], b["i"] or 0)]
    last = b["s"] + land
    ahead = [r for r in path if r[0] <= last]; va = ahead[-1][5] if ahead else vab
    if pmax is not None and (vab / va) ** 2 > 1 + pmax:
        return None
    qin = Qi * stake / its; tok = va - 1 / (1 / va + qin)
    rest = [r for r in path if r[0] > last]
    real = (lambda vr: 1 / (1 / vr + qin)) if MODEL == "q" else (lambda vr: vr - tok)
    def sell(vr):
        vx = real(vr)
        if vx <= 0: return -stake
        g = (1 / vx - 1 / (vx + tok)) / qin
        return stake * (g * COST[x["kind"]] - 1) - 2 * FEE - BIAS
    s_slot, s_after, s_bt = s["s"], s["vt"], s.get("bt")
    if LIFT:
        for r in rest:
            if r[0] >= s["s"]: break
            if (vab / real(r[5])) ** 2 >= ITS_TP:
                s_slot, s_after, s_bt = r[0], r[5] + b["tok"], r[6]; break
            if r[6] and r[6] - b["bt"] >= 1800: break
    peak = 0.0
    for k, r in enumerate(rest):
        if r[0] > s_slot: break
        px = (vab / r[5]) ** 2; peak = max(peak, px)
        if tp and px >= tp: kind = "tp"
        elif sl and px <= sl: kind = "sl"
        elif trail and peak >= trail[0] and px <= peak * (1 - trail[1]): kind = "trail"
        elif r[6] and r[6] - b["bt"] >= hold: kind = "time"
        else: continue
        KIND[x["m"]] = kind if not s_slot <= r[0] + 1 else kind + ", its sell first"
        if kind == "time" and not s_slot <= r[0] + 1:
            nxt = [q for q in rest[k + 1:] if q[0] == r[0] + 1]
            return b["bt"], b["bt"] + hold, sell(nxt[-1][5] if nxt else r[5])
        if s_slot <= r[0] + 1:
            return b["bt"], s_bt or r[6], sell(s_after)
        nxt = [q for q in rest[k + 1:] if q[0] >= r[0] + 1]
        return b["bt"], r[6] or s_bt, sell(nxt[0][5] if nxt else s_after)
    if s_bt and s_bt - b["bt"] >= min(hold, 1790) and s_slot == s["s"]:
        KIND[x["m"]] = "time (its 30-minute sell)"
        return b["bt"], b["bt"] + min(hold, 1790), sell(s["vt"] - s["tok"])
    # no rule of ours fired before its own sell: the path ends there, so we sell right after it (the engine must follow)
    KIND[x["m"]] = "its early sell, we follow" if s_slot == s["s"] else "its 2.5x sell (lift), we follow"
    return b["bt"], s_bt, sell(s_after)

def score(cap=3, **kw):
    rows = []
    for m, x, path in TRIPS:
        r = one(x, path, **kw)
        if r: rows.append((r[0], r[1] or r[0] + 1800, r[2], m))
    openx, got = [], {"P1": [], "P2": []}
    for bt, tx, res, m in sorted(rows):
        openx = [e for e in openx if e > bt]
        if len(openx) >= cap: continue
        openx.append(tx); got[per(bt)].append((res, m))
    out = {}
    for p, v in got.items():
        r = sorted((a for a, _ in v), reverse=True)
        out[p] = (len(r), sum(r), sum(r[2:]))
    return out, got

def show(name, sc):
    a, b = sc["P1"], sc["P2"]
    print(f"   {name:44s} P1 {a[0]:3d} {a[1]:+6.2f} ({a[2]:+6.2f})   P2 {b[0]:3d} {b[1]:+6.2f} ({b[2]:+6.2f})")

print(f"{len(TRIPS)} routable buys of {LO}-{HI} SOL, model {MODEL}, its exits see our tokens {LIFT}, at most 3 open")
show("as built: tp 2.0 sl 0.8 1790 s", score()[0])
if WHAT == "exits":
    res = []
    for tp, sl, hold in itertools.product((1.5, 1.75, 2.0, 2.25, None), (0.6, 0.7, 0.75, 0.8, 0.85, 0.9, None),
                                          (600, 1200, 1790)):
        res.append((f"tp {tp} sl {sl} {hold} s", score(tp=tp, sl=sl, hold=hold)[0]))
    for a, f, sl in itertools.product((1.3, 1.5, 1.75, 2.0), (0.15, 0.25, 0.35), (0.8, None)):
        res.append((f"trail from {a}x -{f:.0%}, sl {sl}, 1790 s", score(tp=None, sl=sl, trail=(a, f))[0]))
    print("   best 15 on P1 (without its best 2):")
    for name, sc in sorted(res, key=lambda r: -r[1]["P1"][2])[:15]:
        show(name, sc)
    rank = sorted(res, key=lambda r: -r[1]["P1"][2])
    i = next(k for k, (n, _) in enumerate(rank) if n == "tp 2.0 sl 0.8 1790 s")
    print(f"   the built rule ranks {i + 1} of {len(rank)} on P1; on P2 it ranks "
          f"{1 + sorted(res, key=lambda r: -r[1]['P2'][2]).index(rank[i])}")
elif WHAT == "stops":
    for sl in (0.75, 0.8, 0.85, 0.9):
        show(f"tp 2.0 sl {sl} 1790 s", score(sl=sl)[0])
elif WHAT == "tp":
    # take profit from 1.1x up, at the built stop and hold, and with a tighter stop; won = closed above zero
    for sl in (0.8, 0.9):
        for tp in (1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 1.75, 2.0, 2.25, 2.5, 3.0, None):
            sc, got = score(tp=tp, sl=sl)
            won = "  ".join(f"{p} won {sum(r > 0 for r, _ in got[p])}/{len(got[p])}" for p in ("P1", "P2"))
            show(f"tp {tp} sl {sl} 1790 s", sc)
            print(f"   {'':44s} {won}")
elif WHAT == "mix":
    # what closed each copy as built, and what it made: the rows "we follow" are copies the backtest sells right after
    # the wallet's own sell (no price path after it)
    _, got = score()
    for p in ("P1", "P2"):
        g = collections.defaultdict(list)
        for res, m in got[p]: g[KIND.get(m, "?")].append(res)
        print(f"   {p}:")
        for k, v in sorted(g.items(), key=lambda kv: -len(kv[1])):
            print(f"     {k:34s} {len(v):4d} copies  {sum(v):+7.2f} SOL  mean {sum(v) / len(v):+.3f}")
elif WHAT == "cap":
    # positions open at once (each takes 1 SOL): the bankroll the cap needs against what it adds
    for cap in (1, 2, 3, 4, 5, 7, 10, 99):
        show(f"at most {cap} open", score(cap=cap)[0])
elif WHAT == "days":
    _, got = score()
    for p in ("P1", "P2"):
        g = collections.defaultdict(list)
        for res, m in got[p]:
            g[datetime.datetime.fromtimestamp(T[m]["tr"][0]["bt"], datetime.timezone.utc).strftime("%m-%d")].append(res)
        print(f"   {p}: " + "  ".join(f"{d} {len(v)} {sum(v):+.2f}" for d, v in sorted(g.items())))
elif WHAT == "premium":
    for pm in (None, 0.5, 0.25, 0.15, 0.10, 0.07, 0.05, 0.03):
        show(f"entry at most {pm} over its price", score(pmax=pm)[0])
elif WHAT == "land":
    for land in (1, 2, 3, 5, 10):
        show(f"landing {land} slot(s) after its slot", score(land=land)[0])
elif WHAT == "quote":
    _, got = score()
    for p in ("P1", "P2"):
        g = collections.defaultdict(list)
        for res, m in got[p]: g[Q.get(m, "?")].append(res)
        print(f"   {p}: " + "  ".join(f"{q} {len(v)} {sum(v):+.2f}" for q, v in sorted(g.items(), key=lambda kv: -len(kv[1]))[:8]))
