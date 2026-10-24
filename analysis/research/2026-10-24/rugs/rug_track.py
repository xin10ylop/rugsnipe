# Track records: each rule coin is judged by the first-outside-buyer outcomes (value at create+10) of the earlier rule
# coins of the same deployer (create signer) or the same launch template (tool, dev buy, bundle wallets, post-bundle),
# known 5 s after each earlier create. Skip when the record is bad. Live "exit" policy, net of 1.2%, 0.5 SOL, by period.
import json, collections
exec(open("/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/pm/rug_combo.py").read().split("train = lambda x")[0])
H10 = lambda x: x["vC"].get("10")
tpl = lambda x: (x["host"], round(x["dev"], 1), x["n_bw"], round(x["pb_sol"]))
KEYS = {"signer": lambda x: x["signer"], "template": tpl, "signer+template": lambda x: (x["signer"],) + tpl(x)}
order = sorted(rows, key=lambda x: x["bt"])
rec = {k: {} for k in KEYS}
for kn, kf in KEYS.items():
    past = collections.defaultdict(list)
    for x in order:
        h = [(t, v) for t, v in past[kf(x)] if t + 5 < x["bt"]]
        rec[kn][x["mint"]] = h
        past[kf(x)].append((x["bt"], H10(x)))
PER = [("all", lambda x: True), ("23 AM", lambda x: x["day"] == 23 and x["hour"] < 12), ("23 PM", lambda x: x["day"] == 23 and x["hour"] >= 12), ("24 AM", lambda x: x["day"] == 24)]
def show(name, f):
    out = []
    for lab, sel in PER:
        g = [x for x in rows if sel(x)]; sk = [x for x in g if f(x)]
        vs = [V(x) - 0.012 for x in sk]; ba = sum(V(x) - 0.012 <= -0.3 for x in g); bs = sum(v <= -0.3 for v in vs)
        out.append(f"{lab}: {len(sk):3d} skip {100*sum(vs)/max(1,len(vs)):+6.2f}% big {bs:2d}/{ba:2d} {0.5*-sum(vs):+6.2f}")
    print(f"   {name:52s} " + " | ".join(out))
print("skip rule: coins skipped, their mean, big losses removed of all, P&L change (SOL)")
for kn in KEYS:
    R = rec[kn]
    for n in (3, 5):
        for last in (5, 10):
            show(f"{kn}: mean of last {last} (n>={n}) < 0", lambda x, n=n, last=last: len(R[x["mint"]]) >= n and sum(v for _, v in R[x["mint"]][-last:]) / len(R[x["mint"]][-last:]) < 0)
    for k in (1, 2):
        show(f"{kn}: {k}+ big losses in the last 10", lambda x, k=k: sum(v <= -0.3 for _, v in R[x["mint"]][-10:]) >= k)
    show(f"{kn}: last one a big loss", lambda x: bool(R[x["mint"]]) and R[x["mint"]][-1][1] <= -0.3)
    show(f"{kn}: big-loss rate >= 20% (n>=5)", lambda x: len(R[x["mint"]]) >= 5 and sum(v <= -0.3 for _, v in R[x["mint"]]) / len(R[x["mint"]]) >= 0.2)
