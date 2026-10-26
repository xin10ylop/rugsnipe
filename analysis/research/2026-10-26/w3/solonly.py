# The 3yrJE1A8 copy on SOL-quoted coins only. Its buys on coins priced in another token (bought through a swap; the
# trade event's SOL reserve is 0) are outside the bot's class, and eval.py valued them on a rebuilt SOL reserve. Sold at
# its buy + 29 (the live exit), by day and by its size, SOL coins against the others.
import collections
exec(open("eval.py").read().split("GRID = ")[0])
quote0 = {}; size = {}
for r in rows:
    for t in r.get("tr") or []:
        if t[1] == W and t[2] == 1 and t[0] not in quote0:
            quote0[t[0]] = t[5] <= 0; size[t[0]] = "2 SOL" if r["d"] < -1e9 else "0.5 SOL"
agg = collections.defaultdict(lambda: collections.defaultdict(float)); n = collections.Counter()
for m, g in G.items():
    ens = entries(m, g)
    if not ens or not g["buy_post"] or not g["states"].get("29"): continue
    pb = g["buy_post"][0] / g["buy_post"][1]; vs = []
    for en in ens:
        if (en[0] / en[1]) / pb - 1 > PREM: vs.append(-0.0001); continue
        v = pnl(en, g["states"]["29"])
        if v is not None: vs.append(v)
    if not vs: continue
    k = ("other quote" if quote0.get(m) else "SOL coin", size.get(m, "?"))
    agg[k][day(m)] += sum(vs) / len(vs); n[k] += 1
for k in sorted(agg):
    byd = agg[k]
    print(f"{k[0]:12} its {k[1]:8} copies {n[k]:3d}  total {sum(byd.values()):+6.2f}  " + " ".join(f"{d[3:]} {v:+.2f}" for d, v in sorted(byd.items())))
