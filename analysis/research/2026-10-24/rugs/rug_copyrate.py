# Copies without a tweet feed: the tweet check knows, a second after each create, whether that coin was first on its
# tweet or a copy. A deployer (create signer) that mostly launches copies will likely launch a copy next. Each rule coin
# is judged by the signer's earlier tool coins (known 3 s after their creates): skip when their copy share is high.
# Live "exit" policy and hold 10, net of 1.2%, 0.5 SOL, by period; all first buyers and WE first.
import json, collections
exec(open("/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/pm/rug_combo.py").read().split("train = lambda x")[0])
H10 = lambda x: x["vC"].get("10")
past = collections.defaultdict(list); rec = {}
for r in feats:
    c = cr.get(r["mint"]); t = c["bt"] if c else r["bt"]; sg = c["signer"] if c else r["creator"]
    rec[r["mint"]] = [k for tt, k in past[sg] if tt + 3 < t]
    if r["sid"]: past[sg].append((t, (r["tweet_rank"] or 1) >= 2))
PER = [("all", lambda x: True), ("23 AM", lambda x: x["day"] == 23 and x["hour"] < 12), ("23 PM", lambda x: x["day"] == 23 and x["hour"] >= 12), ("24 AM", lambda x: x["day"] == 24)]
R = [x for x in rows if x["mint"] in rec]
def show(name, f, val, sub=lambda x: True):
    out = []
    for lab, sel in PER:
        g = [x for x in R if sel(x) and sub(x)]; sk = [x for x in g if f(x)]; kp = [x for x in g if not f(x)]
        vs = [val(x) - 0.012 for x in sk]; vk = [val(x) - 0.012 for x in kp]
        ba = sum(val(x) - 0.012 <= -0.3 for x in g); bs = sum(v <= -0.3 for v in vs)
        out.append(f"{lab}: {len(sk):3d} skip {100*sum(vs)/max(1,len(vs)):+6.2f}% kept {100*sum(vk)/max(1,len(vk)):+5.2f}% big {bs:2d}/{ba:2d} {0.5*-sum(vs):+6.2f}")
    print(f"   {name:24s} " + " | ".join(out))
for n, q in ((3, 0.5), (3, 0.67), (5, 0.6), (5, 0.8), (10, 0.7)):
    f = lambda x, n=n, q=q: len(rec[x["mint"]][-20:]) >= n and sum(rec[x["mint"]][-20:]) / len(rec[x["mint"]][-20:]) >= q
    sk = [x for x in R if f(x)]
    print(f"== signer's last 20 coins: {n}+ known, copies >= {q:.0%}: calls {len(sk)}, true copies {100*sum(not x['first_fresh'] for x in sk)/max(1,len(sk)):.0f}% "
          f"(base {100*sum(not x['first_fresh'] for x in R)/len(R):.0f}%), catches {100*sum(not x['first_fresh'] for x in sk)/max(1,sum(not x['first_fresh'] for x in R)):.0f}% of copies")
    show("all, exit", f, V); show("all, hold 10", f, H10); show("WE first, exit", f, V, lambda x: x["fo_who"] == "us")
