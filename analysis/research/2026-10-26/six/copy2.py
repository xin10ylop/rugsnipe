# The copy of 6anjqMe's buys on the tapes (six/tapes.json): our buy lands at the end of his slot (63% of our landings)
# or the end of the next (37%), bound at his post-trade price + 5%, 0.5 SOL, 1.25% fee each way, 3M buy tip, 0.5M sell
# tip; sold at the end of slot S + h, or the slot after his first sell (we see it, then sell), or (not knowable) the
# slot before his first sell.
import json, collections, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
FEE = 0.0125; TIP = 0.003; SELL_TIP = 0.0005; BASE = 0.00001
tapes = json.load(open(SP + "/six/" + sys.argv[1])); trips = {t["m"]: t for t in json.load(open(SP + "/six/" + sys.argv[2]))}
WIN = int(sys.argv[3])
def pnl(en, ex, stake=0.5):
    v0, t0 = en; k = v0 * t0; s_in = stake * 1e9 / (1 + FEE); q = t0 - k / (v0 + s_in)
    vr, tr = ex
    if tr <= q: return None
    return vr * q / (tr - q) * (1 - FEE) / 1e9 - stake - TIP - SELL_TIP - 2 * BASE
def at(d, slot):
    """the state at the end of `slot`: the last state at or before it (his own post-trade state before any other)"""
    best = None
    for sl, s in d["states"].items():
        if int(sl) <= slot and (best is None or int(sl) > best[0]): best = (int(sl), s)
    return tuple(best[1]) if best else tuple(d["his"])
def val(m, exit_kind, h=None, prem=0.05, w_same=0.63):
    d = tapes[m]; t = trips[m]; S = d["S"]; hisp = d["his"][0] / d["his"][1]
    fs = S + t["hold_slots"] if t["hold_slots"] is not None else None
    out = 0.0; wsum = 0.0
    for land, w in ((S, w_same), (S + 1, 1 - w_same)):
        en = at(d, land)
        if (en[0] / en[1]) / hisp - 1 > prem: out += w * -0.0001; wsum += w; continue
        if exit_kind == "fixed": xs = max(S + h, land + 1)
        elif exit_kind == "follow": xs = max(fs + 1, land + 1) if fs is not None else S + WIN
        else: xs = max(fs - 1, land + 1) if fs is not None else S + WIN
        if xs > S + WIN: xs = S + WIN
        v = pnl(en, at(d, xs))
        if v is None: continue
        out += w * v; wsum += w
    return out / wsum if wsum else None
DAYS = sorted({trips[m]["day"] for m in tapes})
def show(lab, ms, **kw):
    byd = collections.defaultdict(float); n = 0
    for m in ms:
        v = val(m, **kw)
        if v is None: continue
        byd[trips[m]["day"]] += v; n += 1
    tot = sum(byd.values())
    print(f"   {lab:44} n {n:4d} total {tot:+6.2f} /copy {tot / max(n, 1):+.4f}  days+ {sum(1 for d in DAYS if byd[d] > 0)}/{len(DAYS)}  " + " ".join(f"{byd[d]:+.2f}" for d in DAYS))
ms = [m for m in tapes if 1.0 <= trips[m]["stake"] < 2.0]
print(f"== size 1-2 SOL: {len(ms)} coins, his own net on them {sum(trips[m]['pnl'] for m in ms):+.2f} SOL")
for h in (3, 5, 8, 10, 12, 15, 20):
    show(f"sell at end of S + {h}", ms, exit_kind="fixed", h=h)
show("sell the slot after his first sell (follow)", ms, exit_kind="follow")
for p in (0.03, 0.10, 9.0):
    show(f"S + 12, price limit {p:.0%}" if p < 9 else "S + 12, no price limit", ms, exit_kind="fixed", h=12, prem=p)
show("S + 12, our buy always a slot later", ms, exit_kind="fixed", h=12, w_same=0.0)
show("S + 12, stake 1 SOL", ms, exit_kind="fixed", h=12) if False else None
