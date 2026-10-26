# Do the wallets that make the most make good copies? On every SOL-quoted tape (Oct 25 18:00 -> Oct 26 17:40), per wallet
# with 15+ launch buys: its own result inside the 13-slot tape (SOL out - SOL in on its trades of those coins, pump fees
# in, tips not; what it still holds at create + 12 marked at the curve) against what a copy makes (our landing, +5%,
# 0.5 SOL, the best of create + 5 / 9 / 12, sold whatever it does).
import json, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/oos/discover2.py").read()
exec(src[:src.index("KNOWN = ")])
own = collections.defaultdict(float); held = collections.defaultdict(lambda: collections.defaultdict(int))
for m, co in COINS.items():
    seq = co["seq"]; end = state_end(seq, co["cs"] + 12)
    for x in seq:
        w, side, sol, tok, fee = x[4], x[5], x[6], x[7], x[8]
        if side == 1: own[(w, m)] -= sol + fee; held[w][m] += tok
        else: own[(w, m)] += sol - fee; held[w][m] -= tok
    for w in list(held):
        q = held[w].get(m, 0)
        if q > 0 and end:
            v, t = end; own[(w, m)] += v * q / (t + q) * 0.9875
            held[w][m] = 0
res = []
for w, ts in trades.items():
    ts = [t for t in ts if bt2(t["x"][0]) >= T0]
    if len(ts) < 15: continue
    o = sum(own.get((w, t["m"]), 0) for t in ts) / 1e9
    cv = {h: sum(copy_val(t["m"], t["x"], None, h) or 0 for t in ts) for h in (5, 9, 12)}
    h = max(cv, key=cv.get)
    res.append((o, w, len(ts), cv[h], h))
res.sort(reverse=True)
print("the 15 wallets that made the most on their launch buys (inside the tape), and what copying them makes:")
for o, w, n, c, h in res[:15]:
    print(f"   {w[:8]}  launch buys {n:4d}  its own {o:+7.2f} SOL   a copy {c:+6.2f} (best exit create+{h})")
import statistics
top = res[:30]
print(f"top 30 by own result: own {sum(r[0] for r in top):+.1f}, copies {sum(r[3] for r in top):+.1f}; copies positive {sum(1 for r in top if r[3] > 0)} of 30")
