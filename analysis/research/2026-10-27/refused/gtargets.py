# Ghost targets, stage 1 (no RPC): every wallet's in-class launch buys (create + 0/1, outside the dev's bundle run) on
# every tape we hold, copied at the GHOST position (after its buy and j more trades, j from the 83 create-sent buys,
# presend.json; P_AFTER 0.8; bound post-bundle + 10%), sold at create + h (h = 3, 5, 7, 9, 12). Tape sets: the 5-day
# set (Oct 21-25, coins he or we touched, 13 slots), oos/coin (Oct 25 18:00 - Oct 26 14:50, coins the copy finalists
# bought, 13 slots), refused/tape (60 slots: our live coins and his 5-day copies). One tape per mint, the longest.
# The universes are not the wallets' full buy sets: this ranks, stage 2 (gtargets2.py) validates on their own trades.
#   -> SP/refused/gt1.json
import json, glob, collections, bisect, os, datetime
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
K = 30_000_000_000 * 1_073_000_000_000_000
FEE = 0.0125; COST = 0.0045; BIAS = 0.0155; P_AFTER = 0.8; CAP = 0.10
J = json.load(open(SP + "/refused/presend.json"))["js"]; JP = {j: c / len(J) for j, c in collections.Counter(J).items()}
HS = (3, 5, 7, 9, 12)
def pnl(en, ex, stake=0.5):
    v0, t0 = en; k = v0 * t0; s_in = stake * 1e9 / (1 + FEE); q = t0 - k / (v0 + s_in)
    vr, tr = ex
    if tr <= q: return None
    return vr * q / (tr - q) * (1 - FEE) / 1e9 - stake - COST
# slot -> unix time, from his 5-day rows and our live buys
pairs = []
for r in json.load(open(SP + "/dec/five/walk_AHwJSn.json"))["rows"]:
    if r.get("bt"): pairs.append((r["slot"], r["bt"]))
for v in json.load(open(SP + "/refused/live.json"))["mine"].values():
    for a in v:
        if a.get("bt"): pairs.append((a["slot"], a["bt"]))
pairs.sort(); PS = [p[0] for p in pairs]
def t_of(slot):
    i = min(max(bisect.bisect_left(PS, slot), 1), len(PS) - 1)
    (s0, t0), (s1, t1) = pairs[i - 1], pairs[i]
    return t0 + (slot - s0) * (t1 - t0) / max(1, s1 - s0)
tapes = {}
def add(m, cs, rows, create, src, span):
    old = tapes.get(m)
    if old and old["span"] >= span: return
    tapes[m] = dict(cs=cs, rows=rows, create=create, src=src, span=span)
for p in glob.glob(SP + "/dec/five/coin/*.json"):
    d = json.load(open(p))
    if d.get("create") and d.get("rows") and d["create"].get("ix") == "create_v2":
        add(d["mint"], d["cs"], [r[:8] for r in d["rows"]], d["create"], "five", 12)
for p in glob.glob(SP + "/oos/coin/*.json"):
    d = json.load(open(p))
    if d.get("create") and d.get("rows") and d["create"].get("ix") == "create_v2" and d.get("reached"):
        add(d["mint"], d["cs"], [r[:8] for r in d["rows"]], d["create"], "oos", 12)
for p in glob.glob(SP + "/refused/tape/*.json"):
    t = json.load(open(p)); m = os.path.basename(p)[:-5]
    cr = (tapes.get(m) or {}).get("create")
    add(m, t["c0"], t["rows"], cr, "long", t.get("span", 60))
print("tapes", len(tapes), collections.Counter(v["src"] for v in tapes.values()))
out = collections.defaultdict(list)
for m, t in tapes.items():
    rows = sorted((r for r in t["rows"] if r[7] > 0), key=lambda r: (r[0], r[1]))
    if not rows or sum(1 for r in rows if r[6] <= 0) > 0.5 * len(rows): continue     # priced in another token
    seq = [(r[0], r[1], r[6] if r[6] > 0 else K // r[7], r[7], r[2], r[3], r[4]) for r in rows]
    cs = t["cs"]; c = t["create"]
    ci = c["idx"] if c else seq[0][1]
    bund = {c["signer"], c["creator"]} if c else {seq[0][4]}
    prev = ci; last = None
    for x in seq:
        if x[0] != cs or x[1] < ci: continue
        if x[1] == ci or (x[1] - prev <= 2 and x[1] - ci <= 5): bund.add(x[4]); prev = x[1]; last = x
        elif x[1] - prev > 2: break
    if not last: last = seq[0]
    pbp = last[2] / last[3]
    def st_end(slot):
        i = bisect.bisect_left(seq, (slot + 1, -1), key=lambda x: (x[0], x[1]))
        return (seq[i - 1][2], seq[i - 1][3]) if i else None
    first = {}
    for k, x in enumerate(seq):
        if x[5] == 1 and x[4] not in first: first[x[4]] = k
    for w, k in first.items():
        x = seq[k]
        if w in bund or x[0] > cs + 1: continue
        r = dict(m=m, t=t_of(x[0]), sol=x[6] / 1e9, at=x[0] - cs, src=t["src"])
        for h in HS:
            if cs + h - 1 > cs + t["span"]: r[f"g{h}"] = None; continue
            ex = st_end(cs + h - 1)
            e = 0.0; fill = 0.0
            for j, p in JP.items():
                if k + j >= len(seq): continue
                s = (seq[k + j][2], seq[k + j][3])
                if (s[0] / s[1]) / pbp - 1 > CAP: continue
                v = pnl(s, ex)
                if v is None: continue
                e += P_AFTER * p * v; fill += P_AFTER * p
            r[f"g{h}"] = e - BIAS * fill
        out[w].append(r)
json.dump(out, open(SP + "/refused/gt1.json", "w"))
print("wallets with a launch buy", len(out), "; with 30+:", sum(1 for v in out.values() if len(v) >= 30))
