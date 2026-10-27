# How it trades a coin: buys per coin, size, tip, hold to its first and last sell, sells per coin, the price multiple at
# its sells (on the curve, (vt at buy / vt at sell)^2), and how the result depends on each.   python3 -I behave.py WALLET
import sys, json, statistics as st, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = sys.argv[1]
T = json.load(open(f"{SP}/h9long/{W[:8]}_trips.json"))
def q(xs, ps=(10, 25, 50, 75, 90)):
    xs = sorted(xs); return [round(xs[min(len(xs) - 1, int(p / 100 * len(xs)))], 3) for p in ps] if xs else []
for kind in ("pump, other quote", "pump, SOL quote"):
    xs = [x for x in T if x["kind"] == kind and x["tr"] and x["tr"][0]["buy"] and x["net"] is not None and not x["open"]]
    print(f"\n== {kind}: {len(xs)} closed coins whose first trade is a buy, net {sum(x['net'] for x in xs):+.2f}")
    nb = [sum(t["buy"] for t in x["tr"]) for x in xs]; ns = [sum(1 - t["buy"] for t in x["tr"]) for x in xs]
    size = [-(x["tr"][0]["v"] or 0) for x in xs]
    tips = [x["tr"][0]["tip"] / 1e6 for x in xs]
    h1 = [next((t["s"] for t in x["tr"] if not t["buy"]), None) for x in xs]
    h1 = [s - x["tr"][0]["s"] for s, x in zip(h1, xs) if s]
    hl = [max(t["s"] for t in x["tr"] if not t["buy"]) - x["tr"][0]["s"] for x in xs if any(not t["buy"] for t in x["tr"])]
    print("   buys per coin p10..p90", q(nb), " sells per coin", q(ns), " first-buy SOL", q(size), " tip M", q(tips))
    print("   slots to first sell", q(h1), " to last sell", q(hl))
    print("   ix of first buy", collections.Counter(x["tr"][0]["ix"] for x in xs).most_common(4))
    # price multiple at the first sell, from vt
    mult = []
    for x in xs:
        b = x["tr"][0]; s = next((t for t in x["tr"] if not t["buy"]), None)
        if s and b["vt"] and s["vt"]: mult.append(((b["vt"] / s["vt"]) ** 2, x["net"], s["s"] - b["s"], x))
    print("   price at its first sell / after its first buy", q([m[0] for m in mult]))
    ret = [x["net"] / max(size[i], 0.01) for i, x in enumerate(xs)]
    print("   its net / first-buy SOL", q(ret), " mean", round(st.mean(ret), 3), " won", sum(1 for r in ret if r > 0), "/", len(ret))
    # by hold to first sell
    g = collections.defaultdict(list)
    for m, n, h, x in mult:
        g[("<=2", "3-10", "11-50", "51-200", "201-1000", ">1000")[(h > 2) + (h > 10) + (h > 50) + (h > 200) + (h > 1000)]].append(n)
    for k in ("<=2", "3-10", "11-50", "51-200", "201-1000", ">1000"):
        if k in g: print(f"   first sell after {k:>8s} slots: coins {len(g[k]):5d} net {sum(g[k]):+8.2f} won {sum(1 for v in g[k] if v > 0)}")
    g = collections.defaultdict(list)
    for s_, x in zip(size, xs):
        g[("<0.6", "0.6-1.2", "1.2-1.6", "1.6-2.5", ">2.5")[(s_ >= 0.6) + (s_ >= 1.2) + (s_ >= 1.6) + (s_ >= 2.5)]].append(x["net"])
    for k in ("<0.6", "0.6-1.2", "1.2-1.6", "1.6-2.5", ">2.5"):
        if k in g: print(f"   first buy {k:>8s} SOL: coins {len(g[k]):5d} net {sum(g[k]):+8.2f} won {sum(1 for v in g[k] if v > 0)}")
