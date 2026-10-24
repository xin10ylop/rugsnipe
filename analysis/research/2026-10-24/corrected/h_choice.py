# What decides his sell slot H (create + 6 or create + 10)? Month trades: H = slot_off + hold. Features at buy time.
import json, glob, collections, bisect, os
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
T = sorted([t for t in json.load(open(SP + "/month/AHwJ_T.json")) if t["stake"] >= 0.1 and t["hold"] is not None], key=lambda t: t["bt"])
for t in T: t["H"] = t["slot_off"] + t["hold"]
print("month H distribution:", dict(sorted(collections.Counter(t["H"] for t in T).items())))
byday = collections.defaultdict(collections.Counter)
for t in T: byday[t["day"]][t["H"]] += 1
print("H by day (share of 6 / 10 / other):")
for d, c in sorted(byday.items()):
    n = sum(c.values()); print(f"   {d}: n {n:4d}  " + "  ".join(f"H{h}:{100*v/n:3.0f}%" for h, v in sorted(c.items()) if v / n >= 0.05))
R = [t for t in T if t["day"] >= "10-16" and t["H"] in (6, 10)]
print(f"\nsince Oct 16, H in (6, 10): {len(R)} of {sum(1 for t in T if t['day'] >= '10-16')}")
def rate(name, f):
    g = collections.defaultdict(lambda: [0, 0])
    for t in R:
        k = f(t); g[k][0] += 1; g[k][1] += t["H"] == 10
    print(f"P(H=10) by {name}: " + "  ".join(f"{k}: {100*b/a:3.0f}% (n {a})" for k, (a, b) in sorted(g.items(), key=lambda kv: str(kv[0])) if a >= 20))
rate("stake", lambda t: "<0.7" if t["stake"] < 0.7 else "0.7-1.5" if t["stake"] < 1.5 else ">=1.5")
rate("tip", lambda t: "<=1.5M" if (t["tip"] or 0) <= 1_500_000 else "1.5-3M" if t["tip"] <= 3_000_000 else ">3M")
rate("entry slot", lambda t: t["slot_off"])
rate("post-bundle vSOL", lambda t: "<34" if (t["v_before"] or 0) < 34 else "34-37" if t["v_before"] < 37 else "37-40" if t["v_before"] < 40 else ">=40")
rate("hour", lambda t: (t["bt"] % 86400) // 3600 // 4 * 4)
# persistence per creator: previous H with this creator
last = {}; agree = collections.Counter()
for t in R:
    c = t["creator"]
    if c in last: agree["n"] += 1; agree["eq"] += last[c] == t["H"]
    last[c] = t["H"]
print(f"H == this creator's previous H: {100*agree['eq']/agree['n']:.0f}% (n {agree['n']})")
# his P&L with the creator before the trade
pnl = collections.defaultdict(float); nprev = collections.Counter()
g = collections.defaultdict(lambda: [0, 0])
for t in R:
    c = t["creator"]; p = pnl[c]; k = "new" if nprev[c] == 0 else "<0" if p < 0 else "0-0.5" if p < 0.5 else "0.5-2" if p < 2 else ">2"
    g[k][0] += 1; g[k][1] += t["H"] == 10
    pnl[c] += t["pnl"]; nprev[c] += 1
print("P(H=10) by his P&L with the creator before: " + "  ".join(f"{k}: {100*b/a:3.0f}% (n {a})" for k, (a, b) in g.items()))
# his previous result on this creator (last trade won or lost) and previous H
lastr = {}; g = collections.defaultdict(lambda: [0, 0])
for t in R:
    c = t["creator"]
    if c in lastr:
        ph, pr = lastr[c]; k = f"prev H{ph} {'won' if pr > 0 else 'lost'}"; g[k][0] += 1; g[k][1] += t["H"] == 10
    lastr[c] = (t["H"], t["pnl"])
print("P(H=10) by this creator's previous trade: " + "  ".join(f"{k}: {100*b/a:3.0f}% (n {a})" for k, (a, b) in sorted(g.items())))
