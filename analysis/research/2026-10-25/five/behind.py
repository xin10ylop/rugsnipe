# Copy his conviction once it is on chain: he took the first spot; his tip is in his transaction. Buy behind him at
# the end of his slot (or one slot later), sell at +9/+10. Value from that entry, by his tip level, on all coins he
# took first (any pb), and on the >40 set.
import json, collections, statistics as st
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"
R = json.load(open("an_rows.json")); rows = {x["m"]: x for x in json.load(open("big/conv_rows.json"))}
B = json.load(open("bids.json"))
W = json.load(open("walk_AHwJSn.json"))["rows"]
first = {}
for r in sorted(W, key=lambda r: (r["slot"], r["idx"])):
    if r["kind"] == "buy" and r["ok"] and r.get("fill") and r["m"] not in first: first[r["m"]] = r
K = 30_000_000_000 * 1_073_000_000_000_000
def px(v, t): return v / t if v > 0 else K / (t * t)
res = []
for m, r in R.items():
    if (r.get("first") or [None])[0] != A or m not in first: continue
    d = json.load(open(f"coin/{m}.json")); cs = d["cs"]
    hb = first[m]; tip = (B.get(hb["sig"]) or [0])[0] / 1e6
    # price at the end of his slot (last trade of that slot), and at the end of the next slot
    def p_end(sl):
        xs = [x for x in d["rows"] if x[0] <= sl and x[7] > 0]
        return px(xs[-1][6], xs[-1][7]) if xs else None
    s0 = hb["slot"]
    pe0 = p_end(s0); pe1 = p_end(s0 + 1)
    p9 = p_end(cs + 9); p10 = p_end(cs + 10)
    if not (pe0 and pe1 and p9 and p10): continue
    res.append(dict(m=m, pb=r.get("pb") or 0, tip=tip, sol=hb["fill"][0] / 1e9, v10=r["v10"], r0=p9 / pe0 - 1, r1=p9 / pe1 - 1, hold_left=cs + 9 - s0, net=rows[m]["him_net"] if m in rows else 0))
def tb(t): return "a <=1M" if t <= 1.01 else "b 1-4M" if t <= 4 else "c 4-8M" if t <= 8 else "d 8-16M" if t <= 16 else "e >16M"
for lab, sel in (("all pb", res), ("pb > 40", [x for x in res if x["pb"] > 40]), ("pb <= 40", [x for x in res if x["pb"] <= 40])):
    g = collections.defaultdict(list)
    for x in sel: g[tb(x["tip"])].append(x)
    print(f"\n== he took the first spot ({lab}): buy behind him, sell at create+9")
    print(f"{'his tip':10} {'coins':>5} {'his size':>8} {'v10 (his)':>9} {'entry end of his slot':>22} {'median':>7} {'entry one slot later':>21} {'median':>7} {'his net':>8}")
    for k in sorted(g):
        xs = g[k]
        print(f"{k:10} {len(xs):5d} {st.mean(x['sol'] for x in xs):8.2f} {100*st.mean(x['v10'] for x in xs):+8.1f}% {100*st.mean(x['r0'] for x in xs):+21.1f}% {100*st.median(x['r0'] for x in xs):+6.1f}% "
              f"{100*st.mean(x['r1'] for x in xs):+20.1f}% {100*st.median(x['r1'] for x in xs):+6.1f}% {sum(x['net'] for x in xs):+8.2f}")
