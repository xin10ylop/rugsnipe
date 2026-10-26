# Our copies by time of day: (1) simulated copies of him on the 5-day tapes (hours/sim.json), (2) our real trades on
# chain since Oct 25 00:00 UTC (our wallet's SOL change per coin, refused buys' fees in), in 3-hour blocks (Beirut time).
import json, collections, datetime
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
BZ = datetime.timezone(datetime.timedelta(hours=2))      # Beirut now (UTC+2)
def blk(t): return datetime.datetime.fromtimestamp(t, BZ).hour // 3
def day(t): return datetime.datetime.fromtimestamp(t, BZ).strftime("%m-%d")
LAB = [f"{3 * b:02d}-{3 * b + 3:02d}" for b in range(8)]
sim = [(t, v) for t, v, _ in json.load(open(SP + "/hours/sim.json"))]
# our real trades: per coin, first buy time and our SOL change over all our transactions on it
coin = collections.defaultdict(lambda: [None, 0])
for l in open(SP + "/hours/us/4g49xGbg.jsonl"):
    r = json.loads(l)
    ms = {t[0] for t in r["tr"] if t[1] == US} or set(r.get("mints") or [])
    if len(ms) != 1: continue
    m = ms.pop(); c = coin[m]
    c[1] += r["d"]
    if any(t[1] == US and t[2] == 1 for t in r["tr"]) or r.get("err"):
        c[0] = min(c[0] or r["bt"], r["bt"])
real = [(t, d / 1e9) for t, d in coin.values() if t]
def table(name, xs):
    by = collections.defaultdict(list); bd = collections.defaultdict(float)
    for t, v in xs: by[blk(t)].append(v); bd[(blk(t), day(t))] += v
    days = sorted({d for _, d in bd})
    print(f"\n== {name}: {len(xs)} copies, {sum(v for _, v in xs):+.2f} SOL, days {days[0]} to {days[-1]}")
    print("   Beirut   copies  net SOL  per copy   by day")
    for b in range(8):
        v = by.get(b, [])
        cells = " ".join(f"{d} {bd[(b, d)]:+.2f}" if (b, d) in bd else f"{d}   -  " for d in days)
        print(f"   {LAB[b]}h  {len(v):6d}  {sum(v):+7.2f}  {sum(v) / max(len(v), 1):+.4f}   {cells}")
    return {b: sum(by[b]) / len(by[b]) for b in by}
s = table("simulated copies of him (5-day tapes, today's rule: tip >= 2M or >= 2 SOL, 5%, his sell - 1)", sim)
early = table("  the same, Oct 21-23 only", [x for x in sim if day(x[0]) <= "10-23"])
late = table("  the same, Oct 24-25 only", [x for x in sim if day(x[0]) >= "10-24"])
r = table("our real trades on chain since Oct 25 00:00 UTC (every wallet we copy, fees and tips in)", real)
def corr(a, b):
    ks = [k for k in a if k in b]; n = len(ks)
    ra = {k: i for i, k in enumerate(sorted(ks, key=a.get))}; rb = {k: i for i, k in enumerate(sorted(ks, key=b.get))}
    return 1 - 6 * sum((ra[k] - rb[k]) ** 2 for k in ks) / (n * (n * n - 1))
print(f"\n   do the blocks keep their order? simulated Oct 21-23 vs Oct 24-25: {corr(early, late):+.2f}; "
      f"simulated vs our real trades: {corr(s, r):+.2f} (1 = same order, 0 = unrelated)")
