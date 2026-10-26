# Is our short exit the problem? For each pulled wallet (oos/<w>.jsonl: its own landed transactions since Oct 25 18:00,
# pump curve trades with the reserves after each), per coin: its own round trip, and a 0.5 SOL copy that buys at the
# curve state after its buy plus a premium (our lag: +2% / +5%), then sells
#   (a) "+9"      at create + 9 if the tape has it (oos/coin), else skipped
#   (b) "first"   right after its first sell (the curve state after that sell: we sell behind it)
#   (c) "last"    right after its last sell
#   (d) "half"    half at its first sell, half at its last
# Coins it still holds, or sold off the curve (after migration), are counted apart. 1.25% fee each way, 3M + 0.5M tips.
import json, glob, os, collections, statistics as st, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
K = 30_000_000_000 * 1_073_000_000_000_000
FEE = 0.0125; STAKE = 0.5; COST = 0.003 + 0.0005 + 0.00002
def val(entry, ex, prem, frac=1.0):
    v0, t0 = entry; v0 = v0 * (1 + prem) ** 0.5; t0 = K / v0          # the curve moved prem against us before we buy
    s_in = STAKE * 1e9 / (1 + FEE); q = t0 - K / (v0 + s_in)
    vr, tr = ex; q2 = q * frac
    return vr * q2 / (tr + q2) * (1 - FEE) / 1e9
def rt(entry, exits, prem):
    """exits: [(state, fraction)] -> net SOL"""
    return sum(val(entry, e, prem, f) for e, f in exits) - STAKE - COST
tapes = {}
for p in glob.glob(SP + "/oos/coin/*.json"):
    d = json.load(open(p))
    if d.get("rows"): tapes[d["mint"]] = d
def state_end(d, slot):
    v = None
    for r in sorted(d["rows"], key=lambda r: (r[0], r[1])):
        if r[0] > slot: break
        if r[7] > 0: v = (r[6] if r[6] > 0 else K // r[7], r[7])
    return v
W = [l.strip() for l in open(sys.argv[1] if len(sys.argv) > 1 else SP + "/oos/finalists1.txt") if l.strip()]
src = sys.argv[2] if len(sys.argv) > 2 else SP + "/oos"
print(f"{'wallet':9} {'coins':>5} {'own net':>8} {'closed':>6} {'holds':>5} {'med hold':>8} | copy +2%:  {'+9':>6} {'first':>6} {'last':>6} {'half':>6} | +5%: {'first':>6} {'last':>6} | share of own net from top 3 coins")
for w in W:
    p = f"{src}/{w[:8]}.jsonl"
    if not os.path.exists(p): continue
    coins = collections.defaultdict(list); dsum = collections.defaultdict(int)
    for l in open(p):
        r = json.loads(l)
        mine = [t for t in r.get("tr") or [] if t[1] == w]
        for t in mine: coins[t[0]].append((r["s"], r["i"] or 0, t[2], t[3], t[4], t[5], t[6], r.get("bt")))
        ms = {t[0] for t in mine} or set(r.get("mints") or [])
        for m in ms: dsum[m] += r["d"]           # its SOL balance change on the coin (any venue)
    res = collections.defaultdict(list); n = closed = held = 0; holds = []; own = []
    for m, xs in coins.items():
        xs.sort()
        b = [x for x in xs if x[2] == 1]; s = [x for x in xs if x[2] == 0]
        if not b: continue
        n += 1; own.append(dsum[m] / 1e9)
        bought = sum(x[4] for x in b); sold = sum(x[4] for x in s)
        if not s or sold < 0.95 * bought: held += 1; continue
        closed += 1
        def stt(x):
            v, t = x[5], x[6]
            if t <= 0: return None
            return (v if v > 0 else K // t, t)
        e, f, g = stt(b[0]), stt(s[0]), stt(s[-1])
        if not (e and f and g): continue
        if b[0][7] and s[0][7]: holds.append(s[0][7] - b[0][7])
        for prem in (0.02, 0.05):
            res[("first", prem)].append(rt(e, [(f, 1.0)], prem))
            res[("last", prem)].append(rt(e, [(g, 1.0)], prem))
            res[("half", prem)].append(rt(e, [(f, 0.5), (g, 0.5)], prem))
            d = tapes.get(m)
            if d and d.get("cs"):
                x9 = state_end(d, d["cs"] + 9)
                if x9 and b[0][0] <= d["cs"] + 1: res[("+9", prem)].append(rt(e, [(x9, 1.0)], prem))
    if not n: continue
    srt = sorted(own, reverse=True); tot = sum(own)
    top3 = sum(srt[:3]) / tot if tot > 0 else float("nan")
    S = lambda k: sum(res[k])
    print(f"{w[:8]:9} {n:5d} {tot:+8.2f} {closed:6d} {held:5d} {st.median(holds) if holds else 0:7.0f}s | {S(('+9', 0.02)):+6.2f} ({len(res[('+9', 0.02)])}) {S(('first', 0.02)):+6.2f} {S(('last', 0.02)):+6.2f} {S(('half', 0.02)):+6.2f} | {S(('first', 0.05)):+6.2f} {S(('last', 0.05)):+6.2f} | {top3:.0%}")
