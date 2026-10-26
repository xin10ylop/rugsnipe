# The move screen for every wallet, from the complete 13-slot tapes (wide/coin, Oct 23 00:00 - Oct 25 18:00): its launch
# buys (create + 0/1, outside the dev's bundle run) that it first sold inside the tape; the move = the curve price just
# before that sell over the price just after its buy. Slices by its size and its repeat on the create signer; by day.
# Also the share of its launch buys it sold inside the tape (the rest it held longer: not measured here).
import json, glob, os, collections, statistics as st, datetime, bisect
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
K = 30_000_000_000 * 1_073_000_000_000_000
COST = 0.052; STAKE = 0.5
PAIRS = sorted({(r["slot"], r["bt"]) for w in ("AHwJSn", "4g49xG") for r in json.load(open(f"{SP}/dec/five/walk_{w}.json"))["rows"] if r.get("bt")})
PS = [p[0] for p in PAIRS]
def bt_of(slot):
    i = min(max(bisect.bisect_left(PS, slot), 1), len(PS) - 1)
    (s0, t0), (s1, t1) = PAIRS[i - 1], PAIRS[i]
    return int(t0 + (slot - s0) * (t1 - t0) / max(1, s1 - s0))
def day(t): return datetime.datetime.fromtimestamp(t, datetime.timezone.utc).strftime("%m-%d")
W = collections.defaultdict(list); NB = collections.Counter()
for p in glob.glob(SP + "/wide/coin/*.json"):
    d = json.load(open(p))
    if not d.get("create") or not d.get("rows"): continue
    c = d["create"]; cs = d["cs"]; ci = c["idx"]; dd = day(bt_of(cs)); bt = bt_of(cs)
    rows = sorted(d["rows"], key=lambda r: (r[0], r[1]))
    bund = {c["signer"], c["creator"]}; prev = ci
    for r in rows:
        if r[0] != cs or r[1] < ci: continue
        if r[1] == ci or (r[1] - prev <= 2 and r[1] - ci <= 5): bund.add(r[2]); prev = r[1]
        elif r[1] - prev > 2: break
    def post(r): return ((r[6] if r[6] > 0 else K // r[7]), r[7]) if r[7] > 0 else None
    fb = {}
    for i, r in enumerate(rows):
        u = r[2]
        if r[3] == 1 and u not in fb and u not in bund and r[0] <= cs + 1: fb[u] = (i, r)
    for u, (i, r) in fb.items():
        NB[u] += 1
        s = next((x for x in rows[i + 1:] if x[2] == u and x[3] == 0), None)
        if not s: continue
        pb = post(r); ps = post(s)
        if not (pb and ps): continue
        pre_s = (ps[0] + s[4], ps[1] - s[5])
        move = (pre_s[0] / pre_s[1]) / (pb[0] / pb[1]) - 1
        W[u].append(dict(day=dd, bt=bt, move=move, size=r[4] / 1e9, signer=c["signer"], off=s[0] - cs))
res = []
for u, X in W.items():
    if NB[u] < 30 or len(X) < 20: continue
    X.sort(key=lambda x: x["bt"]); seen = collections.defaultdict(list)
    for x in X:
        x["rep"] = sum(1 for t in seen[x["signer"]] if t >= x["bt"] - 86400); seen[x["signer"]].append(x["bt"])
    cuts = {"all": lambda x: True, "size >= 0.5": lambda x: x["size"] >= 0.5, "size >= 1": lambda x: x["size"] >= 1,
            "size < 0.3": lambda x: x["size"] < 0.3, "repeat >= 2": lambda x: x["rep"] >= 2}
    for lab, f in cuts.items():
        s_ = [x for x in X if f(x)]
        if len(s_) < 20: continue
        days = sorted({x["day"] for x in s_})
        byd = {d: st.mean(x["move"] for x in s_ if x["day"] == d) for d in days}
        if len(days) < 3 or not all(v > COST for v in byd.values()): continue
        offs = collections.Counter(x["off"] for x in s_)
        res.append(dict(w=u, cut=lab, n=len(s_), nb=NB[u], sold=len(X) / NB[u], mean=st.mean(x["move"] for x in s_), med=st.median(x["move"] for x in s_),
                        proxy=sum(STAKE * (x["move"] - COST) for x in s_), byd=byd, mode=offs.most_common(1)[0], size=st.median(x["size"] for x in s_)))
res.sort(key=lambda o: -o["med"])
seen = set()
print(f"wallets (and slices) whose mean move is above the ~5.2% cost on each of the 3 days; sorted by the median move")
print(f"{'wallet':9} {'slice':12} {'n':>4} {'launch buys':>11} {'sold in tape':>12} {'size':>5} {'mean':>6} {'median':>6} {'ceiling':>7} {'first sell at':>13}  by day")
for o in res:
    if o["w"] in seen: continue
    seen.add(o["w"])
    print(f"{o['w'][:8]:9} {o['cut']:12} {o['n']:4d} {o['nb']:11d} {o['sold']:12.0%} {o['size']:5.2f} {o['mean']:+6.1%} {o['med']:+6.1%} {o['proxy']:+7.2f} {'create+%d (%d)' % o['mode']:>13}  " +
          " ".join(f"{d[3:]}:{v:+.1%}" for d, v in o["byd"].items()))
