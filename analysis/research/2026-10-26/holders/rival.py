# The rival CWiNFA on the complete tapes (wide/coin, Oct 23 00:00 - Oct 25 18:00): its launch buys, its own result per
# coin inside the tape (sells - buys, fees in; what it still holds marked at create + 12), split by whether he (AHwJ)
# landed a buy on the coin, his position against the rival's, the rival's first-sell slot, and by the create signer's
# heat for him (his landed launch buys on the signer in the previous 24 h, from his walk).
import json, glob, collections, statistics as st, datetime, bisect
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; R = "CWiNFAEk4M1y6SjvnZkmxWDmhqSmnKFcnop1WoDW3S2J"
K = 30_000_000_000 * 1_073_000_000_000_000; FEE = 0.0125
walk = json.load(open(SP + "/dec/five/walk_AHwJSn.json"))["rows"]
cr = {}
for p in glob.glob(SP + "/dec/five/coin/*.json"):
    d = json.load(open(p))
    if d.get("create"): cr[d["mint"]] = d["create"]["signer"]
land_t = collections.defaultdict(list); his_try = collections.defaultdict(list)
for r in walk:
    if r["kind"] == "buy":
        his_try[r["m"]].append(r)
        if r["ok"] and r["m"] in cr: land_t[cr[r["m"]]].append(r["bt"])
for v in land_t.values(): v.sort()
def heat(s, t): v = land_t.get(s, []); return bisect.bisect_left(v, t) - bisect.bisect_left(v, t - 86400)
PAIRS = sorted({(r["slot"], r["bt"]) for r in walk if r.get("bt")}); PS = [p[0] for p in PAIRS]
def bt_of(slot):
    i = min(max(bisect.bisect_left(PS, slot), 1), len(PS) - 1)
    (s0, t0), (s1, t1) = PAIRS[i - 1], PAIRS[i]
    return int(t0 + (slot - s0) * (t1 - t0) / max(1, s1 - s0))
G = collections.defaultdict(lambda: [0, 0.0, 0]); offs = collections.Counter(); rel = collections.Counter(); days = collections.defaultdict(float)
for p in glob.glob(SP + "/wide/coin/*.json"):
    d = json.load(open(p))
    if not d.get("create") or not d.get("rows"): continue
    cs = d["cs"]; rows = sorted(d["rows"], key=lambda r: (r[0], r[1]))
    mine = [r for r in rows if r[2] == R]
    b = next((r for r in mine if r[3] == 1), None)
    if not b or b[0] > cs + 1: continue
    sol = 0; tok = 0
    for r in mine:
        if r[3] == 1: sol -= r[4] + r[8]; tok += r[5]
        else: sol += r[4] - r[8]; tok -= r[5]
    last = rows[-1]; mark = (last[6] if last[6] > 0 else K // last[7]) * tok / (last[7] + tok) * (1 - FEE) if tok > 0 and last[7] > 0 else 0
    net = (sol + mark) / 1e9
    s1 = next((r for r in mine if r[3] == 0), None)
    if s1: offs[s1[0] - cs] += 1
    hb = next((r for r in rows if r[2] == A and r[3] == 1), None)
    if hb:
        rel["he landed before it" if (hb[0], hb[1]) < (b[0], b[1]) else "it landed before him"] += 1
        g = "he landed a buy"
    elif d["mint"] in his_try: g = "he tried, failed"
    else: g = "he did not try"
    t = bt_of(cs); h = heat(d["create"]["signer"], t)
    hk = "hot (>= 2)" if h >= 2 else "1" if h == 1 else "0"
    for k in (g, "signer heat " + hk, "all"):
        G[k][0] += 1; G[k][1] += net; G[k][2] += net > 0
    days[datetime.datetime.fromtimestamp(t, datetime.timezone.utc).strftime("%m-%d")] += net
print("the rival's launch buys on the complete tapes, its own result inside the tape (no tips)")
for k, (n, v, w) in sorted(G.items(), key=lambda kv: -kv[1][0]):
    print(f"  {k:22} coins {n:5d}  net {v:+7.2f} SOL  per coin {v / n:+.4f}  wins {w / n:4.0%}")
print("  by day: " + "  ".join(f"{k} {v:+.2f}" for k, v in sorted(days.items())))
print("  its first sell at create +", sorted(offs.most_common(6)))
print("  against him:", dict(rel))
