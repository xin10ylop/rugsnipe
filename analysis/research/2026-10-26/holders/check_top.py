# Checks on the top every-day holders: (1) where our copy really lands: on the complete tapes, the curve price D
# positions after their buy (our deciles; next slot when the slot ends first) over the price right after their buy;
# (2) when they buy (slots after the create) and how often they share coins (one trader on several wallets?);
# (3) whether the coins' creators got SOL from them (insiders).
import json, glob, os, collections, statistics as st
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
K = 30_000_000_000 * 1_073_000_000_000_000
DEC = [32, 45, 58, 71, 84, 101, 125, 158, 200, 322]
TOP = ["B3zRbxte", "9rbokPEP", "Dg3oFnB4", "4QYcEYZ1", "HAmnM7in", "Et1fXddA", "13yfeA3m", "GB7NgXoi", "5o5vGxrL", "M9VtmEkb", "3yrJE1A8", "DN9LKrJf", "6F1kzJ3U"]
WSOL = "So11111111111111111111111111111111111111112"
logs = {}; full = {}
for k in TOP:
    p = f"{SP}/hold/log/{k}.jsonl"
    if not os.path.exists(p): continue
    rows = [json.loads(l) for l in open(p)]
    w = next((t[1] for r in rows for t in r.get("tr") or [] if t[1].startswith(k)), None)
    logs[k] = rows; full[k] = w
coins = {}
for k, rows in logs.items():
    s = set()
    for r in rows:
        for m, v in (r.get("tok") or {}).items():
            if m != WSOL and v > 0: s.add(m)
    coins[k] = s
tapes = {}
for d_ in (SP + "/wide/coin", SP + "/oos/coin"):
    for p in glob.glob(d_ + "/*.json"):
        m = os.path.basename(p)[:-5]
        if any(m in c for c in coins.values()) and m not in tapes:
            d = json.load(open(p))
            if d.get("rows") and d.get("create"): tapes[m] = d
print(f"{'wallet':9} {'coins':>5} {'taped':>5} {'buy slot after create (share)':>32} {'our entry over their price: p25 / median / p75':>48}")
for k, w in full.items():
    offs = collections.Counter(); prem = []
    for m in coins[k]:
        d = tapes.get(m)
        if not d: continue
        rows = sorted(d["rows"], key=lambda r: (r[0], r[1])); cs = d["cs"]
        i = next((j for j, r in enumerate(rows) if r[2] == w and r[3] == 1), None)
        if i is None: continue
        b = rows[i]; offs[min(b[0] - cs, 5)] += 1
        if b[7] <= 0: continue
        pb = (b[6] if b[6] > 0 else K // b[7]) / b[7]
        for D in DEC:
            en = None
            for y in rows[i:]:
                if (y[0], y[1]) > (b[0], b[1] + D) and y[0] >= b[0]:
                    if y[0] == b[0]: break
                    if y[1] > D: break          # next slot: up to D positions into it
                if y[7] > 0: en = (y[6] if y[6] > 0 else K // y[7]) / y[7]
            if en: prem.append(en / pb - 1)
    n = sum(offs.values())
    q = sorted(prem)
    sh = " ".join(f"+{o}:{v / n:.0%}" for o, v in sorted(offs.items())) if n else "-"
    pq = f"{q[len(q) // 4]:+6.1%} / {q[len(q) // 2]:+6.1%} / {q[3 * len(q) // 4]:+6.1%}" if q else "-"
    print(f"{k:9} {len(coins[k]):5d} {n:5d} {sh:>32} {pq:>48}")
print("shared coins between them:")
ks = list(full)
for a in ks:
    row = [f"{b[:6]} {len(coins[a] & coins[b])}" for b in ks if b != a and len(coins[a] & coins[b]) > 0]
    print(f"  {a}: " + (", ".join(row) if row else "none"))
# insiders: creators of their coins funded by them (standalone transfers)
print("standalone SOL sent to other wallets, and whether any of those created a coin they bought:")
for k, rows in logs.items():
    w = full[k]; sent = collections.Counter()
    for r in rows:
        if r.get("tr") or r.get("pix") or r.get("calls"): continue
        for s, d, lam in r.get("xf") or []:
            if s == w: sent[d] += lam
    crs = {tapes[m]["create"]["signer"] for m in coins[k] if m in tapes} | {tapes[m]["create"]["creator"] for m in coins[k] if m in tapes}
    print(f"  {k}: sent {sum(sent.values()) / 1e9:.2f} SOL to {len(sent)} wallets; creators among them: {len(set(sent) & crs)}")
