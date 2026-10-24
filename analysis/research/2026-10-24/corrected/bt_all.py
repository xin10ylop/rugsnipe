# All the corrected backtests in one pass over the complete tapes (bt.py): A the rule (no check / exit / on) by period,
# B holds (fixed create+H, his per-deployer setting, combined with the exit-mode copy cut), C tiers and stakes,
# D how he uses the tweet and what each tweet fact is worth. Cost = our measured round trip (1.2%) and the cut one (0.6%).
import sys, json, collections, math
sys.path.insert(0, "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/pm")
from bt import *
R = build()
cap = lambda x: x["pb_sol"] is None or x["pb_sol"] <= 40
R = [x for x in R if x["fo"] and x["host"] in TOOLS and TEMPLATE(x["dev"])]
U = [x for x in R if cap(x)]
json.dump(U, open(SP + "/pm/bt_rows.json", "w"))
COST, COST_CUT = 0.012, 0.006
PER = [("Oct 23 00-12", 23, 0, 12), ("Oct 23 12-24", 23, 12, 24), ("Oct 24 00-12", 24, 0, 12)]
def period(lo_hi): d, lo, hi = lo_hi; return [x for x in U if x["day"] == d and lo <= x["hour"] < hi]
def V(x, h): return x["vC"].get(h)
def exit_v(x, hold=10, cut=3):
    return V(x, hold) if x["first_fresh"] else (V(x, max(cut, x["d"] + 1)) if V(x, max(cut, x["d"] + 1)) is not None else V(x, hold))
print(f"rule coins with a first outside buy and a complete tape: {len(U)} (cap excluded {len(R) - len(U)})")
print("\n== A. the rule, corrected (real fees, complete tapes); sell at create+10 like the bot; copies cut at create+3 in exit")
print(f"   {'period':14s} {'coins/h':>7s}  {'no check':38s} {'exit (live mode)':38s} {'on (tweet decides)':38s}")
for name, *p in PER:
    G = period(p); hrs = p[2] - p[1]
    print(f"   {name:14s} {len(G)/hrs:7.1f}  {fmt(stats([V(x,10) for x in G], COST)):38s} {fmt(stats([exit_v(x) for x in G], COST)):38s} {fmt(stats([V(x,10) for x in G if x['first_fresh']], COST)):38s}")
G = U; print(f"   {'all':14s} {'':7s}  {fmt(stats([V(x,10) for x in G], COST)):38s} {fmt(stats([exit_v(x) for x in G], COST)):38s} {fmt(stats([V(x,10) for x in G if x['first_fresh']], COST)):38s}")
print("   by 4-hour band, exit mode:", "  ".join(f"Oct{d} {lo:02d}-{lo+4:02d}: {fmt(stats([exit_v(x) for x in U if x['day']==d and lo<=x['hour']<lo+4], COST))}" for d in (23, 24) for lo in range(0, 24, 4) if any(x['day']==d and lo<=x['hour']<lo+4 for x in U)))
print("\n== B. holds (sell at create + H), net of 1.2%; per period")
for name, *p in PER + [("all", None, None, None)]:
    G = period(p) if p[0] else U
    line = "  ".join(f"H{h}:{100*(stats([V(x,h) for x in G], COST) or {'m':float('nan')})['m']:+.1f}" for h in (2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12))
    print(f"   {name:14s} fixed H: {line}")
for cls, f in (("first coins", lambda x: x["first_fresh"]), ("not first", lambda x: not x["first_fresh"])):
    G = [x for x in U if f(x)]
    print(f"   {cls:14s} fixed H: " + "  ".join(f"H{h}:{100*(stats([V(x,h) for x in G], COST) or {'m':float('nan')})['m']:+.1f}" for h in (2, 3, 4, 5, 6, 8, 10, 12)))
def his_setting(x, default):
    h = x["h_lastH"]
    return h if h in (6, 10) else default
