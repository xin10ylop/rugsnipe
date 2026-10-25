# His month (Sep 24 - Oct 24, his landed round trips): his own result per day on the buys each copy key takes, on
# creators he bought >= 2 times in the previous 24 h (the live signer rule, by the trip's creator). His P&L, not the
# copy's: on the 5 tape days the copy kept about a fifth of his net on both keys (tip >= 4M: +4.22 of +19.3;
# tip < 4M and size >= 2 SOL: +1.42 of +7.84).
import json, collections, time, bisect
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
T = [t for t in json.load(open(SP + "/month/AHwJSnFN_trips.json")) if t.get("landed") and t.get("closed")]
T.sort(key=lambda t: t["bt"])
seen = collections.defaultdict(list)
for t in T:
    c = t.get("creator"); p = seen[c]
    t["hot"] = sum(1 for x in p if x >= t["bt"] - 86400) >= 2
    p.append(t["bt"])
day = lambda bt: time.strftime("%m-%d", time.gmtime(bt))
days = sorted({day(t["bt"]) for t in T})
keys = [("tip >= 4M", lambda t: (t.get("tip") or 0) >= 4e6),
        ("tip < 4M and size >= 2", lambda t: (t.get("tip") or 0) < 4e6 and t["stake"] >= 2.0),
        ("tip < 4M and size 1.5-2", lambda t: (t.get("tip") or 0) < 4e6 and 1.5 <= t["stake"] < 2.0),
        ("tip < 4M and size < 1.5", lambda t: (t.get("tip") or 0) < 4e6 and t["stake"] < 1.5)]
print(f"{'day':5}  " + "  ".join(f"{k:>26}" for k, _ in keys))
tot = collections.defaultdict(lambda: [0, 0.0, 0]); pos = collections.Counter()
for d in days:
    cells = []
    for k, f in keys:
        s = [t for t in T if day(t["bt"]) == d and t["hot"] and f(t)]
        v = sum(t["pnl"] for t in s)
        tot[k][0] += len(s); tot[k][1] += v; tot[k][2] += 1 if s else 0
        if s and v > 0: pos[k] += 1
        cells.append(f"{len(s):4d} {v:+8.3f} SOL" if s else f"{'-':>17}")
    print(f"{d:5}  " + "  ".join(f"{c:>26}" for c in cells))
print()
for k, _ in keys:
    n, v, dd = tot[k]
    print(f"   {k:26} n {n:5d}  his net {v:+8.2f} SOL  per buy {v / max(n, 1):+.4f}  days positive {pos[k]} of {dd}")
