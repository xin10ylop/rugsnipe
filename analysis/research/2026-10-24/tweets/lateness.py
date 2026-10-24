# What being late costs on R5b coins: every real outside buyer's realized position at entry + HR slots, by entry slot offset
# (d = entry slot - create slot) and by entry order. Plus where our own and his buys land (d) today.
#   RS_DAY=24 python3 -I univ/lateness.py HR
import json, glob, os, collections, statistics as St, math, sys, bisect
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/univ")
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"; FEE = 0.0125
DAYN = int(os.environ.get("RS_DAY", "24")); HR = int(sys.argv[1]) if len(sys.argv) > 1 else 8
R = json.load(open(SP + ("/univ/dataset.json" if DAYN == 24 else f"/univ/dataset_{DAYN}.json"))); D = {r["mint"]: r for r in R}
creates = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): creates[c["mint"]] = c
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
inU = lambda r: r["sid"] and r["dev"] >= 0.95 and r["delay"] is not None and r["delay"] < 3600
R5b = lambda r: inU(r) and TEMPLATE(r["dev"]) and r["host"] in TOOLS and r["tweet_rank"] == 1
TT = lambda r: inU(r) and TEMPLATE(r["dev"]) and r["host"] in TOOLS and r["tweet_rank"] != 1
def seq_from_tape(o):
    rows = sorted(o["txs"], key=lambda t: (t["s"], t["i"] if t["i"] is not None else 10**6))
    return [(t["s"], t["i"] or 0, e[1], bool(e[2]), e[3], e[4], e[5], e[6], e[9], e[10]) for t in rows if not t["err"] for e in t["ev"] if e[0] == "T" and len(e) <= 12], max((t["s"] for t in rows), default=0)
def seq_from_early(o):
    rows = sorted(o["rows"], key=lambda t: (t["s"], t["i"] if t["i"] is not None else 10**6))
    return [(t["s"], t["i"] or 0, e[0], bool(e[1]), e[2], e[3], e[4], e[5], e[6] if len(e) > 6 else 0, e[7] if len(e) > 7 else 0) for t in rows if not t["err"] for e in t["ev"]], None
def entries(c, seq, last_slot):
    cs, ci = c["slot"], c["idx"] or 0
    cset = {c["creator"], c["user"], c["signer"]}
    prev = ci; end = (cs, ci); bw = set()
    for s, i, u, b, sol, tok, vs, vt, f1, f2 in seq:
        if (s, i) <= (cs, ci): continue
        if s == cs and b and u not in (A, US) and (i - prev) <= 2 and (i - ci) <= 5: prev = i; end = (s, i); bw.add(u)
        elif (s, i) > end: break
    ins = cset | bw; seen = set(); out = []; k = 0
    for x in seq:
        if (x[0], x[1]) <= end or not x[3] or x[2] in ins or x[2] in seen: continue
        seen.add(x[2]); k += 1
        who, d0 = x[2], x[0] - cs; hz = x[0] + HR
        if last_slot is not None and hz > last_slot: continue
        if last_slot is None and hz > cs + 12: continue
        spent = got = held = 0; st = None
        for s, i, u, b, sol, tok, vs, vt, f1, f2 in seq:
            if s > hz: break
            st = (vs, vt)
            if u != who: continue
            if b: spent += sol + f1 + f2; held += tok
            else: got += sol - f1 - f2; held -= tok
        if not spent: continue
        mark = (st[0] - st[0] * st[1] / (st[1] + held)) * (1 - FEE) if held > 0 else 0
        out.append(dict(k=k, d=d0, ret=(got + mark) / spent - 1, sol=spent, who="him" if who == A else "us" if who == US else "rival"))
    return out
tapes = {}
if DAYN == 24:
    for f in ("tapes.jsonl", "tapes2.jsonl"):
        for l in open(f"{SP}/today/{f}"):
            o = json.loads(l); tapes[o["mint"]] = o
early = {}
for p in glob.glob(SP + "/univ/early/*.json"):
    o = json.load(open(p))
    if not o.get("trunc") and o.get("anchored", True): early[o["mint"]] = o
def ci(v):
    if len(v) < 3: return f"n={len(v)}"
    m = St.mean(v); h = 1.96 * St.pstdev(v) / math.sqrt(len(v)); return f"n={len(v):4d} {100*m:+6.2f}% [{100*(m-h):+6.2f},{100*(m+h):+6.2f}] med {100*St.median(v):+6.2f}%"
for lab, rule in (("R5b (template+tool+tweet-first)", R5b), ("template+tool, NOT tweet-first", TT)):
    E = collections.defaultdict(list); K = collections.defaultdict(list); n = 0; srcs = collections.Counter()
    for m, r in D.items():
        if not rule(r) or m not in creates: continue
        if m in tapes: seq, last = seq_from_tape(tapes[m]); srcs["tape"] += 1
        elif m in early: seq, last = seq_from_early(early[m]); srcs["early"] += 1
        else: continue
        n += 1
        for e in entries(creates[m], seq, last):
            E[min(e["d"], 4)].append(e["ret"]); K[min(e["k"], 6)].append(e["ret"])
    print(f"\n== Oct {DAYN} {lab}: {n} coins with data {dict(srcs)}; each outside buyer's realized position at their entry + {HR} slots (before tips/tx fees)")
    for d in sorted(E): print(f"   entry slot create+{d}{'+' if d == 4 else ' '}: {ci(E[d])}")
    for k in sorted(K): print(f"   entry order {k}{'+' if k == 6 else ' '}:       {ci(K[k])}")
# where buys land today (d) for us and him
if DAYN == 24:
    L = json.load(open(SP + "/today/legs_us.json")); dd = collections.Counter(); ddl = collections.Counter()
    for x in L:
        if x["kind"] != "buy" or x["mint"] not in creates: continue
        d = x["slot"] - creates[x["mint"]]["slot"]; dd[d] += 1
        if not x["err"]: ddl[d] += 1
    print("\nour buy txs today by d (all / landed ok):", sorted(dd.items()), sorted(ddl.items()))
