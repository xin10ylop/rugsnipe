# His low-tip, big-size buys: does the tip key (>= 4M) miss copies worth having? Same copy as the live bot's backtest
# (recent.py: his buys on the signer in the last 24 h >= 2, bound 5%, our landing distances, 0.5 SOL, exit H-1 on
# listed signers, else his modal first-sell offset - 1, else +9), keyed on his tip, his size, or either.
import json, sys, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/dec/copy/recent.py").read().split("T = [r for r in rows if r[\"tip\"] >= 4e6]")[0]
exec(src)
for r in rows: r["v"] = v_of(r)
def hot(r): return sum(1 for t in r["prev_t"] if t >= his_buy[r["m"]]["bt"] - 24 * 3600) >= 2
R = [r for r in rows if r["v"] is not None and hot(r)]
def show(lab, f):
    sel = [r for r in R if f(r)]
    byd = collections.defaultdict(float)
    for r in sel: byd[r["day"]] += r["v"]
    pos = sum(1 for d in DAYS if byd[d] > 0)
    print(f"   {lab:44} n {len(sel):4} total {sum(byd.values()):+6.2f}  per copy {sum(byd.values()) / max(len(sel), 1):+.4f}  days+ {pos}/5  " + " ".join(f"{byd[d]:+.2f}" for d in DAYS))
print("his launch buys on a signer he bought >= 2 times in 24 h; copy value by day " + " ".join(DAYS))
show("tip >= 4M (the live key)", lambda r: r["tip"] >= 4e6)
for s in (1.0, 1.5, 2.0, 2.5):
    show(f"tip < 4M and size >= {s} SOL (what it misses)", lambda r, s=s: r["tip"] < 4e6 and r["sol"] >= s)
for s in (1.5, 2.0, 2.5):
    show(f"tip >= 4M or size >= {s} SOL", lambda r, s=s: r["tip"] >= 4e6 or r["sol"] >= s)
for s in (0.7, 1.0, 1.5, 2.0):
    show(f"size >= {s} SOL (size key)", lambda r, s=s: r["sol"] >= s)
show("every buy of his", lambda r: True)
print()
print("his own result on those coins (his net, SOL), tip < 4M vs >= 4M, by his size:")
for lo, hi in ((0, 1), (1, 2), (2, 99)):
    for tl in (False, True):
        sel = [r for r in R if lo <= r["sol"] < hi and (r["tip"] >= 4e6) == tl]
        print(f"   size {lo}-{hi} SOL tip {'>=' if tl else '< '} 4M: n {len(sel):4}  his net {sum(r['his'] for r in sel):+7.2f}  our copy {sum(r['v'] for r in sel):+6.2f}")
