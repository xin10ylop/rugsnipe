# Its exit rule: for each closed round trip (one buy, one sell), the price just before its sell against the price just
# after its buy (marginal, (vt_after_buy / vt_before_sell)^2), against its average buy price (vt_before_buy * vt_after_buy
# / vt_before_sell^2), its proceeds over its cost (SOL values), and the time held (slots, seconds). Clusters = its rule.
import sys, json, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = sys.argv[1]
T = json.load(open(f"{SP}/h9long/{W[:8]}_trips.json"))
R = []
for x in T:
    tr = x["tr"]
    if x["open"] or len(tr) != 2 or not tr[0]["buy"] or tr[1]["buy"]: continue
    b, s = tr
    if not (b["vt"] and s["vt"]): continue
    vb_after = b["vt"]; vb_before = b["vt"] + b["tok"]; vs_after = s["vt"]; vs_before = s["vt"] - s["tok"]
    if vs_before <= 0: continue
    marg = (vb_after / vs_before) ** 2; avg = vb_before * vb_after / vs_before ** 2
    val = -(s["v"] or 0) / b["v"] if b["v"] else None
    R.append(dict(kind=x["kind"], m=x["m"], marg=marg, avg=avg, val=val, slots=s["s"] - b["s"], sec=s["bt"] - b["bt"], net=x["net"], buy_sol=b["v"]))
json.dump(R, open(f"{SP}/h9long/{W[:8]}_exits.json", "w"))
def hist(xs, edges):
    c = collections.Counter()
    for v in xs:
        for e in edges:
            if v < e: c[e] += 1; break
        else: c["inf"] += 1
    return [(e, c[e]) for e in edges + ["inf"] if c[e]]
for kind in ("pump, other quote", "pump, SOL quote"):
    rs = [r for r in R if r["kind"] == kind]
    print(f"\n== {kind}: {len(rs)} one-buy one-sell trips")
    print("   seconds held 1700-2500", hist([r["sec"] for r in rs if 1700 <= r["sec"] < 2500], list(range(1750, 2550, 50))))
    print("   slots held for those", hist([r["slots"] for r in rs if 1700 <= r["sec"] < 2500], list(range(7000, 9000, 100))))
    tm = [r for r in rs if r["sec"] < 1700]
    print("   held < 1700 s: marginal price multiple", hist([r["marg"] for r in tm], [0.4, 0.5, 0.55, 0.6, 0.65, 0.7, 0.8, 1.0, 1.5, 1.8, 1.9, 2.0, 2.1, 2.2, 2.3, 2.5, 3]))
    print("   held < 1700 s: average-price multiple ", hist([r["avg"] for r in tm], [0.4, 0.5, 0.55, 0.6, 0.65, 0.7, 0.8, 1.0, 1.5, 1.8, 1.9, 2.0, 2.1, 2.2, 2.3, 2.5, 3]))
    print("   held < 1700 s: value multiple (SOL)   ", hist([r["val"] for r in tm if r["val"]], [0.4, 0.5, 0.55, 0.6, 0.65, 0.7, 0.8, 1.0, 1.5, 1.8, 1.9, 2.0, 2.1, 2.2, 2.3, 2.5, 3]))
    print("   seconds held, all", hist([r["sec"] for r in rs], [10, 30, 60, 120, 300, 600, 1200, 1800, 2400, 3000, 3300, 3400, 3450, 3500, 3550, 3600, 3700, 4000]))
