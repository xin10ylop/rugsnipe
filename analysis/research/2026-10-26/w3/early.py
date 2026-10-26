# The 3yrJE1A8 copy (eval.py's model) sold right after its first sell when that comes within E slots of its buy (its
# take-profit on a fast spike), else at its buy + 29 (the timer copy): does following only its early sells help?
import collections
exec(open("eval.py").read().split("GRID = ")[0])
post1 = {}
for r in rows:
    for t in r.get("tr") or []:
        if t[1] == W and t[2] == 0 and t[0] not in post1 and t[6] > 0:
            post1[t[0]] = (r["s"], ((t[5] if t[5] > 0 else K // t[6]), t[6]))
size = {}
for r in rows:
    for t in r.get("tr") or []:
        if t[1] == W and t[2] == 1 and t[0] not in size:
            size[t[0]] = "2" if r["d"] < -1e9 else "0.5"
for E in (0, 9, 12, 14, 17):
    byd = collections.defaultdict(float); bys = collections.defaultdict(float); n = 0; early = 0
    for m, g in G.items():
        ens = entries(m, g)
        if not ens or not g["buy_post"]:
            continue
        pb = g["buy_post"][0] / g["buy_post"][1]
        s1 = post1.get(m)
        follow = bool(E) and s1 and s1[0] <= g["buy_slot"] + E
        ex = s1[1] if follow else g["states"].get("29")
        if not ex:
            continue
        early += bool(follow)
        vs = []
        for en in ens:
            if (en[0] / en[1]) / pb - 1 > PREM:
                vs.append(-0.0001); continue
            v = pnl(en, ex)
            if v is not None:
                vs.append(v)
        if vs:
            v = sum(vs) / len(vs); byd[day(m)] += v; bys[size.get(m, "?")] += v; n += 1
    lab = "never (now: buy + 29)" if not E else f"its sell within {E} slots"
    print(f"follow {lab:24} copies {n} (followed {early})  total {sum(byd.values()):+.2f}  "
          + " ".join(f"{d[3:]}:{byd[d]:+.2f}" for d in sorted(byd)) + "  | by its size " + " ".join(f"{k}:{v:+.2f}" for k, v in sorted(bys.items())))
