# Q2 rules, tested on every closed trade today (ours and his): (a) the bundle's markup at our entry, (b) skip a dev after
# one of its launches had an atomic insider dump, (c) exit on the first insider sell. Each with what it costs on winners.
import json, statistics as St, math, collections, time
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"; A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"
FEE = 0.0125
T = sorted(json.load(open(SP + "/today/trades.json")), key=lambda r: r["cs"])
def ci(v):
    if not v: return "n=0"
    if len(v) < 2: return f"n=1 {100*v[0]:+.1f}%"
    m = St.mean(v); h = 1.96 * St.pstdev(v) / math.sqrt(len(v))
    return f"n={len(v):3d} mean {100*m:+6.2f}% [{100*(m-h):+6.2f},{100*(m+h):+6.2f}] med {100*St.median(v):+6.2f}%"
def markup(r, who):
    """our fill price over the curve's start price (the dev bundle's push)"""
    tr = r["trades"]
    if not tr: return None
    s, i, u, b, sol, tok, vs, vt, isin = tr[0]
    p_start = (vs - sol) / (vt + tok) if b else vs / vt          # the state before the first trade
    f = r[who].get("fill_state")
    if not f: return None
    return (f[0] / f[1]) / p_start - 1
def dumped(r, within=12):
    """atomic insider dump: insiders sell >= 70% of what they bought, within `within` slots of the create"""
    bought = sum(sol for s, i, u, b, sol, tok, vs, vt, isin in r["trades"] if isin and b)
    sold = sum(sol for s, i, u, b, sol, tok, vs, vt, isin in r["trades"] if isin and not b and s <= r["cs"] + within)
    return bought > 0 and sold >= 0.7 * bought
rows = []
for r in T:
    for who in ("us", "him"):
        t = r[who]
        if t and t["closed"]: rows.append(dict(r=r, who=who, v=t["net"] / t["cost"], sol=t["net"], mk=markup(r, who), t=t["bt"]))
print(f"closed trades today, both wallets: {len(rows)}; launches with an atomic insider dump within 12 slots: {sum(1 for r in T if dumped(r))} of {len(T)}")
print("\n(a) result by the bundle's markup at the entry (fill price / curve start price - 1)")
for lo, hi in ((-1, 0.2), (0.2, 0.4), (0.4, 0.6), (0.6, 0.8), (0.8, 9)):
    G = [x for x in rows if x["mk"] is not None and lo <= x["mk"] < hi]
    if G: print(f"   markup {lo:+.0%}..{hi:+.0%}: {ci([x['v'] for x in G])} SOL {sum(x['sol'] for x in G):+.4f}  (ours {sum(1 for x in G if x['who'] == 'us')}, his {sum(1 for x in G if x['who'] == 'him')}, rugs<-20% {sum(1 for x in G if x['v'] < -0.2)})")
# (b) skip a dev for H hours after one of its launches dumped atomically (known by create + 12 slots, ~5 s)
print("\n(b) skip a dev after one of its launches had an atomic insider dump (seen 5 s after that create)")
for H in (1, 3, 24):
    skipped = [x for x in rows if any(dumped(p) and p["cs"] < x["r"]["cs"] - 12 and
               ((x["r"]["us"] or x["r"]["him"])["bt"] - (p["us"] or p["him"])["bt"]) <= H * 3600 and p["dev"] == x["r"]["dev"] for p in T)]
    print(f"   {H:2d} h: skips {len(skipped):3d} trades: {ci([x['v'] for x in skipped])} SOL {sum(x['sol'] for x in skipped):+.4f}  rugs among them {sum(1 for x in skipped if x['v'] < -0.2)}")

print("\n== per wallet: what each rule removes (SOL of the removed trades; negative = the rule saves that much)")
HC = json.load(open(SP + "/today/hc.json"))
def dev_skip(x, H=3):
    return any(dumped(p) and p["cs"] < x["r"]["cs"] - 12 and p["dev"] == x["r"]["dev"] and
               ((x["r"]["us"] or x["r"]["him"])["bt"] - (p["us"] or p["him"])["bt"]) <= H * 3600 for p in T)
for who in ("us", "him"):
    R = [x for x in rows if x["who"] == who]
    cap = [x for x in R if x["mk"] is not None and x["mk"] >= 0.8]
    ds = [x for x in R if dev_skip(x)]
    both = [x for x in R if (x["mk"] is not None and x["mk"] >= 0.8) or dev_skip(x)]
    print(f"  {who:3s} day {sum(x['sol'] for x in R):+.4f} SOL on {len(R)} trades | markup >= 80%: {len(cap)} trades {sum(x['sol'] for x in cap):+.4f} | "
          f"dev skip 3 h: {len(ds)} trades {sum(x['sol'] for x in ds):+.4f} | either: {len(both)} trades {sum(x['sol'] for x in both):+.4f}")
# the combined day for us: (1) only trades he fires on (same-slot detection, sell at once otherwise), (2) the rug rules,
# (3) his cost level instead of ours (failed copies at his 10,795 lamports, round trip at his 0.75% of stake)
U = [x for x in rows if x["who"] == "us"]
print("\n== our day under each change (chain-true trades, counterfactual exits from the tapes)")
base = sum(x["sol"] for x in U)
print(f"  as traded:                                      {base:+.4f} SOL on {len(U)} trades")
keep = [x for x in U if not ((x["mk"] is not None and x["mk"] >= 0.8) or dev_skip(x))]
print(f"  + rug rules (skip markup >= 80%, dev skip 3 h):   {sum(x['sol'] for x in keep):+.4f} SOL on {len(keep)} trades")
fired = [x for x in keep if HC[x["r"]["mint"]]["hc"] == "failed behind us, same slot"]
print(f"  + keep only the trades he fired on in our slot:   {sum(x['sol'] for x in fired):+.4f} SOL on {len(fired)} trades "
      f"(the {len(keep) - len(fired)} others, sold at once, about {sum(x['sol'] for x in keep if x not in fired) * 0.8:+.4f} by Q1b)")
cost_gap = sum(x["r"]["us"]["cost"] for x in fired) * (0.0177 - 0.0075)
print(f"  + his cost level on those trades (+{100*(0.0177-0.0075):.2f}% of stake each): {sum(x['sol'] for x in fired) + cost_gap:+.4f} SOL")