print("   his per-deployer setting (his last create+H on the deployer before the coin; none -> default):")
for name, *p in PER + [("all", None, None, None)]:
    G = period(p) if p[0] else U
    out = []
    for lab, pol in (("fixed 10", lambda x: V(x, 10)), ("fixed 6", lambda x: V(x, 6)),
                     ("his setting, else 10", lambda x: V(x, his_setting(x, 10))), ("his setting, else 6", lambda x: V(x, his_setting(x, 6))),
                     ("exit + fixed 10", lambda x: exit_v(x, 10)), ("exit + his setting, else 10", lambda x: exit_v(x, his_setting(x, 10))),
                     ("exit + his setting, else 6", lambda x: exit_v(x, his_setting(x, 6)))):
        s = stats([pol(x) for x in G], COST); out.append(f"{lab}: {100*s['m']:+.2f}%" if s else f"{lab}: -")
    print(f"     {name:14s} " + " | ".join(out))
known = [x for x in U if x["h_lastH"] in (6, 10)]
print(f"   coins whose deployer has his setting: {len(known)} of {len(U)} ({100*len(known)/len(U):.0f}%)")
for hs in (6, 10):
    G = [x for x in known if x["h_lastH"] == hs]
    print(f"     his setting {hs:2d}: n {len(G):4d}  value at H6 {fmt(stats([V(x,6) for x in G], COST))}   at H10 {fmt(stats([V(x,10) for x in G], COST))}")
print("\n== C. by his tier on the deployer (his median stake there before the coin: 0.4 x 2^tier, jitter x1.5^a)")
def tier_of(st):
    if st is None: return None
    best = min(((abs(math.log(st / (0.4 * 1.5 ** a * 2 ** b))), b) for a in range(3) for b in range(3)))
    return best[1]
for x in U: x["tier"] = tier_of(x["h_stake"])
for t in (None, 0, 1, 2):
    G = [x for x in U if x["tier"] == t]
    print(f"   tier {str(t):4s}: n {len(G):4d}  exit+setting {fmt(stats([exit_v(x, his_setting(x, 10)) for x in G], COST))}  "
          f"by period " + "  ".join(f"{n}: {fmt(stats([exit_v(x, his_setting(x, 10)) for x in G if x['day']==p[0] and p[1]<=x['hour']<p[2]], COST))}" for n, *p in PER))
for lab, lo, hi in (("P&L <0", -99, 0), ("0-0.5", 0, 0.5), ("0.5-2", 0.5, 2), (">2", 2, 99)):
    G = [x for x in U if x["h_n"] > 0 and lo <= x["h_pnl"] < hi]
    print(f"   his P&L on the deployer {lab:7s}: {fmt(stats([exit_v(x, his_setting(x, 10)) for x in G], COST))}")
print("\n== D. the tweet: his attempt rate and the value by coin-to-tweet delay (first coins / copies)")
def dl(x):
    d = x["delay"]
    return "none" if d is None else "<2s" if d < 2 else "2-5s" if d < 5 else "5-10s" if d < 10 else "10-60s" if d < 60 else "1-60min" if d < 3600 else ">1h"
for cls, f in (("first coins", lambda x: x["first_fresh"]), ("copies", lambda x: x["sid"] and x["rank"] > 1)):
    G = [x for x in U if f(x)]; by = collections.defaultdict(list)
    for x in G: by[dl(x)].append(x)
    print(f"   {cls}:")
    for k in ("<2s", "2-5s", "5-10s", "10-60s", "1-60min", ">1h", "none"):
        g = by.get(k, [])
        if g: print(f"     delay {k:8s} n {len(g):4d}  he attempted {100*sum(x['he'] for x in g)/len(g):3.0f}%  hold-10 value {fmt(stats([V(x,10) for x in g], COST))}  cut at +3 {fmt(stats([V(x,3) for x in g], COST))}")
