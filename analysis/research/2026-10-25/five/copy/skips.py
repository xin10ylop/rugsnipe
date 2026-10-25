# What the tip-keyed copy (listed signers, his tip >= 4M, bound 5%, sold at H-1) picks up among his coins the rule
# skips, by the rule's own reason (the exact replay's decision, Oct 23 -> now): the leader filter, the 40 vSOL cap,
# not followed, and the rest. His net on all his landed coins of each reason, next to what the copy makes there.
import json, sys, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/dec/copy/weakdays.py").read().split("L = [r for r in rows if r[\"listed\"]]")[0]
exec(src)
PREM = 0.05
def reason(m):
    r = rep.get(m)
    if not r: return "not a candidate (dev buy < 0.25 SOL, class)"
    if r["fired"]: return "fired by the rule"
    d = (r["decisions"] or ["?"])[-1]
    if d in ("skip_far_leader",): return "leader outside Europe"
    if d in ("skip_bundle_too_big",): return "above 40 vSOL"
    if d in ("skip_not_followed", "skip_dev_buy", "skip_not_operator", "skip_uri_host", "skip_low_tier"): return "not followed / off template"
    return d
def cval(r):
    vs = []
    for D in dec:
        en = entry_at(r, D)
        if not en: continue
        if (en[0] / en[1]) / r["hisp"] - 1 > PREM: vs.append(-0.0001); continue
        ex = exit_at(r["seq"], r["cs"], max(r["H"] - 1, 1), r["s"])
        v = pnl_s(en, ex, 0.5) if ex else None
        if v is not None: vs.append(v)
    return sum(vs) / len(vs) if vs else None
g = collections.defaultdict(lambda: [0, 0.0, 0, 0.0])
for r in rows:
    if r["bt"] < T0: continue
    k = reason(r["m"]); g[k][0] += 1; g[k][1] += r["his"]
    if r["listed"] and r["tip"] >= 4e6:
        v = cval(r)
        if v is not None: g[k][2] += 1; g[k][3] += v
print(f"his landed buys at create+0/1, Oct 23 -> now, by the rule's decision:")
print(f"   {'rule decision':46} {'his coins':>9} {'his net':>9} {'copied':>7} {'copy net':>9}")
for k, (n, h, c, v) in sorted(g.items(), key=lambda kv: -kv[1][1]):
    print(f"   {k:46} {n:9} {h:+9.2f} {c:7} {v:+9.2f}")
