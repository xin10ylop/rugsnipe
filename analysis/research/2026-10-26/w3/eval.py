# The copy of 3yrJE1A8 sold at its buy slot + k (w3/grid.json). Entry: where our copy lands (the complete tapes: our
# landing distances behind its buy, capped at the next slot), else its post-buy price + 2%; refused (a failed buy) above
# its price + 5%. 0.5 SOL, 1.25% fee each way, 3M buy tip + 0.5M sell tip. By day and by its timer (20 or 30 slots).
import json, glob, os, collections, statistics as st, datetime
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
K = 30_000_000_000 * 1_073_000_000_000_000
FEE = 0.0125; STAKE = 0.5; COST = 0.003 + 0.0005 + 0.00002; PREM = 0.05
DEC = [32, 45, 58, 71, 84, 101, 125, 158, 200, 322]
G = json.load(open(SP + "/w3/grid.json"))
def fix(s): return None if not s or s[1] <= 0 else ((s[0] if s[0] > 0 else K // s[1]), s[1])   # a 0 SOL reserve: rebuilt from the tokens
for g in G.values():
    g["buy_post"] = fix(g.get("buy_post")); g["states"] = {k: fix(v) for k, v in g["states"].items() if fix(v)}
    if g.get("sell1_pre"): g["sell1_pre"] = fix(g["sell1_pre"])
rows = [json.loads(l) for l in open(SP + "/hold/log/3yrJE1A8.jsonl")]
W = next(t[1] for r in rows for t in r.get("tr") or [] if t[1].startswith("3yrJE1A8"))
bt = {}
for r in rows:
    for t in r.get("tr") or []:
        if t[1] == W and t[2] == 1 and r.get("bt"): bt.setdefault(t[0], r["bt"])
def day(m): return datetime.datetime.fromtimestamp(bt.get(m, 0), datetime.timezone.utc).strftime("%m-%d")
tapes = {}
for d_ in ("wide", "oos"):
    for p in glob.glob(f"{SP}/{d_}/coin/*.json"):
        m = os.path.basename(p)[:-5]
        if m in G and m not in tapes: tapes[m] = p
def entries(m, g):
    """our entry states (one per landing decile)"""
    pb = g["buy_post"]
    if not pb: return []
    p = tapes.get(m)
    if p:
        d = json.load(open(p)); rs = sorted(d["rows"], key=lambda r: (r[0], r[1]))
        i = next((j for j, r in enumerate(rs) if r[2] == W and r[3] == 1), None)
        if i is not None:
            b = rs[i]; out = []
            for D in DEC:
                en = (b[6] if b[6] > 0 else K // b[7], b[7])
                for y in rs[i + 1:]:
                    if y[0] == b[0] and y[1] > b[1] + D: break
                    if y[0] > b[0] and y[1] > D: break
                    if y[0] > b[0] + 1: break
                    if y[7] > 0: en = (y[6] if y[6] > 0 else K // y[7], y[7])
                out.append(en)
            return out
    v, t = pb; p_in = v / t * 1.02; v0 = (p_in * K) ** 0.5
    return [(v0, K / v0)]
def pnl(en, ex):
    v0, t0 = en; k = v0 * t0; s_in = STAKE * 1e9 / (1 + FEE); q = t0 - k / (v0 + s_in)
    vr, tr = ex
    if tr <= q: return None
    return vr * q / (tr - q) * (1 - FEE) / 1e9 - STAKE - COST
GRID = ["5", "9", "14", "17", "19", "24", "29"]
res = {k: collections.defaultdict(float) for k in GRID}; n = {k: 0 for k in GRID}; tm = {k: collections.defaultdict(float) for k in GRID}
tapes_used = 0; refused = 0
for m, g in G.items():
    ens = entries(m, g)
    if not ens: continue
    if m in tapes: tapes_used += 1
    pb = g["buy_post"][0] / g["buy_post"][1]
    timer = (g["sell1_slot"] - g["buy_slot"]) if g.get("sell1_slot") else None
    tl = "20" if timer in (19, 20, 21) else "30" if timer in (29, 30, 31) else "other"
    for k in GRID:
        ex = g["states"].get(k)
        if not ex: continue
        vs = []
        for en in ens:
            if (en[0] / en[1]) / pb - 1 > PREM: vs.append(-0.0001); continue
            v = pnl(en, ex)
            if v is not None: vs.append(v)
        if not vs: continue
        v = sum(vs) / len(vs); res[k][day(m)] += v; n[k] += 1; tm[k][tl] += v
print(f"coins {len(G)}, with our landing from a tape {tapes_used}")
days = sorted({day(m) for m in G})
for k in GRID:
    print(f"  sell at its buy + {k:>2} slots: copies {n[k]:4d}  total {sum(res[k].values()):+6.2f} SOL  " + " ".join(f"{d[3:]}:{res[k][d]:+.2f}" for d in days) +
          "   | its timer 20: {:+.2f}  30: {:+.2f}  other: {:+.2f}".format(tm[k]['20'], tm[k]['30'], tm[k]['other']))
# adaptive: sell right after its first sell when that comes before buy + H (its sell's post-trade state, then one more
# slot of others' trades is not modelled: optimistic by the trades behind it), else at buy + H
post1 = {}
for r in rows:
    for t in r.get("tr") or []:
        if t[1] == W and t[2] == 0 and t[0] not in post1 and t[6] > 0:
            post1[t[0]] = (r["s"], ((t[5] if t[5] > 0 else K // t[6]), t[6]))
for H in ("24", "29"):
    byd = collections.defaultdict(float); n_ = 0
    for m, g in G.items():
        ens = entries(m, g)
        if not ens or not g["buy_post"]: continue
        pb = g["buy_post"][0] / g["buy_post"][1]
        s1 = post1.get(m)
        ex = s1[1] if (s1 and s1[0] <= g["buy_slot"] + int(H)) else g["states"].get(H)
        if not ex: continue
        vs = []
        for en in ens:
            if (en[0] / en[1]) / pb - 1 > PREM: vs.append(-0.0001); continue
            v = pnl(en, ex)
            if v is not None: vs.append(v)
        if vs: byd[day(m)] += sum(vs) / len(vs); n_ += 1
    print(f"  adaptive (after its sell, else buy + {H}): copies {n_}  total {sum(byd.values()):+6.2f}  " + " ".join(f"{d[3:]}:{byd[d]:+.2f}" for d in days))
