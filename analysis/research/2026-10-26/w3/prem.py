# The 3yrJE1A8 copy (eval.py's landing model) at wider price limits, sold at its buy + 29: does the limit cut winners?
import collections
src = open("eval.py").read().split("GRID = ")[0]
for P in (0.05, 0.10, 0.15, 0.25, 0.5, 9.0):
    exec(src.replace("PREM = 0.05", f"PREM = {P}"))
    byd = collections.defaultdict(float); n = 0; filled = 0.0
    for m, g in G.items():
        ens = entries(m, g)
        if not ens or not g["buy_post"]: continue
        pb = g["buy_post"][0] / g["buy_post"][1]; ex = g["states"].get("29")
        if not ex: continue
        vs = []; f = 0
        for en in ens:
            if (en[0] / en[1]) / pb - 1 > PREM: vs.append(-0.0001); continue
            v = pnl(en, ex)
            if v is not None: vs.append(v); f += 1
        if vs: byd[day(m)] += sum(vs) / len(vs); n += 1; filled += f / len(ens)
    print(f"limit {'none' if P > 1 else f'{P:.0%}':>5}: copies {n} filled {filled:.0f}  total {sum(byd.values()):+.2f}  " + " ".join(f"{d[3:]}:{byd[d]:+.2f}" for d in sorted(byd)))
