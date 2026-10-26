# Dump-and-bounce backtest on the fetched paths (flush/path/<mint>.json: the curve every 3 slots for 3 minutes after the
# dump). Our trades act one bucket after the signal (we see it, then land). 0.5 SOL, 1.25% fee each way, 1M buy tip +
# 0.5M sell tip, our own price impact through the curve.
#   entry  now      the bucket after the dump bucket
#          bounce r the bucket after the first one whose price is >= the post-dump low x (1 + r)
#          deep L   the bucket after the first one whose price is <= the high x (1 - L)
#   exit   take profit TP (the bucket after the price reaches entry x (1 + TP)), stop SL, else the time limit T
# By day (create day) and by seller; events from flush/events_*.json.   python3 -I flush/bounce.py EVENTS [EVENTS_UNSEEN]
import json, os, sys, collections, statistics as st, datetime, bisect, itertools
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
K = 30_000_000_000 * 1_073_000_000_000_000
P0 = 30_000_000_000 / 1_073_000_000_000_000
FEE = 0.0125; STAKE = 0.5; COST = 0.001 + 0.0005 + 0.00002
PAIRS = sorted({(r["slot"], r["bt"]) for w in ("AHwJSn", "4g49xG") for r in json.load(open(f"{SP}/dec/five/walk_{w}.json"))["rows"] if r.get("bt")})
for p in os.listdir(SP + "/oos"):
    if p.endswith(".jsonl"):
        for l in open(SP + "/oos/" + p):
            r = json.loads(l)
            if r.get("bt"): PAIRS.append((r["s"], r["bt"]))
PAIRS.sort(); PS = [p[0] for p in PAIRS]
def day(slot):
    i = min(max(bisect.bisect_left(PS, slot), 1), len(PS) - 1)
    (s0, t0), (s1, t1) = PAIRS[i - 1], PAIRS[i]
    t = int(t0 + (slot - s0) * (t1 - t0) / max(1, s1 - s0))
    return datetime.datetime.fromtimestamp(t, datetime.timezone.utc).strftime("%m-%d")
def series(path):
    out = []; last = None
    for b in path["buckets"]:
        if b[1] and b[2] and b[2] > 0: last = (b[1], b[2])
        out.append((b[0], last))
    return out
def pnl(entry, ex):
    v0, t0 = entry; k = v0 * t0; s_in = STAKE * 1e9 / (1 + FEE); q = t0 - k / (v0 + s_in)
    vr, tr = ex
    if tr <= q: return None
    return vr * q / (tr - q) * (1 - FEE) / 1e9 - STAKE - COST
def price(s): return s[0] / s[1]
def trade(ev, S, entry, tp, sl, T):
    """one trade's net SOL, or None (no entry)"""
    lo = None; i0 = None
    hi = ev["hi"] * P0
    for i, (slot, s) in enumerate(S):
        if s is None: continue
        p = price(s); lo = p if lo is None else min(lo, p)
        if entry[0] == "now": i0 = i + 1; break
        if entry[0] == "bounce" and i > 0 and p >= lo * (1 + entry[1]): i0 = i + 1; break
        if entry[0] == "deep" and p <= hi * (1 - entry[1]): i0 = i + 1; break
    if i0 is None or i0 >= len(S) or S[i0][1] is None: return None
    en = S[i0][1]; pe = price(en); t_end = S[i0][0] + T
    for j in range(i0 + 1, len(S)):
        slot, s = S[j]
        if s is None: continue
        if slot >= t_end: return pnl(en, s)
        p = price(s)
        if (tp and p >= pe * (1 + tp)) or (sl and p <= pe * (1 - sl)):
            k = min(j + 1, len(S) - 1)
            return pnl(en, S[k][1] or s)
    last = next((s for _, s in reversed(S) if s), None)
    return pnl(en, last) if last else None
def load(evf):
    E = json.load(open(evf)); out = []
    for e in E:
        p = f"{SP}/flush/path/{e['m']}.json"
        if not os.path.exists(p): continue
        path = json.load(open(p))
        if path["trunc"]: e["trunc"] = True
        e["S"] = series(path); e["day"] = day(path["dump_slot"]); out.append(e)
    return out
IN = load(sys.argv[1]); UN = load(sys.argv[2]) if len(sys.argv) > 2 else []
print(f"events with a path: in-sample {len(IN)} ({collections.Counter(e['day'] for e in IN)}), unseen {len(UN)}; truncated {sum(1 for e in IN + UN if e.get('trunc'))}")
ENTRY = [("now", 0), ("bounce", 0.05), ("bounce", 0.10), ("deep", 0.5), ("deep", 0.6)]
TP = [0.2, 0.3, 0.4, None]; SL = [0.15, 0.25, None]; TT = [75, 150, 300, 450]
SELL = {"all": lambda e: True, "dev": lambda e: e["who"] == "dev", "bundle": lambda e: e["who"] == "bundle", "other": lambda e: e["who"] == "other"}
def run(evs, entry, tp, sl, T, f):
    byd = collections.defaultdict(float); n = 0; w = 0
    for e in evs:
        if not f(e): continue
        v = trade(e, e["S"], entry, tp, sl, T)
        if v is None: continue
        byd[e["day"]] += v; n += 1; w += v > 0
    return n, sum(byd.values()), byd, w
res = []
for entry, tp, sl, T, (fl, f) in itertools.product(ENTRY, TP, SL, TT, SELL.items()):
    n, tot, byd, w = run(IN, entry, tp, sl, T, f)
    if n < 30: continue
    res.append(dict(entry=entry, tp=tp, sl=sl, T=T, f=fl, n=n, tot=tot, byd=dict(byd), win=w / n, pos=all(v > 0 for v in byd.values()) and len(byd) >= 3))
days = sorted({e["day"] for e in IN})
print(f"variants tried {len(res)}; positive in total {sum(1 for r in res if r['tot'] > 0)}; positive on every in-sample day {sum(1 for r in res if r['pos'])}")
def lab(r): return f"{r['entry'][0]} {r['entry'][1]:<4} tp {str(r['tp']):4} sl {str(r['sl']):4} T {r['T'] * 0.4:3.0f}s {r['f']:6}"
print("the plain strategy (buy right after the dump), each exit, all sellers:")
for r in res:
    if r["entry"] == ("now", 0) and r["f"] == "all" and r["sl"] in (0.25, None) and r["T"] in (150, 450):
        print(f"   {lab(r)} n {r['n']:4d} total {r['tot']:+6.2f} win {r['win']:4.0%}  " + " ".join(f"{d[3:]}:{r['byd'].get(d, 0):+.2f}" for d in days))
best = sorted([r for r in res if r["pos"]], key=lambda r: -r["tot"])[:12]
print("best variants positive on every in-sample day, then on the unseen period:")
for r in best:
    un = run(UN, r["entry"], r["tp"], r["sl"], r["T"], SELL[r["f"]]) if UN else (0, 0, {}, 0)
    print(f"   {lab(r)} n {r['n']:4d} total {r['tot']:+6.2f} win {r['win']:4.0%}  " + " ".join(f"{d[3:]}:{r['byd'].get(d, 0):+.2f}" for d in days) +
          f"  | unseen n {un[0]:3d} {un[1]:+6.2f}")
