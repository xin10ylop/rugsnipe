# The candidate rule on the tape model, by day: copy its buys of at least MIN SOL on any pump curve (both quotes), land
# in its slot or the next, exit a step ahead of its own TP / SL / 30-min stop. Per day, with and without the best 2.
#   python3 -I rule.py WALLET MIN STAKE LAND FEE [ROUTE]
import sys, json, collections, datetime
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W, MIN, STAKE, LAND, FEE = sys.argv[1], float(sys.argv[2]), sys.argv[3], sys.argv[4], float(sys.argv[5])
ROUTE = sys.argv[6] if len(sys.argv) > 6 else None
BIAS = 0.0155
R = [r for r in json.load(open(f"{SP}/h9long/{W[:8]}_sim2.json")) if str(r["stake"]) == STAKE and r["where"] == LAND and r["exit"] == "before"
     and (r["its_sol"] >= MIN or (ROUTE and r["ix"] == ROUTE))]
g = collections.defaultdict(list)
for r in R: g[datetime.datetime.fromtimestamp(r["bt"], datetime.timezone.utc).strftime("%m-%d")].append(r["net_curve"] - 2 * FEE - BIAS)
print(f"its buys >= {MIN} SOL{' or route ' + ROUTE if ROUTE else ''}, stake {STAKE}, land {LAND}, fee {FEE}: {len(R)} copies, {sum(sum(v) for v in g.values()):+.2f}")
for d in sorted(g):
    v = sorted(g[d], reverse=True); print(f"   {d}  {len(v):4d}  {sum(v):+7.2f}  without best 2 {sum(v[2:]):+7.2f}  won {sum(1 for x in v if x > 0)}")
