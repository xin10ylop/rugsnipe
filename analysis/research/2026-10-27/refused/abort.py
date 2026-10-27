# The ghost fills on all his buys; when his buy turns out unkeyed (tip < 2M, size < 2 SOL; known the moment we see
# his transaction), sell at once: our sale lands one or two slots after his buy. What do those aborted fills cost?
# His other buys (the 300 sampled, scaled to 1,109), ghost entry j from presend.json, P_AFTER 0.8, post-bundle + 10%.
import json, collections
exec(open("/home/user/rugsnipe/analysis/research/2026-10-27/refused/eng.py").read())
J = json.load(open(SP + "/refused/presend.json"))["js"]; JP = {j: c / len(J) for j, c in collections.Counter(J).items()}
REST = {m for m, _ in json.load(open(SP + "/refused/items_rest300.json"))}
P_AFTER = 0.8; CAP = 0.10
tot = collections.defaultdict(float); n = 0; fills = 0.0
for r in rows:
    if r["m"] not in REST or r["tip"] >= 2e6 or r["sol"] >= 2.0: continue
    Lg = long_seq(r["m"])
    if not Lg: continue
    seq = Lg[0]; pb = post_bundle(coins[r["m"]], seq)
    if not pb: continue
    b = his_buy[r["m"]]; hk = (b["slot"], b["idx"]); after = [x for x in seq if (x[0], x[1]) > hk]
    s0 = st_before(seq, (hk[0], hk[1] + 1)); n += 1
    for j, p in JP.items():
        s = s0 if j == 0 else ((after[j - 1][2], after[j - 1][3]) if j <= len(after) else None)
        if not s or (s[0] / s[1]) / (pb[0] / pb[1]) - 1 > CAP: continue
        fills += P_AFTER * p
        for h in (1, 2, 3):
            ex = st_end(seq, b["slot"] + h)
            v = pnl_s(s, ex, 0.5) if ex else None
            if v is not None: tot[h] += P_AFTER * p * (v - BIAS)
sc = 1109 / max(n, 1)
print(f"his other buys sampled {n} (x {sc:.2f}); ghost fills on them {fills * sc:.0f} over 5 days")
for h, v in sorted(tot.items()): print(f"   sold {h} slot(s) after his buy: {v * sc:+.2f} SOL over 5 days ({v / max(fills, 1e-9):+.4f} a fill)")
