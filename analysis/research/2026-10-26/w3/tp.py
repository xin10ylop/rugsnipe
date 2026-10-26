# (1) Is its early sell a take profit? Its gain at its sell on the early coins, and the timer coins that passed that
# gain at a sampled slot before their own sell without selling. (2) A take profit for our copy: sell at the first
# sampled slot (buy + 5/9/14/17/19/24) where the price is X above its post-buy price, else at buy + 29 (eval.py's model).
import json, collections
exec(open("eval.py").read().split("GRID = ")[0])
def px(s): return s[0] / s[1] if s else None
early, viol = [], collections.Counter()
for m, g in G.items():
    if not g["buy_post"] or not g.get("sell1_slot"): continue
    d = g["sell1_slot"] - g["buy_slot"]; p0 = px(g["buy_post"])
    if d <= 12 and g.get("sell1_pre"): early.append(px(g["sell1_pre"]) / p0 - 1)
    elif 18 <= d <= 32:
        mx = max([px(v) / p0 - 1 for k, v in g["states"].items() if int(k) < d] or [-1])
        for X in (0.3, 0.4, 0.45, 0.5, 0.6):
            if mx >= X: viol[X] += 1
early.sort()
print("early sells: its gain at the sell, sorted:", " ".join(f"{x:+.0%}" for x in early))
n_t = sum(1 for g in G.values() if g["buy_post"] and g.get("sell1_slot") and 18 <= g["sell1_slot"] - g["buy_slot"] <= 32)
print(f"timer coins ({n_t}) that were up at least X at a sampled slot before their sell, and did not sell:", dict(viol))
size = {}
for r in rows:
    for t in r.get("tr") or []:
        if t[1] == W and t[2] == 1 and t[0] not in size: size[t[0]] = "2" if r["d"] < -1e9 else "0.5"
KS = ["5", "9", "14", "17", "19", "24"]
for X in (None, 0.2, 0.3, 0.4, 0.5, 0.6):
    byd = collections.defaultdict(float); bys = collections.defaultdict(float); n = 0; hit = 0
    for m, g in G.items():
        ens = entries(m, g)
        if not ens or not g["buy_post"]: continue
        p0 = px(g["buy_post"])
        ex = None
        if X is not None:
            for k in KS:
                s = g["states"].get(k)
                if s and px(s) / p0 - 1 >= X: ex = s; hit += 1; break
        ex = ex or g["states"].get("29")
        if not ex: continue
        vs = []
        for en in ens:
            if (en[0] / en[1]) / p0 - 1 > PREM: vs.append(-0.0001); continue
            v = pnl(en, ex)
            if v is not None: vs.append(v)
        if vs:
            v = sum(vs) / len(vs); byd[day(m)] += v; bys[size.get(m, "?")] += v; n += 1
    lab = "none (buy + 29)" if X is None else f"+{X:.0%}"
    print(f"take profit {lab:16} copies {n} (hit {hit})  total {sum(byd.values()):+.2f}  " + " ".join(f"{d[3:]}:{byd[d]:+.2f}" for d in sorted(byd))
          + "  | its 0.5 SOL " + f"{bys['0.5']:+.2f}" + "  2 SOL " + f"{bys['2']:+.2f}")
