# What exactly about an operator's recent coins moves his take of the next one (last night, cells taken 10-90%, launches
# with separate bundle transactions)? Variants of the measure, each split into terciles over all launches, his take in
# the top vs bottom tercile; plus the market's recent heat and his own cumulative P&L on the operator.
import json, os, collections, statistics as st, bisect
N = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/dec/night"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"
S = json.load(open("prebuy.json")); ops = json.load(open(N + "/ops.json")); him = json.load(open(N + "/him.json"))
exec(open("structure.py").read().split("cell = collections.defaultdict(list)")[0])
M = {}
for m, d in S.items():
    p = f"{N}/coins/{m}.json"
    if not os.path.exists(p): continue
    t = json.load(open(p)); cs = t["cs"]; ws = set(ops[d["op"]][m]["wallets"]); dev = t["create"]["user"]
    ev = sorted(((row[0], row[1], e) for row in t["rows"] for e in row[3] if e[4] > 0 and e[5] > 0), key=lambda x: (x[0], x[1]))
    b = [x for x in ev if x[2][0] in ws and x[2][1]]
    if not b: continue
    lb = max(b, key=lambda x: (x[0], x[1])); p0 = lb[2][4] / lb[2][5]
    def price_at(k):
        xs = [x for x in ev if x[0] <= cs + k]; return xs[-1][2][4] / xs[-1][2][5] / p0 - 1
    def price_at_ex(k):  # without his own trades: the last trade not his
        xs = [x for x in ev if x[0] <= cs + k and x[2][0] != A]; return xs[-1][2][4] / xs[-1][2][5] / p0 - 1 if xs else 0
    out_sol = sum(x[2][2] for x in ev if x[2][1] and x[2][0] not in ws and x[2][0] != dev and x[2][0] != A and (x[0], x[1]) > (lb[0], lb[1])) / 1e9
    peak = max(x[2][4] / x[2][5] for x in ev if (x[0], x[1]) >= (lb[0], lb[1])) / p0 - 1
    ins_sell = min([x[0] - cs for x in ev if x[2][0] in ws and not x[2][1]] or [99])
    h = him.get(m) or {}
    M[m] = dict(v3=price_at(3), v6=price_at(6), v10=price_at(10), v10x=price_at_ex(10), out=out_sol, peak=peak, dump=ins_sell,
                pnl=(h.get("got", 0) - h.get("spent", 0)) / 1e9 if h.get("sell_slot") else None)
cell = collections.defaultdict(list)
for m, d in S.items():
    if m in M and struct(d).startswith("separate"): cell[(d["op"], d["dev_fees"])].append((d["cs"], m, d))
for k in cell: cell[k].sort()
allruns = sorted((d["cs"], m) for m, d in S.items() if m in M)
rows = []
for k, v in cell.items():
    if len(v) < 6 or not 0.1 <= sum(x[2]["he"] for x in v) / len(v) <= 0.9: continue
    for i in range(1, len(v)):
        cs, m, d = v[i]
        past = [x for x in v[:i] if x[0] + 10 < cs]
        if not past: continue
        r = dict(he=d["he"], prev_he=v[i - 1][2]["he"])
        for f in ("v3", "v6", "v10", "v10x", "out", "peak"):
            for n in (1, 2, 3, 5):
                xs = [M[x[1]][f] for x in past[-n:]]; r[f"{f}_last{n}"] = st.mean(xs)
        r["dump_last1"] = M[past[-1][1]]["dump"]
        pn = [M[x[1]]["pnl"] for x in past if M[x[1]]["pnl"] is not None]; r["his_cum_pnl"] = sum(pn) if pn else 0
        j = bisect.bisect_left(allruns, (cs - 8500, "")); jj = bisect.bisect_left(allruns, (cs - 10, ""))
        mk = [M[mm]["v10x"] for s_, mm in allruns[j:jj]]; r["market_v10x_30min"] = st.mean(mk) if mk else 0
        rows.append(r)
print("launches", len(rows), "taken", sum(r["he"] for r in rows))
res = []
for f in [k for k in rows[0] if k not in ("he", "prev_he")]:
    xs = sorted(r[f] for r in rows); lo, hi = xs[len(xs) // 3], xs[2 * len(xs) // 3]
    for state in (0, 1):
        sub = [r for r in rows if r["prev_he"] == state]
        top = [r["he"] for r in sub if r[f] >= hi]; bot = [r["he"] for r in sub if r[f] <= lo]
        if len(top) >= 10 and len(bot) >= 10:
            res.append((f, state, sum(top) / len(top), len(top), sum(bot) / len(bot), len(bot)))
by = collections.defaultdict(dict)
for f, s, t, nt, b_, nb in res: by[f][s] = (t, nt, b_, nb)
for f, d in sorted(by.items(), key=lambda kv: -sum(x[0] - x[2] for x in kv[1].values())):
    print(f"   {f:22s} " + " | ".join(f"after a {'take' if s else 'skip'}: top {100*t:4.0f}% ({nt}) bottom {100*b_:4.0f}% ({nb})" for s, (t, nt, b_, nb) in sorted(d.items())))
