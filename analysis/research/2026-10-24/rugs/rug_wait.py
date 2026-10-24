# Wait and see: buy at the start of create+K (K = 1, 2) only if the insiders have not sold by then; sell at create+10
# (first coins) or create+max(3, K+1) (the rest, live "exit"). Net of both pump fees and 1.2%. Against buying behind
# the bundle as now (the first outside buyer's value). All rule coins and the insider-heavy ones (share >= 20%).
import json, glob, collections, sys
exec(open("/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/pm/late_entry.py").read().split("res = collections.defaultdict(list)")[0])
RR = {r["m"]: r for r in json.load(open(SP + "/pm/rug_rows.json"))}
A, US = bt.A, bt.US
out = collections.defaultdict(list)
for p in glob.glob(SP + "/full/*.json"):
    if p.endswith(".tmp"): continue
    t = json.load(open(p)); x = U.get(t["mint"]); c = cr.get(t["mint"])
    if not x or not c or x["vC"].get("10") is None or t["mint"] not in RR: continue
    seq = [(s, i, u, bool(b), sol, tok, vs, vt, f) for s, i, sg, evs in t["rows"] for (u, b, sol, tok, vs, vt, f) in evs]
    cs, ci = c["slot"], c["idx"] or 0
    cset = {c["creator"], c["user"], c["signer"]}; prev = ci; end = (cs, ci); bw = set()
    for s, i, u, b, sol, tok, *_ in seq:
        if (s, i) < (cs, ci): continue
        if (s, i) == (cs, ci) or (s == cs and u in cset and (i - prev) <= 2): prev = max(prev, i); end = max(end, (s, i)); continue
        if s == cs and b and u not in (A, US) and (i - prev) <= 2 and (i - ci) <= 5: prev = i; end = (s, i); bw.add(u)
        elif (s, i) > end: break
    ins = cset | bw
    sold_before = lambda K: any(u in ins and not b and (s, i) > end and s < cs + K for s, i, u, b, *_ in seq)
    H = lambda K: 10 if x["first_fresh"] else max(3, K + 1, x["d"] + 1)
    base = (x["vC"].get(str(10 if x["first_fresh"] else max(3, x["d"] + 1))) or x["vC"]["10"]) - 0.012
    heavy = RR[t["mint"]]["ins_share"] >= 0.20
    tag = (x["day"], x["hour"] >= 12, heavy)
    out[("now: behind the bundle",) + tag].append(base)
    for K in (1, 2):
        v = late(seq, cs, K, H(K), 0.5e9)
        out[(f"buy at +{K} always",) + tag].append((v - 0.012) if v is not None else base)
        out[(f"buy at +{K} if insiders held, else skip",) + tag].append(None if sold_before(K) else ((v - 0.012) if v is not None else base))
        out[(f"heavy only: +{K} if held, else skip; rest now",) + tag].append(base if not heavy else (None if sold_before(K) else ((v - 0.012) if v is not None else base)))
def agg(pol, sel):
    v = [a for (p, d, pm, hv), vs in out.items() if p == pol and sel(d, pm, hv) for a in vs]
    tr = [a for a in v if a is not None]
    return f"{len(tr):4d} trades {100*sum(tr)/max(1,len(tr)):+6.2f}% big {sum(a <= -0.3 for a in tr):2d} total {0.5*sum(tr):+7.2f} SOL"
for pol in sorted({k[0] for k in out}):
    print(f"== {pol}")
    for lab, sel in (("all", lambda d, pm, hv: True), ("23 AM", lambda d, pm, hv: d == 23 and not pm), ("23 PM", lambda d, pm, hv: d == 23 and pm), ("24 AM", lambda d, pm, hv: d == 24)):
        print(f"   {lab:6s} {agg(pol, sel)}")
