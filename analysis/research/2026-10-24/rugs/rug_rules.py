# Pre-buy rug rules, ex ante, in time order: (a) the bot's dumper list (an insider wallet that sold >= THR of its run
# tokens within W slots of an earlier launch), (b) a deployer back after an hour with no launch. Scored on all rule coins
# and per period, net of 1.2%, hold 10 and the live "exit" policy (copies sold at create+3).
import sys, json, glob, collections, random
sys.path.insert(0, "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/pm")
import bt
SP = bt.SP; A, US = bt.A, bt.US; COST = 0.012
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
U = {x["mint"]: x for x in json.load(open(SP + "/pm/bt_rows.json"))}
RR = {r["m"]: r for r in json.load(open(SP + "/pm/rug_rows.json"))}
coins = []
for p in glob.glob(SP + "/full/*.json"):
    if p.endswith(".tmp"): continue
    t = json.load(open(p)); m = t["mint"]; c = cr.get(m)
    if not c: continue
    seq = [(s, i, u, bool(b), sol, tok) for s, i, sg, evs in t["rows"] for (u, b, sol, tok, vs, vt, f) in evs]
    cs, ci = c["slot"], c["idx"] or 0
    cset = {c["creator"], c["user"], c["signer"]}; prev = ci; end = (cs, ci); bw = set()
    for s, i, u, b, sol, tok in seq:
        if (s, i) < (cs, ci): continue
        if (s, i) == (cs, ci) or (s == cs and u in cset and (i - prev) <= 2): prev = max(prev, i); end = max(end, (s, i)); continue
        if s == cs and b and u not in (A, US) and (i - prev) <= 2 and (i - ci) <= 5: prev = i; end = (s, i); bw.add(u)
        elif (s, i) > end: break
    ins = cset | bw
    bought = collections.Counter(); sold = collections.defaultdict(collections.Counter)
    for s, i, u, b, sol, tok in seq:
        if u in ins and b and (s, i) <= end: bought[u] += tok
        if u in ins and not b and (s, i) > end: sold[u][s - cs] += tok
    coins.append(dict(m=m, bt=c["bt"], slot=cs, ins=ins, bought=bought, sold=sold, signer=c["signer"]))
coins.sort(key=lambda x: x["slot"])
def dumped(x, w, thr):
    tot = sum(x["bought"].values())
    if not tot: return set()
    s = sum(v for u in x["ins"] for k, v in x["sold"][u].items() if k <= w)
    return set(u for u in x["ins"] if x["bought"][u] > 0) if s >= thr * tot else set()
def ev(x):
    r = U.get(x["m"]); return r
flags = collections.defaultdict(dict)
for w, thr in ((3, 0.5), (5, 0.5), (3, 0.25), (10, 0.5)):
    marked = {}   # wallet -> time marked (the dump is visible by create slot + w; ~0.4 s a slot)
    for x in coins:
        hit = any(u in marked and marked[u] < x["bt"] for u in x["ins"])
        flags[x["m"]][(w, thr)] = hit
        for u in dumped(x, w, thr): marked.setdefault(u, x["bt"] + w * 0.4)
for x in coins:
    r = RR.get(x["m"])
    flags[x["m"]]["dormant"] = bool(r and r["launches_1h"] == 0)
def V(x, pol):
    if pol == "exit" and not x["first_fresh"]:
        v = x["vC"].get(str(max(3, x["d"] + 1)))
        if v is not None: return v
    return x["vC"].get("10")
PER = [("Oct 23 00-12", lambda x: x["day"] == 23 and x["hour"] < 12), ("Oct 23 12-24", lambda x: x["day"] == 23 and x["hour"] >= 12), ("Oct 24 00-12", lambda x: x["day"] == 24)]
rows = [x for x in U.values() if x["vC"].get("10") is not None and x["mint"] in flags]
def line(sel, keyf, pol):
    keep = [x for x in rows if sel(x) and not keyf(x)]; skip = [x for x in rows if sel(x) and keyf(x)]
    vk = [V(x, pol) - COST for x in keep]; vs = [V(x, pol) - COST for x in skip]
    big_s = sum(v <= -0.3 for v in vs); big_all = big_s + sum(v <= -0.3 for v in vk)
    return (f"skips {len(skip):4d} ({100*len(skip)/max(1,len(keep)+len(skip)):3.0f}%), their mean {100*sum(vs)/max(1,len(vs)):+6.2f}%, "
            f"big losses removed {big_s:2d} of {big_all:2d}; total P&L change {0.5*-sum(vs):+7.3f} SOL")
RULES = [("dumper list: insiders sold 50% within 3 slots (the bot's rule)", lambda x: flags[x["mint"]][(3, 0.5)]),
         ("dumper list: 50% within 5 slots", lambda x: flags[x["mint"]][(5, 0.5)]),
         ("dumper list: 25% within 3 slots", lambda x: flags[x["mint"]][(3, 0.25)]),
         ("dumper list: 50% within 10 slots", lambda x: flags[x["mint"]][(10, 0.5)]),
         ("dormant deployer (no launch in the hour before)", lambda x: flags[x["mint"]]["dormant"]),
         ("dumper 3/50% or dormant", lambda x: flags[x["mint"]][(3, 0.5)] or flags[x["mint"]]["dormant"])]
for pol in ("hold10", "exit"):
    print(f"######## policy: {pol}")
    for name, f in RULES:
        print(f"== {name}")
        print(f"   all          : {line(lambda x: True, f, pol)}")
        for lab, sel in PER: print(f"   {lab}: {line(sel, f, pol)}")
