# Bundle wallets (same-slot buyers right behind the create, as bt.coin finds them) and the first-outside-buyer value
# for every tape in full/ (rule universe) and fp/ (outside universe).  -> dec/bw.json {mint: {...}}
import json, glob, sys, os
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/pm")
import bt
A, US = bt.A, bt.US
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
OUT = SP + "/dec/bw.json"
out = json.load(open(OUT)) if os.path.exists(OUT) else {}
for d in ("full", "fp"):
    for p in glob.glob(f"{SP}/{d}/*.json"):
        if p.endswith(".tmp") or os.path.basename(p).startswith("_"): continue
        m = os.path.basename(p)[:-5]
        if m in out or m not in cr: continue
        try: tape = json.load(open(p))
        except Exception: continue
        if not tape.get("reached"): continue
        c = cr[m]
        seq = [(s, i, u, bool(b), sol, tok, vs, vt, f) for s, i, sg, evs in tape["rows"] for (u, b, sol, tok, vs, vt, f) in evs]
        cs, ci = c["slot"], c["idx"] or 0
        cset = {c["creator"], c["user"], c["signer"]}; prev = ci; bw = []
        for s, i, u, b, sol, tok, vs, vt, f in seq:
            if (s, i) <= (cs, ci): continue
            if s == cs and u in cset and (i - prev) <= 2: prev = max(prev, i); continue
            if s == cs and b and u not in (A, US) and (i - prev) <= 2 and (i - ci) <= 5: prev = i; bw.append(u)
            elif s > cs or (i - prev) > 2: break
        x = bt.coin(c, tape)
        out[m] = dict(src=d, bw=bw, pb=x["pb_sol"], fo_who=x.get("fo_who"), d=x.get("d"), vC=x.get("vC"), vE=x.get("vE"), ins_sell=x["ins_sell"])
json.dump(out, open(OUT, "w"))
print("coins", len(out), {d: sum(1 for v in out.values() if v["src"] == d) for d in ("full", "fp")})
