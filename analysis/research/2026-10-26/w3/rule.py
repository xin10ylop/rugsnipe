# 3yrJE1A8's exit decoded from the curve: its gain (price / its post-buy price) at buy + k against when it sold
# (early <= 12 slots, ~20, ~30). Is the timer a price rule (take profit, stop, or a check that picks 20 or 30)?
import json, collections, statistics as st
exec(open("eval.py").read().split("GRID = ")[0])
def px(s): return s[0] / s[1] if s else None
grp = collections.defaultdict(list)
for m, g in G.items():
    if not g["buy_post"] or not g.get("sell1_slot"):
        continue
    d = g["sell1_slot"] - g["buy_slot"]
    k = "early<=12" if d <= 12 else "t20" if 18 <= d <= 22 else "t30" if 28 <= d <= 32 else "other"
    p0 = px(g["buy_post"]); r = {kk: px(v) / p0 - 1 for kk, v in g["states"].items()}
    r["at_sell"] = px(g.get("sell1_pre")) / p0 - 1 if g.get("sell1_pre") else None
    r["d"] = d
    grp[k].append(r)
def q(xs):
    xs = sorted(x for x in xs if x is not None)
    if not xs: return "-"
    return f"p25 {xs[len(xs) // 4]:+.0%} med {xs[len(xs) // 2]:+.0%} p75 {xs[3 * len(xs) // 4]:+.0%}"
for k in ("early<=12", "t20", "t30", "other"):
    rs = grp[k]
    print(f"{k}: {len(rs)} coins")
    for kk in ("5", "9", "14", "17", "19", "at_sell"):
        print(f"   gain at {'buy+' + kk if kk != 'at_sell' else 'its sell'}: {q([r.get(kk) for r in rs])}")
json.dump({k: v for k, v in grp.items()}, open(SP + "/w3/rule_groups.json", "w"))
