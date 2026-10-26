# What picks its 20-slot vs 30-slot timer? Per coin: the timer, its buy size, create + 0/1, and the launch's signer
# (operator) and host from the complete tapes (wide/coin, oos/coin). Is the timer a property of the operator?
import json, glob, os, collections
exec(open("eval.py").read().split("GRID = ")[0])
cre = {}
for d_ in ("wide", "oos"):
    for p in glob.glob(f"{SP}/{d_}/coin/*.json"):
        m = os.path.basename(p)[:-5]
        if m in G and m not in cre:
            j = json.load(open(p))
            if j.get("create"): cre[m] = (j["create"]["signer"], j["create"].get("host"), j.get("cs"))
size = {}
for r in rows:
    for t in r.get("tr") or []:
        if t[1] == W and t[2] == 1 and t[0] not in size: size[t[0]] = "2" if r["d"] < -1e9 else "0.5"
def timer(g):
    d = g["sell1_slot"] - g["buy_slot"]
    return "t20" if 18 <= d <= 22 else "t30" if 28 <= d <= 32 else "early" if d <= 12 else "other"
by_op = collections.defaultdict(collections.Counter); by_size = collections.defaultdict(collections.Counter)
by_host = collections.defaultdict(collections.Counter); by_at = collections.defaultdict(collections.Counter)
seq = collections.defaultdict(list)
for m, g in sorted(G.items(), key=lambda kv: kv[1]["buy_slot"]):
    if not g.get("sell1_slot"): continue
    t = timer(g); by_size[size.get(m)][t] += 1
    if m in cre:
        s, h, cs = cre[m]; by_op[s][t] += 1; by_host[h][t] += 1; by_at[g["buy_slot"] - cs][t] += 1; seq[s].append(t)
print("by its size:", {k: dict(v) for k, v in by_size.items()})
print("by its buy slot - create:", {k: dict(v) for k, v in sorted(by_at.items())})
print("coins with a tape:", sum(sum(v.values()) for v in by_op.values()), "operators:", len(by_op))
pure = mixed = 0; pn = mn = 0
for s, c in sorted(by_op.items(), key=lambda kv: -sum(kv[1].values()))[:25]:
    n20, n30 = c["t20"], c["t30"]
    if n20 + n30 >= 3:
        if min(n20, n30) == 0: pure += 1; pn += n20 + n30
        else: mixed += 1; mn += n20 + n30
    print(f"   {s[:8]} {dict(c)}  sequence {' '.join(x[1:] for x in seq[s][:24])}")
print(f"operators with 3+ timer coins: one timer only {pure} ({pn} coins), both {mixed} ({mn} coins)")
print("by host (top):", {k: dict(v) for k, v in sorted(by_host.items(), key=lambda kv: -sum(kv[1].values()))[:6]})
