# A sharper dumper list: the bot marks every insider wallet of a dumped launch for good, after one dump. Here a wallet
# is judged on its record: launches it was an insider of whose outcome was known before this create (the dump shows
# by create + 3 slots), how many of them it dumped (50% of the insiders' tokens within 3 slots), how recent.
# Live "exit" policy, net of 1.2%, 0.5 SOL stakes, scored per period.
import json, collections, sys
exec(open("/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/pm/rug_rules.py").read().split("def V(x, pol):")[0])
def V(x):
    if not x["first_fresh"]:
        v = x["vC"].get(str(max(3, x["d"] + 1)))
        if v is not None: return v
    return x["vC"].get("10")
hist = collections.defaultdict(list)   # wallet -> [(known_at, dumped, bt)]
feat = {}
for x in coins:
    d = bool(dumped(x, 3, 0.5)); f = {}
    recs = {u: [r for r in hist[u] if r[0] < x["bt"]] for u in x["ins"]}
    sg = x["signer"]
    def agg(us):
        n = max((len(recs[u]) for u in us), default=0)
        best = 0.0; nd = 0; last = False; rec = None; nmin = 0
        for u in us:
            rs = recs[u]
            if not rs: continue
            k = sum(r[1] for r in rs); nd = max(nd, k)
            if len(rs) >= 2: best = max(best, k / len(rs))
            if rs[-1][1]: last = True
            t = max((r[2] for r in rs if r[1]), default=None)
            if t is not None: rec = t if rec is None else max(rec, t)
        return dict(n=n, rate=best, nd=nd, last=last, since=(x["bt"] - rec) if rec is not None else None)
    f["all"] = agg(x["ins"]); f["sg"] = agg({sg} & x["ins"] or {sg})
    f["frac_marked"] = sum(1 for u in x["ins"] if any(r[1] for r in recs[u])) / max(1, len(x["ins"]))
    feat[x["m"]] = f
    for u in x["ins"]: hist[u].append((x["bt"] + 1.2, d, x["bt"]))
rows = [x for x in U.values() if x["vC"].get("10") is not None and x["mint"] in feat and x["mint"] in flags]
PER = [("all", lambda x: True), ("23 AM", lambda x: x["day"] == 23 and x["hour"] < 12), ("23 PM", lambda x: x["day"] == 23 and x["hour"] >= 12), ("24 AM", lambda x: x["day"] == 24)]
def show(name, f):
    out = []
    for lab, sel in PER:
        g = [x for x in rows if sel(x)]; sk = [x for x in g if f(x)]
        vs = [V(x) - 0.012 for x in sk]; ba = sum(V(x) - 0.012 <= -0.3 for x in g); bs = sum(v <= -0.3 for v in vs)
        out.append(f"{lab}: {len(sk):3d} skip {100*sum(vs)/max(1,len(vs)):+6.2f}% big {bs:2d}/{ba:2d} {0.5*-sum(vs):+6.2f}")
    print(f"   {name:44s} " + " | ".join(out))
F = lambda x: feat[x["mint"]]
print("skip rule: coins skipped, their mean, big losses removed of all, P&L change (SOL)")
show("bot list (any insider dumped once, ever)", lambda x: flags[x["mint"]][(3, 0.5)])
for k in (2, 3, 4): show(f"an insider dumped >= {k} launches", lambda x, k=k: F(x)["all"]["nd"] >= k)
for r in (0.5, 0.67, 0.8, 1.0): show(f"an insider's dump rate >= {r:.0%} (2+ known)", lambda x, r=r: F(x)["all"]["rate"] >= r)
show("an insider's last known launch was a dump", lambda x: F(x)["all"]["last"])
for h in (600, 1800, 3600, 3 * 3600): show(f"an insider dumped in the last {h//60} min", lambda x, h=h: F(x)["all"]["since"] is not None and F(x)["all"]["since"] <= h)
show("the signer dumped once", lambda x: F(x)["sg"]["nd"] >= 1)
show("the signer dumped >= 2", lambda x: F(x)["sg"]["nd"] >= 2)
show("the signer's last launch was a dump", lambda x: F(x)["sg"]["last"])
for r in (0.5, 0.67, 1.0): show(f"the signer's dump rate >= {r:.0%}", lambda x, r=r: F(x)["sg"]["rate"] >= r)
for q in (0.5, 0.75, 1.0): show(f"share of insiders marked >= {q:.0%}", lambda x, q=q: F(x)["frac_marked"] >= q)
show("last was a dump AND rapidlaunch", lambda x: F(x)["all"]["last"] and "rapidlaunch" in x["host"])
show("bot list AND rapidlaunch", lambda x: flags[x["mint"]][(3, 0.5)] and "rapidlaunch" in x["host"])
