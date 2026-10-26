# The copy of 3yrJE1A8 (eval.py's entry and exit model: our landing behind its buy, its +5% bound, 0.5 SOL, fees and
# tips) split by its own buy size (0.5 vs 2 SOL): is min_sol = 1.5 right at our prices, not only at its own?
import json, collections
exec(open("eval.py").read().split("GRID = ")[0])
size = {}
for r in rows:
    for t in r.get("tr") or []:
        if t[1] == W and t[2] == 1 and t[0] not in size:
            size[t[0]] = "2 SOL" if r["d"] < -1e9 else "0.5 SOL"
for k in ("24", "29"):
    agg = collections.defaultdict(lambda: [0, 0.0, 0, collections.defaultdict(float)])
    for m, g in G.items():
        ens = entries(m, g)
        if not ens or not g["buy_post"]:
            continue
        pb = g["buy_post"][0] / g["buy_post"][1]
        ex = g["states"].get(k)
        if not ex:
            continue
        vs = []
        for en in ens:
            if (en[0] / en[1]) / pb - 1 > PREM:
                vs.append(-0.0001); continue
            v = pnl(en, ex)
            if v is not None:
                vs.append(v)
        if not vs:
            continue
        v = sum(vs) / len(vs); a = agg[size.get(m, "?")]
        a[0] += 1; a[1] += v; a[2] += v > 0; a[3][day(m)] += v
    for s, (n, tot, w, byd) in sorted(agg.items()):
        print(f"sell at its buy + {k}: its {s:7s} copies {n:3d} wins {w:3d} total {tot:+6.2f} SOL  mean {100 * tot / n / STAKE:+5.1f}%  "
              + " ".join(f"{d[3:]}:{byd[d]:+.2f}" for d in sorted(byd)))
