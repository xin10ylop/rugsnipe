# Stake schemes on the corrected rule rows (bt_rows.json): SOL per coin = stake x value - fixed cost per landed trade
# (tips and fees in lamports, independent of the stake; the pump fee is already in the value). Tier = his stake level
# on the deployer before the coin (0.4 SOL x 2^tier); none = no history (the bot's tier 0).
import json, math, random, collections, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
U = json.load(open(SP + "/pm/bt_rows.json"))
HOLD = int(sys.argv[1]) if len(sys.argv) > 1 else 10
FIXED = {"now (buy 3M, sell 2M + 1M)": 0.0062, "cut sells (buy 3M, sell 0.5M + 1M)": 0.0047}
def tier_of(st):
    if st is None: return 0
    return min(((abs(math.log(st / (0.4 * 1.5 ** a * 2 ** b))), b) for a in range(3) for b in range(3)))[1]
for x in U: x["tier"] = tier_of(x["h_stake"]); x["v"] = x["vC"].get(str(HOLD), x["vC"].get(HOLD))
U = [x for x in U if x["v"] is not None]
hours = {(23, 0, 24): 24, (24, 0, 12): 12}
print(f"hold create+{HOLD}; {len(U)} coins; by tier:", dict(sorted(collections.Counter(x["tier"] for x in U).items())))
for t in (0, 1, 2):
    G = [x["v"] for x in U if x["tier"] == t]
    if G: print(f"   tier {t}: n {len(G):4d}  mean value {100*sum(G)/len(G):+6.2f}% (before tips/fees)  win {100*sum(v>0 for v in G)/len(G):3.0f}%")
SCHEMES = {
    "flat 0.5 (now)": (0.5, 0.5, 0.5),
    "his ladder scaled, max 0.5 (0.125/0.25/0.5)": (0.125, 0.25, 0.5),
    "gentler ladder (0.25/0.375/0.5)": (0.25, 0.375, 0.5),
    "skip tier 0 (0/0.5/0.5)": (0.0, 0.5, 0.5),
}
LAND = 0.5   # share of rule coins we win (live today: about half)
per_day = len(U) / 36 * 24 * LAND
for cost_name, c in FIXED.items():
    print(f"\n== costs {cost_name}: {c*1e3:.1f} mSOL per landed trade; about {per_day:.0f} trades a day at a {int(LAND*100)}% land rate")
    best = {}
    for t in (0, 1, 2):
        G = [x["v"] for x in U if x["tier"] == t]
        if not G: continue
        cands = [0.0, 0.1, 0.125, 0.15, 0.2, 0.25, 0.3, 0.375, 0.4, 0.5]
        best[t] = max(cands, key=lambda s: sum(s * v - (c if s > 0 else 0) for v in G))
    SCHEMES2 = dict(SCHEMES); SCHEMES2[f"best per tier ({'/'.join(str(best.get(t, 0)) for t in (0, 1, 2))})"] = tuple(best.get(t, 0) for t in (0, 1, 2))
    for name, st in SCHEMES2.items():
        pnl = [st[x["tier"]] * x["v"] - c for x in U if st[x["tier"]] > 0]
        n = len(pnl); mean = sum(pnl) / max(n, 1)
        sd = (sum((p - mean) ** 2 for p in pnl) / max(n - 1, 1)) ** 0.5
        k = n / len(U) * per_day          # trades a day under this scheme
        day = mean * k; day_sd = sd * math.sqrt(k)
        random.seed(3); bs = sorted(sum(random.choice(pnl) for _ in pnl) / n for _ in range(500))
        print(f"   {name:46s}: {k:5.0f} trades/day  {mean*1e3:+6.2f} mSOL/trade [{bs[12]*1e3:+.2f},{bs[487]*1e3:+.2f}]  "
              f"{day:+6.3f} SOL/day (1-sd day {day_sd:.3f})  worst trade {min(pnl):+.3f} SOL")
