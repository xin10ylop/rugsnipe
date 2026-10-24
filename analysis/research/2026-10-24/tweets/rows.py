# Per-coin outcome rows for the template+tool universe of one day: the first outside buyer after the bundle, its entry slot
# offset and its realized value at entry + k slots (k = 1, 2, 3, 5, 8, 10), plus the post-bundle virtual SOL.
#   RS_DAY=24 python3 -I univ/rows.py      -> univ/rows_<day>.json
import json, glob, os, collections, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"; FEE = 0.0125
DAYN = int(os.environ.get("RS_DAY", "24"))
R = json.load(open(SP + ("/univ/dataset.json" if DAYN == 24 else f"/univ/dataset_{DAYN}.json")))
creates = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): creates[c["mint"]] = c
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
def seq_from_tape(o):
    rows = sorted(o["txs"], key=lambda t: (t["s"], t["i"] if t["i"] is not None else 10**6))
    return [(t["s"], t["i"] or 0, e[1], bool(e[2]), e[3], e[4], e[5], e[6], e[9], e[10]) for t in rows if not t["err"] for e in t["ev"] if e[0] == "T" and len(e) <= 12], max((t["s"] for t in rows), default=0)
def seq_from_early(o, cs):
    rows = sorted(o["rows"], key=lambda t: (t["s"], t["i"] if t["i"] is not None else 10**6))
    return [(t["s"], t["i"] or 0, e[0], bool(e[1]), e[2], e[3], e[4], e[5], e[6] if len(e) > 6 else 0, e[7] if len(e) > 7 else 0) for t in rows if not t["err"] for e in t["ev"]], cs + 12
tapes = {}
if DAYN == 24:
    for f in ("tapes.jsonl", "tapes2.jsonl"):
        for l in open(f"{SP}/today/{f}"):
            o = json.loads(l); tapes[o["mint"]] = o
early = {}
for p in glob.glob(SP + "/univ/early/*.json"):
    o = json.load(open(p))
    if not o.get("trunc"): early[o["mint"]] = o
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
out = []
U = [r for r in R if r["dev"] >= 0.95 and TEMPLATE(r["dev"]) and r["host"] in TOOLS]
for r in U:
    m = r["mint"]; c = creates.get(m)
    if not c: continue
    if m in tapes: seq, last = seq_from_tape(tapes[m]); src = "tape"
    elif m in early: seq, last = seq_from_early(early[m], c["slot"]); src = "early"
    else: continue
    cs, ci = c["slot"], c["idx"] or 0
    cset = {c["creator"], c["user"], c["signer"]}; prev = ci; end = (cs, ci); bw = set(); pb = None
    for s, i, u, b, sol, tok, vs, vt, f1, f2 in seq:
        if (s, i) < (cs, ci): continue
        if (s, i) == (cs, ci) or (s == cs and u in cset and (i - prev) <= 2): prev = max(prev, i); end = max(end, (s, i)); pb = vs; continue
        if s == cs and b and u not in (A, US) and (i - prev) <= 2 and (i - ci) <= 5: prev = i; end = (s, i); bw.add(u); pb = vs
        elif (s, i) > end: break
    ins = cset | bw
    fo = next((x for x in seq if (x[0], x[1]) > end and x[3] and x[2] not in ins), None)
    row = dict(mint=m, hour=r["hour"], dev=r["dev"], host=r["host"], he=r["he"], us=r.get("us"), sid=r["sid"],
               fresh=bool(r["sid"] and r["delay"] is not None and r["delay"] < 3600), delay=r["delay"],
               first=bool(r["sid"] and r["tweet_rank"] == 1), rank=r["tweet_rank"], creator=r["creator"],
               cph=r.get("creator_prior_he"), cp=r.get("creator_prior"), mn=r.get("month_n"), mp=r.get("month_pnl"),
               pb_sol=(pb / 1e9) if pb else None, nbw=len(bw), src=src, d=None, who=None)
    if fo:
        row["d"] = fo[0] - cs; row["who"] = "him" if fo[2] == A else "us" if fo[2] == US else "rival"
        for k in (1, 2, 3, 5, 8, 10):
            if fo[0] + k <= last: row[f"v{k}"] = val(seq, fo[2], fo[0] + k)
    out.append(row)
json.dump(out, open(SP + f"/univ/rows_{DAYN}.json", "w"))
n_he = sum(1 for r in U if r["he"]); n_not = len(U) - n_he
print(f"day {DAYN}: template+tool creates {len(U)} (his attempts {n_he}); rows with data {len(out)} "
      f"(his {sum(1 for x in out if x['he'])}, others {sum(1 for x in out if not x['he'])}), with a first outside buy {sum(1 for x in out if x['d'] is not None)}")
