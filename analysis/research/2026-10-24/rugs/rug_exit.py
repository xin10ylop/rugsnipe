# Exit on the insiders' dump: once they have sold at least THETA of the tokens they bought in the run (counted through
# the end of slot create+k), sell at create+k+1 (the bot reacts the next slot); else hold to create+10. Value of the
# real first outside buyer's position, net of 1.2%, all rule coins (winners included), by period; and the same for the
# coins where WE were first. Also: does exiting on the insiders' FIRST sell work (docs/09 said no)?
import sys, json, glob, collections, random
sys.path.insert(0, "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/pm")
import bt
SP = bt.SP; A, US = bt.A, bt.US; COST = 0.012
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
U = {x["mint"]: x for x in json.load(open(SP + "/pm/bt_rows.json"))}
res = collections.defaultdict(list)
for p in glob.glob(SP + "/full/*.json"):
    if p.endswith(".tmp"): continue
    t = json.load(open(p)); m = t["mint"]; x = U.get(m); c = cr.get(m)
    if not x or not c or x["vC"].get("10") is None: continue
    seq = [(s, i, u, bool(b), sol, tok, vs, vt, f) for s, i, sg, evs in t["rows"] for (u, b, sol, tok, vs, vt, f) in evs]
    cs, ci = c["slot"], c["idx"] or 0
    cset = {c["creator"], c["user"], c["signer"]}; prev = ci; end = (cs, ci); bw = set()
    for s, i, u, b, sol, tok, vs, vt, f in seq:
        if (s, i) < (cs, ci): continue
        if (s, i) == (cs, ci) or (s == cs and u in cset and (i - prev) <= 2): prev = max(prev, i); end = max(end, (s, i)); continue
        if s == cs and b and u not in (A, US) and (i - prev) <= 2 and (i - ci) <= 5: prev = i; end = (s, i); bw.add(u)
        elif (s, i) > end: break
    ins = cset | bw
    ins_tok = sum(tok for s, i, u, b, sol, tok, *_ in seq if (s, i) <= end and u in ins and b)
    fo = x["fo"]; entry = cs + x["d"]
    def at(k): return bt.val(seq, fo, cs + k)
    cum = 0; first_sell = None; hit = {}
    by_slot = collections.Counter()
    for s, i, u, b, sol, tok, *_ in seq:
        if u in ins and not b and (s, i) > end: by_slot[s - cs] += tok
    for k in range(0, 10):
        cum += by_slot.get(k, 0)
        if first_sell is None and by_slot.get(k, 0) > 0 and cs + k >= entry: first_sell = k
        for th in (0.1, 0.25, 0.5):
            if th not in hit and ins_tok and cum / ins_tok >= th and cs + k >= entry: hit[th] = k
    base = at(10)
    tag = (x["day"], x["hour"] >= 12, x["fo_who"] == "us", x["first_fresh"])
    res[("hold 10",) + tag].append(base)
    for th in (0.1, 0.25, 0.5):
        k = hit.get(th)
        v = at(k + 1) if k is not None and k + 1 < 10 else base
        res[(f"exit when insiders sold {int(th*100)}%",) + tag].append(v if v is not None else base)
    k = first_sell
    v = at(k + 1) if k is not None and k + 1 < 10 else base
    res[("exit on the insiders' first sell",) + tag].append(v if v is not None else base)
def agg(pol, sel):
    v = [a - COST for (p, d, pm, us, ff), vs in res.items() if p == pol and sel(d, pm, us, ff) for a in vs]
    if len(v) < 5: return "n<5"
    random.seed(4); m = sum(v) / len(v); bs = sorted(sum(random.choice(v) for _ in v) / len(v) for _ in range(500))
    return f"{100*m:+6.2f}% [{100*bs[12]:+5.2f},{100*bs[487]:+5.2f}] big<=-30%: {sum(a <= -0.3 for a in v):3d} n {len(v)}"
for pol in ("hold 10", "exit on the insiders' first sell", "exit when insiders sold 10%", "exit when insiders sold 25%", "exit when insiders sold 50%"):
    print(f"== {pol}")
    for lab, sel in (("all coins", lambda d, pm, us, ff: True), ("Oct 23 00-12", lambda d, pm, us, ff: d == 23 and not pm),
                     ("Oct 23 12-24", lambda d, pm, us, ff: d == 23 and pm), ("Oct 24 00-12", lambda d, pm, us, ff: d == 24),
                     ("we were first", lambda d, pm, us, ff: us), ("copies only", lambda d, pm, us, ff: not ff), ("first coins only", lambda d, pm, us, ff: ff)):
        print(f"   {lab:16s} {agg(pol, sel)}")
