# Realized result of the FIRST outside buyer after the dev bundle on each launch (his or a rival's own buy and sells in the
# tape), and whether a deployer's earlier first-buyer results predict the next launch's.
import json, collections, statistics as St, math
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"; FEE = 0.0125
D = {r["mint"]: r for r in json.load(open(SP + "/univ/dataset.json"))}
tapes = {}
for f in ("tapes.jsonl", "tapes2.jsonl"):
    for l in open(f"{SP}/today/{f}"):
        o = json.loads(l); tapes[o["mint"]] = o
out = []
for m, o in tapes.items():
    r = D.get(m)
    rows = sorted(o["txs"], key=lambda t: (t["s"], t["i"] if t["i"] is not None else 10**6))
    ctx = next((t for t in rows if any(x.startswith("create") for x in t["ix"]) or any(e[0] == "C" for e in t["ev"])), None)
    if not ctx or not r: continue
    cs, ci = ctx["s"], ctx["i"] or 0
    cset = set(ctx["signers"]) | {r["creator"]}
    seq = [(t["s"], t["i"] or 0, e[1], e) for t in rows if not t["err"] for e in t["ev"] if e[0] == "T" and len(e) <= 12]
    prev = ci; end = (cs, ci); bw = set()
    for s, i, u, e in seq:
        if (s, i) <= (cs, ci): continue
        if s == cs and e[2] and u not in (A, US) and (i - prev) <= 2 and (i - ci) <= 5: prev = i; end = (s, i); bw.add(u)
        else: break
    ins = cset | bw
    fo = next(((s, i, u, e) for s, i, u, e in seq if (s, i) > end and e[2] and u not in ins), None)
    if not fo: continue
    who = fo[2]
    spent = sum(e[3] + e[9] + e[10] for s, i, u, e in seq if u == who and e[2]); got = sum(e[3] - e[9] - e[10] for s, i, u, e in seq if u == who and not e[2])
    held = sum(e[4] for s, i, u, e in seq if u == who and e[2]) - sum(e[4] for s, i, u, e in seq if u == who and not e[2])
    last = seq[-1][3]; vs, vt = last[5], last[6]
    mark = (vs - vs * vt / (vt + held)) * (1 - FEE) if held > 0 else 0
    ret = (got + mark) / spent - 1 if spent else None
    out.append(dict(m=m, bt=r["bt"], creator=r["creator"], who="him" if who == A else ("us" if who == US else "rival"), ret=ret, sold_all=held <= 0,
                    slot=fo[0] - cs, he=r["he"], tool=r["host"], dev=r["dev"]))
out.sort(key=lambda x: x["bt"])
def ci(v):
    v = [x for x in v if x is not None]
    if len(v) < 3: return f"n={len(v)}"
    m = St.mean(v); h = 1.96 * St.pstdev(v) / math.sqrt(len(v)); return f"n={len(v):4d} {100*m:+6.2f}% [{100*(m-h):+6.2f},{100*(m+h):+6.2f}] med {100*St.median(v):+6.2f}%"
print(f"launches with a first outside buyer in the tape: {len(out)}; who: {collections.Counter(x['who'] for x in out)}; sold everything inside the tape: {sum(1 for x in out if x['sold_all'])}")
print("first buyer's realized result:", ci([x["ret"] for x in out]))
for w in ("him", "rival", "us"): print(f"   first buyer = {w:5s}", ci([x["ret"] for x in out if x["who"] == w]))
# deployer scoreboard: running mean of earlier first-buyer results per creator
hist = collections.defaultdict(list); B = collections.defaultdict(list)
for x in out:
    h = hist[x["creator"]]
    if len(h) >= 2:
        mu = St.mean(h[-5:])
        k = "prior mean < -5%" if mu < -0.05 else ("prior -5..0%" if mu < 0 else ("prior 0..+5%" if mu < 0.05 else "prior >= +5%"))
    else: k = f"{len(h)} earlier launches"
    B[k].append(x["ret"])
    if x["ret"] is not None: h.append(x["ret"])
print("\nnext launch's first-buyer result, by the deployer's last-5 first-buyer mean (earlier today):")
for k in ("0 earlier launches", "1 earlier launches", "prior mean < -5%", "prior -5..0%", "prior 0..+5%", "prior >= +5%"):
    print(f"   {k:20s} {ci(B[k])}")
json.dump(out, open(SP + "/univ/firstbuyer.json", "w"))
