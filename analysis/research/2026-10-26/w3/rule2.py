# Its early sell as a take profit on what its position would fetch (its tokens sold into the curve at that state, less
# the 1.25% fee) over what it paid, not on the price: the early sells at their sell, and the timer coins' best sampled
# value before their own sell.
import json, collections
exec(open("eval.py").read().split("GRID = ")[0])
pos = {}
for r in rows:
    for t in r.get("tr") or []:
        if t[1] == W and t[2] == 1 and t[0] not in pos:
            pos[t[0]] = (t[4], -r["d"])            # its tokens, its SOL out (fees, tip in)
def fetch(s, q):
    v, t = s; k = v * t
    if q >= t: return 0
    return (v - k / (t + q)) * (1 - FEE)
early, timer = [], []
for m, g in G.items():
    if m not in pos or not g.get("sell1_slot"): continue
    q, cost = pos[m]; d = g["sell1_slot"] - g["buy_slot"]
    vals = {int(k): fetch(s, q) / cost - 1 for k, s in g["states"].items()}
    if d <= 12 and g.get("sell1_pre"):
        early.append((fetch(g["sell1_pre"], q) / cost - 1, cost))
    elif 18 <= d <= 32:
        before = [v for k, v in vals.items() if k < d]
        if before: timer.append((max(before), cost, d))
print("early sells, what its position fetched at its sell over its cost:")
print("   ", " ".join(f"{v:+.0%}{'(2)' if c > 1e9 else ''}" for v, c in sorted(early)))
for T in (0.2, 0.25, 0.3, 0.35, 0.4):
    n = sum(1 for v, c, d in timer if v >= T)
    n2 = sum(1 for v, c, d in timer if v >= T and c > 1e9)
    print(f"timer coins ({len(timer)}) whose position was worth >= +{T:.0%} at a sampled slot before their sell: {n} ({n2} of them 2 SOL)")
print()
print("the take profit only inside its first ~12 slots? timer coins worth >= T at buy + 5 or + 9 (before any early sell):")
early_k = collections.Counter()
for m, g in G.items():
    if m not in pos or not g.get("sell1_slot"): continue
    d = g["sell1_slot"] - g["buy_slot"]
    if d <= 12: early_k[d] += 1
print("   early sells at buy +", dict(sorted(early_k.items())))
for T in (0.25, 0.3, 0.35, 0.4, 0.45):
    n = 0; n2 = 0; tot = 0
    for m, g in G.items():
        if m not in pos or not g.get("sell1_slot"): continue
        q, cost = pos[m]; d = g["sell1_slot"] - g["buy_slot"]
        if not (18 <= d <= 32): continue
        tot += 1
        v = max([fetch(g["states"][k], q) / cost - 1 for k in ("5", "9") if k in g["states"]] or [-1])
        if v >= T: n += 1; n2 += cost > 1e9
    e = sum(1 for v, c in early if v >= T)
    print(f"   T +{T:.0%}: timer coins above it at +5/+9: {n} of {tot} ({n2} of them 2 SOL); early sells at or above it: {e} of {len(early)}")
