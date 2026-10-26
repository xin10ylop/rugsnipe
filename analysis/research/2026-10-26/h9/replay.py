# Wallet 4 / 6 copies replayed on the long tapes (h9/coin): every in-class launch buy of the wallet at create + 0/1.
# Our landing: the live landings of our copies of this wallet (h9/land.json), the 10 nearest to the wallet's block index
# (a buy late in its block puts us in the next slot), each moved onto the coin; our price bound PREM over its post-trade
# price, else refused (the fee only). Our 0.5 SOL buy and sell: 1.25% each way, 4.5M of tips and fees.
# Exits: create + h (h <= 40: every tape reaches create + 40), the wallet's first sell + 1 slot, its last sell + 1.
# Periods: P1 Oct 25 18:00 - Oct 26 06:00 UTC, P2 06:00 - 17:44 UTC (before the bot watched it), P3 live (17:44 on).
#   python3 -I replay.py WALLET [PREM ...]
import json, sys, os, bisect, statistics as st
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = sys.argv[1]; w8 = W[:8]
PREMS = [float(x) for x in sys.argv[2:]] or [0.05]
FEE = 0.0125; STAKE = 0.5; COST = 0.0045; REFUSED = -0.000015
P1, P2 = 1792994400, {"H9kuqGKN": 1793036640, "D5BDDkBt": 1793036160}[w8]
D = json.load(open(f"{SP}/h9/{w8}_coins.json")); own = D["own"]; meta = D["coins"]
LAND = json.load(open(f"{SP}/h9/land.json"))
land = sorted(LAND["H9kuqGKN"] + LAND["D5BDDkBt"], key=lambda x: x["his_i"])   # both wallets' live landings
def landings(his_i, k=10):
    return sorted(land, key=lambda x: abs(x["his_i"] - his_i))[:k]
def pnl(entry, ex):
    v0, t0 = entry; k = v0 * t0; s_in = STAKE * 1e9 / (1 + FEE); q = t0 - k / (v0 + s_in)
    vr, tr = ex
    if tr <= q: return None
    return vr * q / (tr - q) * (1 - FEE) / 1e9 - STAKE - COST
def state_before(seq, key):
    """reserves after the last trade before position key = (slot, idx)"""
    i = bisect.bisect_left(seq, key, key=lambda r: (r[0], r[1]))
    return (seq[i - 1][6], seq[i - 1][7]) if i else None
def state_end(seq, slot):
    return state_before(seq, (slot + 1, -1))
EXITS = [3, 5, 7, 9, 12, 15, 20, 25, 30, 40]
rows = []
for m, mt in meta.items():
    if not mt.get("in_class") or mt.get("at", 9) > 1: continue
    p = f"{SP}/h9/coin/{m}.json"
    c = json.load(open(p)); seq = c.get("rows") or []
    if not seq: continue
    ts = own[m]; b = ts[0]
    x = next((r for r in seq if r[2] == W and r[3] == 1 and r[0] == b["s"]), None)
    if not x: continue
    cs = c["cs"]; wp = x[6] / x[7]
    seen = set(); its = 0
    for t in ts:
        if (t["s"], t["i"]) in seen: continue
        seen.add((t["s"], t["i"])); its += t["d"]
    held = sum(t["tok"] * (1 if t["buy"] else -1) for t in ts)
    bought = sum(t["tok"] for t in ts if t["buy"])
    sells = [t["s"] for t in ts if not t["buy"]]
    r = dict(m=m, bt=b["bt"], per=1 if b["bt"] < P1 else (2 if b["bt"] < P2 else 3), its=its / 1e9,
             closed=held <= 1e-3 * bought, its_sol=b["sol"] / 1e9, first_sell=(sells[0] - cs) if sells else None,
             last_sell=(max(sells) - cs) if sells else None, end=c["end"] - cs, x=(x[0], x[1]))
    for prem in PREMS:
        for name in [f"c{h}" for h in EXITS] + ["first", "last", "first40"]:
            vs = []
            for L in landings(x[1]):
                pos = (x[0], x[1] + (L["our_i"] - L["his_i"])) if L["dslot"] == 0 else (x[0] + L["dslot"], L["our_i"])
                en = state_before(seq, pos)
                if not en: continue
                if (en[0] / en[1]) / wp - 1 > prem: vs.append(REFUSED); continue
                if name.startswith("c"): s = cs + int(name[1:])
                elif name == "first": s = (sells[0] + 1) if sells else c["end"]
                elif name == "last": s = (max(sells) + 1) if sells else c["end"]
                else: s = min((sells[0] + 1) if sells else cs + 40, cs + 40)
                s = max(s, pos[0] + 1)
                if s > c["end"]: s = c["end"]
                ex = state_end(seq, s - 1)
                v = pnl(en, ex) if ex else None
                if v is not None: vs.append(v)
            r[f"{prem}:{name}"] = sum(vs) / len(vs) if vs else None
            r[f"{prem}:{name}:ref"] = sum(1 for v in vs if v == REFUSED) / len(vs) if vs else None
    rows.append(r)
json.dump(rows, open(f"{SP}/h9/{w8}_replay.json", "w"))
def S(rs, k): return sum(r[k] or 0 for r in rs)
print(f"== {w8}: {len(rows)} in-class launch buys at create + 0/1 on the tapes (P1 {sum(r['per'] == 1 for r in rows)}, "
      f"P2 {sum(r['per'] == 2 for r in rows)}, P3 live {sum(r['per'] == 3 for r in rows)})")
for per in (1, 2, 3):
    rs = [r for r in rows if r["per"] == per]
    print(f"   P{per}: its own net on them {sum(r['its'] for r in rs if r['closed']):+.2f} ({sum(not r['closed'] for r in rs)} still held); "
          f"its first sell after create: median {st.median([r['first_sell'] for r in rs if r['first_sell'] is not None]) if any(r['first_sell'] is not None for r in rs) else '-'}")
for prem in PREMS:
    print(f"\n   our copy, bound +{prem * 100:.0f}% (SOL per period; refused share):")
    print("   exit      " + "  ".join(f"{'P' + str(p):>14}" for p in (1, 2, 3)) + "     P1+P2")
    for name in [f"c{h}" for h in EXITS] + ["first", "last", "first40"]:
        k = f"{prem}:{name}"
        cells = []
        for per in (1, 2, 3):
            rs = [r for r in rows if r["per"] == per]
            ref = st.mean([r[k + ":ref"] for r in rs if r[k + ":ref"] is not None]) if rs else 0
            cells.append(f"{S(rs, k):+7.2f} ({ref * 100:2.0f}%)")
        lab = {"first": "its 1st sell+1", "last": "its last sell+1", "first40": "1st sell, <=c40"}.get(name, "create+" + name[1:])
        print(f"   {lab:15} " + "  ".join(f"{c:>14}" for c in cells) + f"   {S([r for r in rows if r['per'] < 3], k):+7.2f}")
