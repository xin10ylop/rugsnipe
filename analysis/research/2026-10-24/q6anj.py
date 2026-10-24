# 6anjqMe's non-SOL-quoted round trips: P&L in SOL from his WSOL + SOL changes in every tx of the trip (sample).
import sys, json, collections, random, statistics as St, math, time
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl")
import tri, pumpparse as PP
from concurrent.futures import ThreadPoolExecutor
W = "6anjqMeWigQFo5wpciARAunVQUXTqoat8w8fFWarCkpc"; WSOL = "So11111111111111111111111111111111111111112"
N = int(sys.argv[1]) if len(sys.argv) > 1 else 400
rows = [json.loads(l) for l in open(f"{SP}/month/6anjqMeW.jsonl")]
by = collections.defaultdict(list)
for r in rows:
    for t in r["tr"]:
        if t[1] == W: by[t[0]].append((r, t))
trips = [(m, xs) for m, xs in by.items() if any(t[2] for r, t in xs) and any(not t[2] for r, t in xs) and not all(t[3] > 0 for r, t in xs)]
random.seed(7); sample = random.sample(trips, min(N, len(trips)))
def sol_wsol(sig):
    t = tri.tx(sig)
    if not t: return None
    ks = PP.keys_of(t); m = t["meta"]
    i = ks.index(W) if W in ks else None
    d = (m["postBalances"][i] - m["preBalances"][i]) / 1e9 if i is not None else 0
    def wsol(bals): return sum(float(b["uiTokenAmount"]["uiAmountString"] or 0) for b in bals or [] if b.get("owner") == W and b["mint"] == WSOL)
    return d + wsol(m.get("postTokenBalances")) - wsol(m.get("preTokenBalances")), t["blockTime"]
def trip_pnl(item):
    m, xs = item
    sigs = sorted({r["sig"] for r, t in xs})
    vals = [sol_wsol(s) for s in sigs]
    if any(v is None for v in vals): return None
    return dict(m=m, pnl=sum(v[0] for v in vals), n=len(sigs), bt=min(v[1] for v in vals), ix=xs[0][1][11])
with ThreadPoolExecutor(3) as ex: res = [r for r in ex.map(trip_pnl, sample) if r]
v = [r["pnl"] for r in res]; mu = St.mean(v); h = 1.96 * St.pstdev(v) / math.sqrt(len(v))
print(f"non-SOL-quoted trips: {len(trips)}; sampled {len(res)}: P&L per trip {mu:+.4f} SOL [{mu-h:+.4f}, {mu+h:+.4f}], win {100*sum(1 for x in v if x > 0)/len(v):.0f}%"
      f" -> all {len(trips)} trips about {mu*len(trips):+.1f} SOL")
D = collections.defaultdict(list)
for r in res: D[time.strftime("%m-%d", time.gmtime(r["bt"]))].append(r["pnl"])
print("  by day (sampled trips: n, mean):", "  ".join(f"{d} {len(x)}/{St.mean(x):+.3f}" for d, x in sorted(D.items())))
json.dump(res, open(SP + "/month/q6anj.json", "w"))
