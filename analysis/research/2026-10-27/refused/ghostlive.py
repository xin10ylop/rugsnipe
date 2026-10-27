# The ghost copy on our LIVE copies (Oct 25 22:08 UTC on), at our real reactive positions. Ghost entry: after the
# wallet's buy and j more trades, j from the 83 create-sent buys of the 5-day data (presend.json), each ghost after the
# wallet with P_AFTER (else today's copy); the wallet's exit as the bot sells it. Bounds: today's 5% (20% for wallet 4)
# over the wallet's price (an oracle: needs its price), or the post-bundle price + CAP (what a pre-sent buy can fix).
# Corrections as ghost3: 0.0155 off every added fill, each wallet's best 2 coins out; the spray (0.000015 a launch on
# 1,861 launches a day) is charged per day of live data.
#   python3 -I ghostlive.py [P_AFTER]
import sys, json, os, bisect, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
P_AFTER = float(sys.argv[1]) if len(sys.argv) > 1 else 0.8
FEE = 0.0125; COST = 0.0045; BIAS = 0.0155; REF = -0.000015; CF = 0.000015; LAUNCH_DAY = 1861
J = json.load(open(SP + "/refused/presend.json"))["js"]; JP = {j: c / len(J) for j, c in collections.Counter(J).items()}
R = json.load(open(SP + "/refused/live_recs.json")); L = json.load(open(SP + "/refused/live.json"))["mine"]
WAL = {"him": "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2", "3yrJE1A8": "3yrJE1A8dgSuy5NLimG5GSYxA6wMkvnAYLD9Y4xR2fjJ",
       "JBokzLFR": "JBokzLFRg8CEcThrjweWa9WoZoZPYitRSVffXuxdHHVu", "H9kuqGKN": "H9kuqGKNYgPFYCA17uUTkaqXGum7MK8y1GwbS7cxsjDg",
       "CxyKYmQo": "CxyKYmQoaYLCds6puxSbWndk6ktqax8PwAeq8enRzph8", "D5BDDkBt": "D5BDDkBtjjva35nwu45WfmZgytsog9GST2tHQaZUYuFX"}
def pnl(en, ex, stake=0.5):
    v0, t0 = en; k = v0 * t0; s_in = stake * 1e9 / (1 + FEE); q = t0 - k / (v0 + s_in)
    vr, tr = ex
    if tr <= q: return None
    return vr * q / (tr - q) * (1 - FEE) / 1e9 - stake - COST
def st_before(seq, key):
    i = bisect.bisect_left(seq, key, key=lambda x: (x[0], x[1]))
    return (seq[i - 1][6], seq[i - 1][7]) if i else None
def st_end(seq, slot): return st_before(seq, (slot + 1, -1))
out = []
for r in R:
    t = json.load(open(f"{SP}/refused/tape/{r['m']}.json")); seq = [x for x in t["rows"] if x[7] > 0]; cs = t["c0"]
    w = WAL[r["who"]]
    wb = next((x for x in seq if x[2] == w and x[3] == 1 and x[0] <= cs + 1), None)
    if not wb or not seq: continue
    first = seq[0]; pbx = first
    for x in seq[1:]:
        if x[0] != cs or x[1] - pbx[1] > 2 or x[1] - first[1] > 5 or x[3] != 1: break
        pbx = x
    pbp = pbx[6] / pbx[7]; wp = wb[6] / wb[7]
    ex = st_end(seq, cs + r["ex_slot"])
    att = sorted(L[r["m"]], key=lambda a: (a["slot"], a["idx"] or 0))[0]
    en = st_before(seq, (att["slot"], att["idx"] or 0))
    base = (pnl(en, ex) if (en[0] / en[1]) / wp - 1 <= r["bound"] else REF) if en and ex else None
    if base is None: continue
    after = [x for x in seq if (x[0], x[1]) > (wb[0], wb[1])]
    res = dict(m=r["m"], who=r["who"], bt=r["bt"], react=base, status=r["status"])
    for cap in (None, 0.10, 0.20, 0.30):
        e = (1 - P_AFTER) * base; add = 0.0
        for j, p in JP.items():
            s = (wb[6], wb[7]) if j == 0 else ((after[j - 1][6], after[j - 1][7]) if j <= len(after) else None)
            ok = s is not None and (((s[0] / s[1]) / wp - 1 <= r["bound"]) if cap is None else ((s[0] / s[1]) / pbp - 1 <= cap))
            v = pnl(s, ex) if ok else None
            if v is None: e += P_AFTER * p * base
            else:
                e += P_AFTER * p * v
                if base == REF: add += P_AFTER * p
        res[str(cap)] = e; res[f"{cap}:add"] = add
    out.append(res)
span_days = (max(x["bt"] for x in out) - min(x["bt"] for x in out)) / 86400 if out else 0
print(f"{len(out)} live copies with tapes, over {span_days:.2f} days; P_AFTER {P_AFTER}")
for who in ["all"] + list(WAL):
    xs = [x for x in out if who == "all" or x["who"] == who]
    if not xs: continue
    line = f"   {who:9} copies {len(xs):3d}  today {sum(x['react'] for x in xs):+.3f}  ghost gain:"
    for cap in (None, 0.10, 0.20, 0.30):
        g = sorted((x[str(cap)] - BIAS * x[f"{cap}:add"] - x["react"] for x in xs), reverse=True)
        lab = "oracle" if cap is None else f"pb+{cap:.0%}"
        line += f"  {lab} {sum(g):+.3f} [{sum(g[2:]):+.3f}]"
    print(line)
print(f"   spray cost over the period: {CF * LAUNCH_DAY * span_days:.3f} SOL (one ghost a launch, every in-class launch)")
