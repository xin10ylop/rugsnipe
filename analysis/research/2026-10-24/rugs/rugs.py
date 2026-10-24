# Big losses on the rule coins (complete tapes): what the bot knows before its buy, and how the insiders sold.
# Insiders = creator, user, signer and the bundle run's wallets. Supply = 1e15 raw token units (1B, 6 decimals).
import sys, json, glob, collections, bisect
sys.path.insert(0, "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/pm")
import bt
SP = bt.SP; A, US = bt.A, bt.US
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
U = {x["mint"]: x for x in json.load(open(SP + "/pm/bt_rows.json"))}
# every create of a signer (all of pump.fun, from the decoded hours) for burst / history features
by_signer = collections.defaultdict(list)
for c in cr.values(): by_signer[c["signer"]].append(c["bt"])
for v in by_signer.values(): v.sort()
rows = []
for p in glob.glob(SP + "/full/*.json"):
    if p.endswith(".tmp"): continue
    t = json.load(open(p)); m = t["mint"]; x = U.get(m); c = cr.get(m)
    if not x or not c: continue
    seq = [(s, i, u, bool(b), sol, tok, vs, vt, f) for s, i, sg, evs in t["rows"] for (u, b, sol, tok, vs, vt, f) in evs]
    cs, ci = c["slot"], c["idx"] or 0
    cset = {c["creator"], c["user"], c["signer"]}; prev = ci; end = (cs, ci); bw = set()
    for s, i, u, b, sol, tok, vs, vt, f in seq:
        if (s, i) < (cs, ci): continue
        if (s, i) == (cs, ci) or (s == cs and u in cset and (i - prev) <= 2): prev = max(prev, i); end = max(end, (s, i)); continue
        if s == cs and b and u not in (A, US) and (i - prev) <= 2 and (i - ci) <= 5: prev = i; end = (s, i); bw.add(u)
        elif (s, i) > end: break
    ins = cset | bw
    ins_sol = sum(sol for s, i, u, b, sol, *_ in seq if (s, i) <= end and u in ins and b) / 1e9
    ins_tok = sum(tok for s, i, u, b, sol, tok, *_ in seq if (s, i) <= end and u in ins and b)
    dev_sol = sum(sol for s, i, u, b, sol, *_ in seq if (s, i) == (cs, ci) and b) / 1e9
    # insiders' selling by slot: share of their tokens sold by create + k
    sold = collections.Counter()
    for s, i, u, b, sol, tok, *_ in seq:
        if u in ins and not b and (s, i) > end: sold[s - cs] += tok
    def share_by(k): return sum(v for kk, v in sold.items() if kk <= k) / ins_tok if ins_tok else 0
    dump_slot = next((k for k in range(0, 13) if share_by(k) >= 0.5), None)
    bts = by_signer[c["signer"]]; i0 = bisect.bisect_left(bts, c["bt"])
    rows.append(dict(m=m, day=x["day"], hour=x["hour"], bt=c["bt"], signer=c["signer"], v10=x["vC"].get("10"), v3=x["vC"].get("3"),
        fo_who=x["fo_who"], d=x["d"], first=x["first_fresh"], pb=x["pb_sol"], n_bw=len(bw), ins_sol=ins_sol, ins_share=ins_tok / 1e15,
        dev=dev_sol, tip=c.get("tip") or 0, cup=c.get("cup") or 0, ix="+".join(c.get("ix") or []),
        dump_slot=dump_slot, sold5=share_by(5), sold10=share_by(10), he=x["he"], h_n=x["h_n"], h_pnl=x["h_pnl"], host=x["host"],
        launches_1h=i0 - bisect.bisect_left(bts, c["bt"] - 3600), gap_s=(c["bt"] - bts[i0 - 1]) if i0 > 0 else None))
json.dump(rows, open(SP + "/pm/rug_rows.json", "w"))
R = [r for r in rows if r["v10"] is not None]
big = [r for r in R if r["v10"] <= -0.30]
print(f"rule coins: {len(R)}; big losses (first outside buyer at create+10 <= -30%): {len(big)} ({100*len(big)/len(R):.1f}%); "
      f"their share of all the money lost: {100*sum(r['v10'] for r in big)/sum(r['v10'] for r in R if r['v10'] < 0):.0f}%")
print("   mean value all: %+.2f%%, without the big losses: %+.2f%%" % (100*sum(r['v10'] for r in R)/len(R), 100*sum(r['v10'] for r in R if r not in big)/(len(R)-len(big))))
print("big losses: when did the insiders sell half (slot after the create):", dict(sorted(collections.Counter(r["dump_slot"] if r["dump_slot"] is not None else 99 for r in big).items())))
print("everyone   : insiders sold half by +5 / +10:", f"{100*sum(r['sold5']>=0.5 for r in R)/len(R):.0f}% / {100*sum(r['sold10']>=0.5 for r in R)/len(R):.0f}%;",
      "big losses:", f"{100*sum(r['sold5']>=0.5 for r in big)/len(big):.0f}% / {100*sum(r['sold10']>=0.5 for r in big)/len(big):.0f}%")
print("big losses by tweet status: first", sum(r["first"] for r in big), "copies", sum(not r["first"] for r in big), "| first buyer:", dict(collections.Counter(r["fo_who"] for r in big)))
