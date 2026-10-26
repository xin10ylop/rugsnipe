# Hot hours? His landed trades Sep 24 - Oct 24 (month/AHwJSnFN_trips.json): his own net per hour of the day, how many
# days each hour was positive, and whether the hours that led in the first half of the month also led in the second.
import json, collections, datetime, statistics as st
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
T = [r for r in json.load(open(SP + "/month/AHwJSnFN_trips.json")) if r.get("landed") and r.get("closed")]
MID = 1791552000   # Oct 9 00:00 UTC
def rank_corr(a, b):
    ra = {k: i for i, k in enumerate(sorted(a, key=a.get))}; rb = {k: i for i, k in enumerate(sorted(b, key=b.get))}
    ks = list(a); n = len(ks)
    return 1 - 6 * sum((ra[k] - rb[k]) ** 2 for k in ks) / (n * (n * n - 1))
for name, rows in (("all his trades", T), ("the ones our rule copies (tip >= 2M or >= 2 SOL)", [r for r in T if r["tip"] >= 2_000_000 or r["sol_in"] >= 2.0])):
    by = collections.defaultdict(list); day = collections.defaultdict(float); h1 = collections.defaultdict(float); h2 = collections.defaultdict(float)
    for r in rows:
        dt = datetime.datetime.fromtimestamp(r["bt"], datetime.timezone.utc)
        h = dt.hour; by[h].append(r["pnl"]); day[(h, dt.date())] += r["pnl"]
        (h1 if r["bt"] < MID else h2)[h] += r["pnl"]
    days = sorted({d for _, d in day})
    print(f"\n== {name}: {len(rows)} trades over {len(days)} days, his net {sum(r['pnl'] for r in rows):+.1f} SOL")
    print("   UTC   Beirut  trades  his net  per trade  win%   days +/total   1st half  2nd half")
    for h in range(24):
        v = by.get(h, [])
        if not v: continue
        dp = sum(1 for d in days if day.get((h, d), 0) > 0); dn = sum(1 for d in days if (h, d) in day)
        print(f"   {h:02d}h   {(h + 2) % 24:02d}h    {len(v):5d}   {sum(v):+7.2f}   {sum(v) / len(v):+.4f}   {100 * sum(x > 0 for x in v) / len(v):3.0f}%   {dp:2d}/{dn:2d}         {h1.get(h, 0):+6.2f}    {h2.get(h, 0):+6.2f}")
    hs = [h for h in range(24) if h in h1 and h in h2]
    a = {h: h1[h] for h in hs}; b = {h: h2[h] for h in hs}
    print(f"   do the same hours lead in both halves? rank correlation of the hours' net, 1st vs 2nd half: {rank_corr(a, b):+.2f} (1 = same order, 0 = unrelated)")
    top1 = sorted(hs, key=lambda h: -a[h])[:6]
    print(f"   the 6 best hours of the 1st half {[f'{h:02d}' for h in top1]}: 2nd half {sum(b[h] for h in top1):+.2f} SOL, against {sum(b.values()) * 6 / len(hs):+.2f} for 6 average hours")
