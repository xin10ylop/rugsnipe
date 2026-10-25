# His month by tip bucket (his own result per buy, creators he bought >= 2 times in 24 h; buys under 2 SOL, the size key
# apart), by day: is the 2M floor robust beyond the 5 tape days?
import json, collections, time
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
T = [t for t in json.load(open(SP + "/month/AHwJSnFN_trips.json")) if t.get("landed") and t.get("closed")]
T.sort(key=lambda t: t["bt"]); seen = collections.defaultdict(list)
for t in T:
    p = seen[t.get("creator")]; t["hot"] = sum(1 for x in p if x >= t["bt"] - 86400) >= 2; p.append(t["bt"])
day = lambda bt: time.strftime("%m-%d", time.gmtime(bt))
B = [(0, 0.5e6), (0.5e6, 1.5e6), (1.5e6, 2e6), (2e6, 3e6), (3e6, 4e6), (4e6, 1e12)]
print(f"{'tip bucket':14} {'buys':>5} {'his net':>9} {'per buy':>8} {'days+':>6}  (size < 2 SOL, hot creators)")
for lo, hi in B:
    s = [t for t in T if t["hot"] and t["stake"] < 2.0 and lo <= (t.get("tip") or 0) < hi]
    byd = collections.defaultdict(float)
    for t in s: byd[day(t["bt"])] += t["pnl"]
    pos = sum(1 for v in byd.values() if v > 0)
    print(f"{lo/1e6:4.1f}-{hi/1e6 if hi < 1e11 else 99:4.1f}M     {len(s):5d} {sum(t['pnl'] for t in s):+9.2f} {sum(t['pnl'] for t in s)/max(len(s),1):+8.4f} {pos:3d}/{len(byd):<3d}")
