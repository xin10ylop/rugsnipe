# The 3yrJE1A8 copy (eval.py's model, sold at its buy + 29): a second try after a refusal, at the end of its buy slot + k
# on the complete tapes, under a ceiling of its price + C. Only refused copies (some landing deciles over +5%) try again.
import json, collections
exec(open("eval.py").read().split("GRID = ")[0])
def end_state(m, slot):
    p = tapes.get(m)
    if not p: return None
    d = json.load(open(p)); s = None
    for r in sorted(d["rows"], key=lambda r: (r[0], r[1])):
        if r[0] > slot: break
        if r[7] > 0: s = (r[6] if r[6] > 0 else K // r[7], r[7])
    return s
base = 0.0; ref = []
for m, g in G.items():
    ens = entries(m, g)
    if not ens or not g["buy_post"] or not g["states"].get("29"): continue
    pb = g["buy_post"][0] / g["buy_post"][1]; ex = g["states"]["29"]
    vs = []; nref = 0
    for en in ens:
        if (en[0] / en[1]) / pb - 1 > PREM: vs.append(-0.0001); nref += 1; continue
        v = pnl(en, ex)
        if v is not None: vs.append(v)
    if vs: base += sum(vs) / len(vs)
    if nref and m in tapes: ref.append((m, g, nref / len(ens)))
print(f"the copy now: {base:+.2f} SOL; refused copies with a tape: {len(ref)}")
for k in (1, 2, 3):
    for C in (0.10, 0.25, 9.0):
        add = 0.0; n = 0; byd = collections.defaultdict(float)
        for m, g, share in ref:
            en = end_state(m, g["buy_slot"] + k); pb = g["buy_post"][0] / g["buy_post"][1]
            if not en or (en[0] / en[1]) / pb - 1 > C: continue
            v = pnl(en, g["states"]["29"])
            if v is None: continue
            add += share * v; n += 1; byd[day(m)] += share * v
        print(f"   second try at its slot + {k}, ceiling {'none' if C > 1 else f'+{C:.0%}':>5}: tries {n:3d}  adds {add:+.2f}  "
              + " ".join(f"{d[3:]} {v:+.2f}" for d, v in sorted(byd.items())))
