# The live signer count (entry.copy_signer_min_buys) starts empty at each start of rugsnipe-hot, and the operator
# refresh restarts it every 4 h (03,07,11,15,19,23:50 UTC). The tip-keyed copy (tip >= 4M, bound 5%, our landing
# distances, 0.5 SOL) with his buys on the signer counted only since the last restart, against the count over 24 h.
import json, sys, collections, bisect, datetime
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/dec/copy/recent.py").read().split("def show(")[0]
exec(src)
def last_restart(bt, hours=(3, 7, 11, 15, 19, 23)):
    d = datetime.datetime.fromtimestamp(bt, datetime.timezone.utc)
    best = None
    for back in (0, 1):
        day0 = (d - datetime.timedelta(days=back)).replace(hour=0, minute=0, second=0, microsecond=0)
        for h in hours:
            t = (day0 + datetime.timedelta(hours=h, minutes=50)).timestamp()
            if t <= bt and (best is None or t > best): best = t
    return best
def show(lab, f):
    sel = [r for r in T if r["v"] is not None and f(r)]
    byd = collections.defaultdict(float)
    for r in sel: byd[r["day"]] += r["v"]
    pos = sum(1 for d in DAYS if byd[d] > 0)
    print(f"   {lab:58} n {len(sel):4} total {sum(byd.values()):+6.2f}  days+ {pos}/5  " + " ".join(f"{byd[d]:+.2f}" for d in DAYS))
    return sel
def cnt(r, W, since=None):
    bt = his_buy[r["m"]]["bt"]; lo = bt - W * 3600
    if since is not None: lo = max(lo, since)
    return sum(1 for t in r["prev_t"] if t >= lo)
print("his tip >= 4M, bound 5%; by day " + " ".join(DAYS))
a = show(">= 2 in 24 h, counted over 24 h (no restart)", lambda r: cnt(r, 24) >= 2)
b = show(">= 2 in 24 h, the count lost at each 4-hourly restart", lambda r: cnt(r, 24, last_restart(his_buy[r["m"]]["bt"])) >= 2)
lost = [r for r in a if r not in b]
print(f"   lost to the restarts: {len(lost)} copies, {sum(r['v'] for r in lost):+.2f} SOL")
show(">= 1 in 24 h, the count lost at each restart", lambda r: cnt(r, 24, last_restart(his_buy[r["m"]]["bt"])) >= 1)
show("every signer (no count)", lambda r: True)
