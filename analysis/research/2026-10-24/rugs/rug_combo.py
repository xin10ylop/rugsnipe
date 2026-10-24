# Combinations of pre-buy rug signals, chosen on Oct 23 00-12 (train), scored on Oct 23 12-24 and Oct 24 00-12 (test).
# Signals: dumper list (3 slots / 50%), the tweet feed's copy call (0.4 s lag, 30 min, all accounts), insiders' share of
# the supply, dormant deployer, tool. Live "exit" policy (copies sold at create+3), net of 1.2%, 0.5 SOL stakes.
import json, itertools, collections, sys
sys.path.insert(0, "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/pm")
exec(open("/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/pm/rug_rules.py").read().split("def V(x, pol):")[0])
exec(open("/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/pm/tweetfeed.py").read().split("print(f\"tweets with text")[0])
used = set(); call = {}
for r in feats:
    c = cr.get(r["mint"])
    if not c: continue
    ms = matches(c, c["bt"], 0.4, 1800)
    if ms: call[r["mint"]] = "copy" if all(s in used for s in ms) else "first"
    for s in ms: used.add(s)
def V(x):
    if not x["first_fresh"]:
        v = x["vC"].get(str(max(3, x["d"] + 1)))
        if v is not None: return v
    return x["vC"].get("10")
rows = [x for x in U.values() if x["vC"].get("10") is not None and x["mint"] in flags and x["mint"] in RR]
SIG = {
    "dumper": lambda x: flags[x["mint"]][(3, 0.5)],
    "feed copy": lambda x: call.get(x["mint"]) == "copy",
    "not feed first": lambda x: call.get(x["mint"]) != "first",
    "insiders >= 20%": lambda x: RR[x["mint"]]["ins_share"] >= 0.20,
    "insiders >= 25%": lambda x: RR[x["mint"]]["ins_share"] >= 0.25,
    "dormant": lambda x: flags[x["mint"]]["dormant"],
    "rapidlaunch": lambda x: "rapidlaunch" in x["host"],
    "his P&L there < 0.5": lambda x: x["h_pnl"] < 0.5,
}
train = lambda x: x["day"] == 23 and x["hour"] < 12
tests = [("Oct 23 12-24", lambda x: x["day"] == 23 and x["hour"] >= 12), ("Oct 24 00-12", lambda x: x["day"] == 24)]
def score(sel, f):
    g = [x for x in rows if sel(x)]; sk = [x for x in g if f(x)]
    vs = [V(x) - 0.012 for x in sk]; big_all = sum(V(x) <= -0.3 for x in g)
    return len(sk), (sum(vs) / len(vs) if vs else 0), sum(v <= -0.3 for v in vs), big_all, 0.5 * -sum(vs)
cands = []
names = list(SIG)
for k in (1, 2, 3):
    for combo in itertools.combinations(names, k):
        f = (lambda cs: lambda x: all(SIG[c](x) for c in cs))(combo)
        n, m, bs, ba, chg = score(train, f)
        if n >= 15 and chg > 0 and bs >= 2: cands.append((chg, combo, f))
cands.sort(key=lambda c: -c[0])
print("rules that paid on the training half (Oct 23 00-12), best first; then their score on the two test periods:")
for chg, combo, f in cands[:12]:
    n, m, bs, ba, _ = score(train, f)
    line = f"   {' AND '.join(combo):60s} train: skip {n:3d}, mean {100*m:+6.2f}%, big {bs}/{ba}, {chg:+.3f} SOL |"
    for lab, sel in tests:
        n2, m2, bs2, ba2, chg2 = score(sel, f)
        line += f" {lab}: skip {n2:3d} mean {100*m2:+6.2f}% big {bs2}/{ba2} {chg2:+.3f} SOL |"
    print(line)
