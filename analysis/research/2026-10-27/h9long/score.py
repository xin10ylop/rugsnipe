# Score sim2 rows: per setting and filter, P1 (Oct 19-22) and P2 (Oct 23-27), with costs and the bias, without each
# period's best 2 coins.     python3 -I score.py WALLET
import sys, json, collections, datetime
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = sys.argv[1]
R = json.load(open(f"{SP}/h9long/{W[:8]}_sim2.json"))
BIAS = 0.0155
def per(t): return "P1" if datetime.datetime.fromtimestamp(t, datetime.timezone.utc).day <= 22 else "P2"
def net(r, fee): return r["net_curve"] - 2 * fee - BIAS
def cell(rs, fee):
    v = sorted((net(r, fee) for r in rs), reverse=True)
    return f"{len(v):5d} {sum(v):+7.2f} ({sum(v[2:]):+7.2f})" if v else "    0"
def show(title, pred):
    print(f"\n== {title}    [copies, net SOL (without best 2)] at fee 0.0045 | 0.0095 a transaction")
    g = collections.defaultdict(lambda: collections.defaultdict(list))
    for r in R:
        if pred(r): g[(r["kind"][6:], r["stake"], r["where"], r["exit"])][per(r["bt"])].append(r)
    for k in sorted(g, key=str):
        a, b = g[k]["P1"], g[k]["P2"]
        print(f"   {k[0]:12s} stake {str(k[1]):4s} land {k[2]:4s} exit {k[3]:6s}  P1 {cell(a, .0045)} {cell(a, .0095)} | P2 {cell(b, .0045)} {cell(b, .0095)}")
if __name__ == "__main__":
    show("every trip", lambda r: True)
