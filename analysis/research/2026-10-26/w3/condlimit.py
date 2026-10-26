# The 3yrJE1A8 copy (eval.py's model, sold at its buy + 29): what a 7.5% or 10% limit adds over 5%, by its buy size
# (the live copy takes its 2 SOL buys only) and by day.
import collections
src = open("eval.py").read().split("GRID = ")[0]
def run(P):
    exec(src.replace("PREM = 0.05", f"PREM = {P}"), globals())
    out = {}
    for m, g in G.items():
        ens = entries(m, g)
        if not ens or not g["buy_post"] or not g["states"].get("29"): continue
        pb = g["buy_post"][0] / g["buy_post"][1]; vs = []
        for en in ens:
            if (en[0] / en[1]) / pb - 1 > PREM: vs.append(-0.0001); continue
            x = pnl(en, g["states"]["29"])
            if x is not None: vs.append(x)
        if vs: out[m] = sum(vs) / len(vs)
    return out
b5 = run(0.05); b75 = run(0.075); b10 = run(0.10)
size = {}
for r in rows:
    for t in r.get("tr") or []:
        if t[1] == W and t[2] == 1 and t[0] not in size: size[t[0]] = "2 SOL" if r["d"] < -1e9 else "0.5 SOL"
for lab, bb in (("7.5%", b75), ("10%", b10)):
    agg = collections.defaultdict(lambda: collections.defaultdict(float))
    for m in b5:
        agg[size.get(m, "?")][day(m)] += bb.get(m, 0) - b5[m]
    for s, byd in sorted(agg.items()):
        print(f"limit {lab:4} its {s:7}: adds {sum(byd.values()):+.3f}  " + " ".join(f"{d[3:]} {v:+.3f}" for d, v in sorted(byd.items())))
