# The real first outside buyer after the dev bundle: their position value at create + H slots (sells so far + the rest marked
# to the curve at that point), for his attempted launches (full tapes) and the skipped sample (early files).
import json, glob, os, collections, statistics as St, math, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"; FEE = 0.0125; H = int(sys.argv[1]) if len(sys.argv) > 1 else 10
D = {r["mint"]: r for r in json.load(open(SP + "/univ/dataset.json"))}
creates = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): creates[c["mint"]] = c
def seq_from_tape(o):
    rows = sorted(o["txs"], key=lambda t: (t["s"], t["i"] if t["i"] is not None else 10**6))
    return [(t["s"], t["i"] or 0, e[1], bool(e[2]), e[3], e[4], e[5], e[6], e[9], e[10]) for t in rows if not t["err"] for e in t["ev"] if e[0] == "T" and len(e) <= 12]
def seq_from_early(o):
    rows = sorted(o["rows"], key=lambda t: (t["s"], t["i"] if t["i"] is not None else 10**6))
    return [(t["s"], t["i"] or 0, e[0], bool(e[1]), e[2], e[3], e[4], e[5], e[6] if len(e) > 6 else 0, e[7] if len(e) > 7 else 0) for t in rows if not t["err"] for e in t["ev"]]
def fb(c, seq):
    cs, ci = c["slot"], c["idx"] or 0
    cset = {c["creator"], c["user"], c["signer"]}
    prev = ci; end = (cs, ci); bw = set()
    for s, i, u, b, sol, tok, vs, vt, f1, f2 in seq:
        if (s, i) <= (cs, ci): continue
        if s == cs and b and u not in (A, US) and (i - prev) <= 2 and (i - ci) <= 5: prev = i; end = (s, i); bw.add(u)
        elif (s, i) > end: break
    ins = cset | bw
    fo = next((x for x in seq if (x[0], x[1]) > end and x[3] and x[2] not in ins), None)
    if not fo: return None
    who = fo[2]; spent = got = held = 0; st = None
    for s, i, u, b, sol, tok, vs, vt, f1, f2 in seq:
        if s > cs + H: break
        st = (vs, vt)
        if u != who: continue
        if b: spent += sol + f1 + f2; held += tok
        else: got += sol - f1 - f2; held -= tok
    if not spent or st is None: return None
    mark = (st[0] - st[0] * st[1] / (st[1] + held)) * (1 - FEE) if held > 0 else 0
    return dict(who="him" if who == A else ("us" if who == US else "rival"), ret=(got + mark) / spent - 1, slot=fo[0] - cs)
res = {"attempted": [], "skipped": []}
tapes = {}
for f in ("tapes.jsonl", "tapes2.jsonl"):
    for l in open(f"{SP}/today/{f}"):
        o = json.loads(l); tapes[o["mint"]] = o
for m, o in tapes.items():
    r = D.get(m); c = creates.get(m)
    if r and c and r["he"]:
        x = fb(c, seq_from_tape(o))
        if x: res["attempted"].append(x)
U = {r["mint"] for r in D.values() if r["sid"] and r["dev"] >= 0.95 and r["delay"] is not None and r["delay"] < 3600 and not r["he"]}
ntr = 0
for p in glob.glob(SP + "/univ/early/*.json"):
    o = json.load(open(p))
    if o["mint"] not in U: continue
    if o.get("trunc"): ntr += 1; continue
    x = fb(creates[o["mint"]], seq_from_early(o))
    if x: res["skipped"].append(x)
def ci(v):
    if len(v) < 3: return f"n={len(v)}"
    m = St.mean(v); h = 1.96 * St.pstdev(v) / math.sqrt(len(v)); return f"n={len(v):4d} {100*m:+6.2f}% [{100*(m-h):+6.2f},{100*(m+h):+6.2f}] med {100*St.median(v):+6.2f}%"
print(f"first outside buyer's position at create + {H} slots (before tips and tx fees); skipped launches dropped as truncated: {ntr}")
for k in ("attempted", "skipped"):
    G = res[k]; print(f"  launches he {k:9s}: {ci([x['ret'] for x in G])}   first buyer: {dict(collections.Counter(x['who'] for x in G))}")
    for w in ("him", "rival", "us"):
        g = [x["ret"] for x in G if x["who"] == w]
        if g: print(f"      first buyer = {w:5s} {ci(g)}")
