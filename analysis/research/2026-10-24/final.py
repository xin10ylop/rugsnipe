import json, statistics as St, math, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
FEE = 0.0125; W = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
T = json.load(open(SP + "/today/trades.json")); HC = json.load(open(SP + "/today/hc.json"))
def ci(v):
    if not v: return "n=0"
    if len(v) < 2: return f"n=  1 {100*v[0]:+.1f}%"
    m = St.mean(v); h = 1.96 * St.pstdev(v) / math.sqrt(len(v))
    return f"n={len(v):3d} mean {100*m:+6.2f}% [{100*(m-h):+6.2f},{100*(m+h):+6.2f}] med {100*St.median(v):+6.2f}%"
print("== result by the post-bundle virtual SOL reserve (entry.max_post_bundle_sol), both wallets")
for lo, hi in ((0, 36), (36, 38), (38, 40), (40, 42), (42, 45), (45, 50), (50, 999)):
    for who in ("us", "him"):
        G = [r for r in T if r[who] and r[who]["closed"] and r["post_bundle"] and lo <= r["post_bundle"][0] / 1e9 < hi]
        if G: print(f"   {lo:3d}-{hi:<3d} {who:3s} {ci([r[who]['net'] / r[who]['cost'] for r in G])} SOL {sum(r[who]['net'] for r in G):+.4f}  rugs {sum(1 for r in G if r[who]['net'] / r[who]['cost'] < -0.2)}")
# our day, rebuilt step by step
def val(st, q): vs, vt = st; return (vs - vs * vt / (vt + q)) * (1 - FEE) / 1e9
def sells_value(r):
    tot = q = 0
    for s, i, u, b, sol, tok, vs, vt, ins in r["trades"]:
        if u == W and not b: tot += val((vs + sol, vt - tok), tok); q += tok
    return tot, q
def early_exit(r):  # sell when our slot ends without his buy; lands at the end of the next slot (pessimistic)
    es = {int(k): v for k, v in r["end_state"].items()}; bs = r["us"]["buy_slot"]
    ks = [k for k in es if k <= bs + 1]
    actual, q = sells_value(r)
    return r["us"]["net"] + (val(es[max(ks)], q) - actual) if (ks and q) else r["us"]["net"]
U = [r for r in T if r["us"] and r["us"]["closed"]]
LU = json.load(open(SP + "/today/legs_us.json"))
fail = -sum(x["delta"] for x in LU if x["kind"] == "buy" and x["err"]) / 1e9
nfail = sum(1 for x in LU if x["kind"] == "buy" and x["err"])
print(f"\n== our day (00:00-12:29 UTC), chain-true; failed buy copies {nfail} cost {fail:.4f} SOL")
step = lambda lab, v: print(f"   {lab:72s} {v:+.4f} SOL")
step("as traded (75 trades) minus failed copies", sum(r["us"]["net"] for r in U) - fail)
cap = 40.0
K = [r for r in U if not (r["post_bundle"] and r["post_bundle"][0] / 1e9 > cap)]
step(f"1. skip post-bundle reserve > {cap:.0f} virtual SOL ({len(U) - len(K)} trades skipped)", sum(r["us"]["net"] for r in K) - fail)
fired = lambda r: HC[r["mint"]]["hc"] == "failed behind us, same slot"
e = sum(r["us"]["net"] if fired(r) else early_exit(r) for r in K)
step(f"2. + sell at once when his buy is not in our slot ({sum(1 for r in K if not fired(r))} trades)", e - fail)
step(f"3. + failed copies at his cost (10,795 lamports each instead of {1e9 * fail / nfail:,.0f})", e - nfail * 10795e-9)
saved = sum(r["us"]["cost"] for r in K) * (0.0177 - 0.0075)
step(f"4. + round trips at his cost (0.75% of stake instead of 1.77%)", e - nfail * 10795e-9 + saved)
