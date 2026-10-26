# The second search, widened and split in time: every wallet's launch buys on all SOL-quoted tapes fetched so far
# (oos/coin), copied as the bot copies (our landing distances, +5%, 0.5 SOL, sold at create + h whatever they do).
# Picked on the first half only (Oct 25 18:00 -> Oct 26 06:00: 12+ copies, its best hold there positive), scored on the
# second (06:00 -> the pull) at that hold. Not yet complete for any wallet (the tapes are the coins the candidates bought).
import json, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/oos/discover2.py").read()
exec(src[:src.index("KNOWN = ")])
MID = 1792994400; HOLDS = (3, 5, 7, 9, 12)
DONE = {l.strip()[:8] for f in ("finalists1.txt", "cands2.txt") for l in open(f"{SP}/oos/{f}") if l.strip()} | {"AHwJSnFN", "4g49xGbg"}
res = []
for w, ts in trades.items():
    r1, r2 = [], []
    for t in ts:
        bt = bt2(t["x"][0])
        if bt < T0: continue
        (r1 if bt < MID else r2).append(t)
    if len(r1) < 12 or len(r2) < 8: continue
    v1 = {h: sum(copy_val(t["m"], t["x"], None, h) or 0 for t in r1) for h in HOLDS}
    hp = max(v1, key=v1.get)
    if v1[hp] <= 0: continue
    v2 = sum(copy_val(t["m"], t["x"], None, hp) or 0 for t in r2)
    res.append((v2, w, hp, len(r1), v1[hp], len(r2)))
print(f"SOL tapes {len(COINS)}; wallets positive on the first half (12+ copies there, 8+ after): {len(res)}")
print("scored on the second half at the hold picked on the first, best first:")
for v2, w, hp, n1, a, n2 in sorted(res, reverse=True):
    tag = "(done)" if w[:8] in DONE else ""
    print(f"   {w[:8]} {tag:6} create+{hp:<2} 1st half {a:+6.2f} ({n1:3d})  2nd half {v2:+6.2f} ({n2:3d})  {v2 / n2:+.4f} a copy")
json.dump([dict(w=w, hold=hp, n1=n1, v1=a, n2=n2, v2=v2) for v2, w, hp, n1, a, n2 in res], open(SP + "/oos/discover3.json", "w"))
