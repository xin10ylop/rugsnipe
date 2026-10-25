# Inside last night's half-taken (operator, fee routing) cells, the launches with separate bundle transactions: (1) the
# bundle size against the operator's usual, dev buy, cadence, hour (temporal split and pooled rates); (2) a global
# throttle (his recent P&L, how busy he is); (3) the bundle wallets' funding sources; (4) the operator's recent coins'
# value against his next take. Run from SP/dec/cell after prebuy.py, bfund.py and cell_val.py.
import json, collections, statistics as st, bisect
exec(open("prebuy_cmp.py").read().split("cell = collections.defaultdict(list)")[0])
exec(open("structure.py").read().split("cell = collections.defaultdict(list)")[0])
R = {r["mint"]: r for r in json.load(open("launch.json"))}
ops = json.load(open("../night/ops.json")); V = json.load(open("val.json")); BF = json.load(open("bfund.json"))
him = json.load(open("../night/him.json")); att = json.load(open("../night/att.json"))
cell = collections.defaultdict(list)
for m, d in S.items(): cell[(d["op"], d["dev_fees"])].append((d["cs"], m, d))
for k in cell: cell[k].sort()
for k, v in cell.items():
    seen = []; prev = None
    for cs, m, d in v:
        sz = round(sum(ops[d["op"]][m]["sizes"]), 2); med = st.median(seen) if seen else None
        d["x_dev"] = R.get(m, {}).get("dev"); d["x_bsz"] = sz
        d["x_bsz_dev"] = None if med is None else ("same" if abs(sz - med) <= 0.05 * med else ("bigger" if sz > med else "smaller"))
        d["x_gap"] = None if prev is None else cs - prev
        seen.append(sz); prev = cs
mixed = {k: [x for x in v if struct(x[2]).startswith("separate")] for k, v in cell.items() if len(v) >= 6 and 0.1 <= sum(x[2]["he"] for x in v) / len(v) <= 0.9}
mixed = {k: v for k, v in mixed.items() if len(v) >= 6}
print("(1) mixed cells", len(mixed), "launches", sum(len(v) for v in mixed.values()))
for f, key in (("bundle size vs usual", "x_bsz_dev"),):
    c = collections.defaultdict(lambda: [0, 0])
    for v in mixed.values():
        for _, m, d in v: c[d[key]][0] += 1; c[d[key]][1] += d["he"]
    print(f"   {f}: " + " | ".join(f"{k}: {100*h/n:.0f}% of {n}" for k, (n, h) in c.items()))
closed = sorted((h["sell_slot"], (h["got"] - h["spent"]) / 1e9) for h in him.values() if h.get("sell_slot") and h.get("buy_slot"))
cl = [c for c, _ in closed]; aslots = sorted(a["slot"] for a in att); T = collections.defaultdict(lambda: [0, 0])
for v in mixed.values():
    for cs, m, d in v:
        i = bisect.bisect_left(cl, cs - 8500); j = bisect.bisect_left(cl, cs); pnl = sum(p for _, p in closed[i:j])
        g = "recent P&L <= -0.05" if pnl <= -0.05 else ("recent P&L >= +0.05" if pnl >= 0.05 else "recent P&L flat")
        T[g][0] += 1; T[g][1] += d["he"]
print("(2) " + " | ".join(f"{k}: {100*h/n:.0f}% of {n}" for k, (n, h) in sorted(T.items())))
print("(3) funding sources, taken vs skipped, per cell:")
for k, v in mixed.items():
    src = lambda m: "+".join(sorted({r[1][:6] for r in BF[m] if r[1]})) or "none"
    a = collections.Counter(src(m) for _, m, d in v if d["he"]); b_ = collections.Counter(src(m) for _, m, d in v if not d["he"])
    print(f"   {k[0][:8]} fees {k[1]}: taken {dict(a.most_common(3))} skipped {dict(b_.most_common(3))}")
T = collections.defaultdict(lambda: [0, 0])
for k, v in cell.items():
    v = [x for x in v if struct(x[2]).startswith("separate")]
    if len(v) < 6 or not 0.1 <= sum(x[2]["he"] for x in v) / len(v) <= 0.9: continue
    for i in range(3, len(v)):
        prev = [p for p in (V.get(v[j][1], {}).get("10") for j in range(i - 3, i)) if p is not None]
        if len(prev) < 2: continue
        pv = st.mean(prev); state = "after a take" if v[i - 1][2]["he"] else "after a skip"
        g = ">= +10%" if pv >= 0.10 else ("<= -5%" if pv <= -0.05 else "between")
        T[(state, g)][0] += 1; T[(state, g)][1] += v[i][2]["he"]
print("(4) " + " | ".join(f"{s} {g}: {100*h/n:.0f}% of {n}" for (s, g), (n, h) in sorted(T.items())))
