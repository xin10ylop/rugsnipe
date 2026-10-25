# The ceiling: how often do we SEE him attempt a coin he surely wants? (rule coins of deployers he trades a lot)
import json, collections, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/pm")
import bt
rows = bt.build()
print("rule-universe coins with tapes", len(rows))
def tab(lab, g, f):
    c = collections.defaultdict(lambda: [0, 0])
    for r in g: k = f(r); c[k][0] += 1; c[k][1] += r["he"]
    print(lab)
    for k, (n, h) in sorted(c.items(), key=lambda kv: str(kv[0])): print(f"   {str(k):30s} n {n:5d} his {h:4d} {100*h/n:5.1f}%")
tab("his trades on the deployer in the last 7 days", rows, lambda r: min(r["h_n"], 10) if r["h_n"] < 5 else (">=5" if r["h_n"] < 10 else ">=10"))
tab("pb_sol <= 40, h_n >= 5", [r for r in rows if r["h_n"] >= 5], lambda r: (r["pb_sol"] or 99) <= 40)
