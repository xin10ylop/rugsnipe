# For every launch he attempted today (full tapes): the first spot behind the dev bundle at 0.6 SOL, his own trades removed
# (his exact-state buy fails if we are first), sold at create + 4 / 6 / 10 slots; joined to the create-time features.
import json, statistics as St, math, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"; FEE = 0.0125
D = {r["mint"]: r for r in json.load(open(SP + "/univ/dataset.json"))}
tapes = {}
for f in ("tapes.jsonl", "tapes2.jsonl"):
    for l in open(f"{SP}/today/{f}"):
        o = json.loads(l); tapes[o["mint"]] = o
def val(vs, vt, q): return (vs - vs * vt / (vt + q)) * (1 - FEE) / 1e9
out = []
for m, o in tapes.items():
    r = D.get(m)
    if not r or not r["he"]: continue
    rows = sorted(o["txs"], key=lambda t: (t["s"], t["i"] if t["i"] is not None else 10**6))
    ctx = next((t for t in rows if any(x.startswith("create") for x in t["ix"]) or any(e[0] == "C" for e in t["ev"])), None)
    if not ctx: continue
    cs, ci = ctx["s"], ctx["i"] or 0
    seq = [(t["s"], t["i"] or 0, e[1], e) for t in rows if not t["err"] for e in t["ev"] if e[0] == "T" and len(e) <= 12]
    dev = [x for x in seq if (x[0], x[1]) == (cs, ci)]
    if not dev: continue
    vs, vt = dev[-1][3][5], dev[-1][3][6]; prev = ci; end = (cs, ci); nb = 0
    for s, i, u, e in seq:
        if (s, i) <= (cs, ci): continue
        if s == cs and e[2] and u not in (A, US) and (i - prev) <= 2 and (i - ci) <= 5: vs, vt = e[5], e[6]; prev = i; end = (s, i); nb += 1
        else: break
    S = 0.6e9 / (1 + FEE); a, b = vs + S, vs * vt / (vs + S); q = vt - b
    later = [(s, i, u, e) for s, i, u, e in seq if (s, i) > end and u != A]
    res = dict(m=m, nb=nb, post_vs=vs / 1e9)
    for H in (4, 6, 10):
        x, y = a, b
        for s, i, u, e in later:
            if s > cs + H: break
            k = x * y
            if e[2]: x += e[3]; y = k / x
            else: y += e[4]; x = k / y
        res[f"v{H}"] = val(x, y, q) / 0.6 - 1
    res.update({k: r[k] for k in ("creator_prior_he", "month_n", "delay", "dev", "host", "img", "tweet_rank", "author_prior_he", "hour", "sid", "he_land")})
    res["his_ret"] = r.get("his_ret")
    out.append(res)
json.dump(out, open(SP + "/univ/hisvalue.json", "w"))
def ci(v):
    v = [x for x in v if x is not None]
    if len(v) < 3: return f"n={len(v)}"
    m = St.mean(v); h = 1.96 * St.pstdev(v) / math.sqrt(len(v)); return f"n={len(v):4d} {100*m:+6.2f}% [{100*(m-h):+6.2f},{100*(m+h):+6.2f}]"
print(f"his attempted launches with a full tape: {len(out)}")
print("first spot, sold at +4:", ci([o["v4"] for o in out]), " +6:", ci([o["v6"] for o in out]), " +10:", ci([o["v10"] for o in out]))
print("his real result on the ones he landed:", ci([o["his_ret"] for o in out]))
for lab, sel in (("creator followed (he attempted 5+ earlier today)", lambda o: o["creator_prior_he"] >= 5), ("followed 1-4", lambda o: 1 <= o["creator_prior_he"] <= 4),
                 ("new to him today", lambda o: o["creator_prior_he"] == 0), ("month trades with creator >= 10", lambda o: o["month_n"] >= 10),
                 ("tweet delay 2-10 s", lambda o: o["delay"] is not None and 2 <= o["delay"] < 10), ("tweet delay >= 10 s", lambda o: o["delay"] is not None and o["delay"] >= 10),
                 ("old Axiom image server", lambda o: o["img"].startswith("axiomtrading.sfo3")), ("dev 3.0-3.95", lambda o: 2.95 <= o["dev"] < 3.96),
                 ("dev 1.0", lambda o: 0.95 <= o["dev"] < 1.05), ("uxento", lambda o: o["host"] == "meta.uxento.io"), ("j7tracker", lambda o: o["host"] == "metadata.j7tracker.io"),
                 ("no tweet link", lambda o: not o["sid"])):
    G = [o for o in out if sel(o)]
    print(f"  {lab:48s} first spot +6: {ci([o['v6'] for o in G])} | his real: {ci([o['his_ret'] for o in G])}")
