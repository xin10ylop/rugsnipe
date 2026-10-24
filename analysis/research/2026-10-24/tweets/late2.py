# R5b vs not-first template+tool coins: when the first outside buy lands (d, block-position gap after the bundle),
# and the first outside buyer's realized value at entry + HR slots for HR = 1..10 (for an early exit on a late verdict).
import json, glob, os, collections, statistics as St, math, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.argv = [sys.argv[0], "8"]
exec(open(SP + "/univ/lateness.py").read().split("tapes = {}")[0])
tapes = {}
for f in ("tapes.jsonl", "tapes2.jsonl"):
    for l in open(f"{SP}/today/{f}"):
        o = json.loads(l); tapes[o["mint"]] = o
early = {}
for p in glob.glob(SP + "/univ/early/*.json"):
    o = json.load(open(p))
    if not o.get("trunc"): early[o["mint"]] = o
def first_out(c, seq):
    cs, ci = c["slot"], c["idx"] or 0
    cset = {c["creator"], c["user"], c["signer"]}; prev = ci; end = (cs, ci); bw = set()
    for s, i, u, b, sol, tok, vs, vt, f1, f2 in seq:
        if (s, i) <= (cs, ci): continue
        if s == cs and b and u not in (A, US) and (i - prev) <= 2 and (i - ci) <= 5: prev = i; end = (s, i); bw.add(u)
        elif (s, i) > end: break
    ins = cset | bw
    fo = next((x for x in seq if (x[0], x[1]) > end and x[3] and x[2] not in ins), None)
    return fo, end, ins
def val(seq, who, hz):
    spent = got = held = 0; st = None
    for s, i, u, b, sol, tok, vs, vt, f1, f2 in seq:
        if s > hz: break
        st = (vs, vt)
        if u != who: continue
        if b: spent += sol + f1 + f2; held += tok
        else: got += sol - f1 - f2; held -= tok
    if not spent: return None
    mark = (st[0] - st[0] * st[1] / (st[1] + held)) * (1 - FEE) if held > 0 else 0
    return (got + mark) / spent - 1
def ci(v):
    if len(v) < 3: return f"n={len(v)}"
    m = St.mean(v); h = 1.96 * St.pstdev(v) / math.sqrt(len(v)); return f"{100*m:+6.2f}% [{100*(m-h):+6.2f},{100*(m+h):+6.2f}] med {100*St.median(v):+6.2f}% n={len(v)}"
for lab, rule in (("R5b", R5b), ("not tweet-first", TT)):
    dd = collections.Counter(); gap = []; V = collections.defaultdict(list); n = 0
    for m, r in D.items():
        if not rule(r) or m not in creates: continue
        if m in tapes: seq, last = seq_from_tape(tapes[m])
        elif m in early: seq, last = seq_from_early(early[m]); last = creates[m]["slot"] + 12
        else: continue
        n += 1; c = creates[m]
        fo, end, ins = first_out(c, seq)
        if not fo: dd["none in window"] += 1; continue
        d = fo[0] - c["slot"]; dd[min(d, 3)] += 1
        if d == 0: gap.append(fo[1] - end[1])
        for hr in (1, 2, 3, 5, 8, 10):
            if fo[0] + hr <= last:
                v = val(seq, fo[2], fo[0] + hr)
                if v is not None: V[hr].append(v)
    print(f"\n== {lab}: {n} coins. first outside buy lands at d = {dict(sorted(dd.items(), key=lambda x: str(x[0])))}")
    if gap: q = sorted(gap); print(f"   d=0: block positions after the bundle's last trade: p25 {q[len(q)//4]}, median {q[len(q)//2]}, p75 {q[3*len(q)//4]}")
    for hr in sorted(V): print(f"   first outside buyer, value at entry + {hr:2d} slots: {ci(V[hr])}")
