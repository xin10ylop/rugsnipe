# Where the big losses are under the live "exit" policy, and how many of them the tweet feed's copy skip ("on") removes,
# with every author followed and with the 5 / 50 most frequent authors (ranked on Oct 23 00-12). Net of 1.2%, 0.5 SOL.
import json, collections, sys
exec(open("/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/pm/rug_combo.py").read().split("train = lambda x")[0])
cnt = collections.Counter(x["author"] for x in U.values() if x["day"] == 23 and x["hour"] < 12 and x["author"])
top = {n: {a for a, _ in cnt.most_common(n)} for n in (5, 50)}
print("top 5 authors:", sorted(top[5]))
# feed calls with only the followed authors' posts
def calls(auth):
    used = set(); out = {}
    for r in feats:
        c = cr.get(r["mint"])
        if not c: continue
        ms = [s for s in matches(c, c["bt"], 0.4, 1800) if auth is None or ((TW[s].get("author") or {}).get("screen_name") or "").lower() in auth]
        if ms: out[r["mint"]] = "copy" if all(s in used for s in ms) else "first"
        for s in ms: used.add(s)
    return out
CALL = {"all authors": calls(None), "top 50": calls({a.lower() for a in top[50]}), "top 5": calls({a.lower() for a in top[5]})}
PER = [("all", lambda x: True), ("23 AM", lambda x: x["day"] == 23 and x["hour"] < 12), ("23 PM", lambda x: x["day"] == 23 and x["hour"] >= 12), ("24 AM", lambda x: x["day"] == 24)]
big = [x for x in rows if V(x) - 0.012 <= -0.3]
print(f"rule coins {len(rows)}, big losses under exit {len(big)}: copies {sum(not x['first_fresh'] for x in big)}, first coins {sum(x['first_fresh'] for x in big)}")
print("   big-loss copies: sold at +3 by the tweet check", sum(not x['first_fresh'] and x['d'] < 3 for x in big), "| the rest:", collections.Counter(x["fo_who"] for x in big))
def show(name, f):
    out = []
    for lab, sel in PER:
        g = [x for x in rows if sel(x)]; sk = [x for x in g if f(x)]
        vs = [V(x) - 0.012 for x in sk]; ba = sum(V(x) - 0.012 <= -0.3 for x in g); bs = sum(v <= -0.3 for v in vs)
        out.append(f"{lab}: {len(sk):3d} skip {100*sum(vs)/max(1,len(vs)):+6.2f}% big {bs:2d}/{ba:2d} {0.5*-sum(vs):+6.2f}")
    print(f"   {name:34s} " + " | ".join(out))
print("skip rule: coins skipped, their mean, big losses removed of all, P&L change (SOL)")
for k, c in CALL.items():
    show(f"feed copy call, {k}", lambda x, c=c: c.get(x["mint"]) == "copy")
for k, c in CALL.items():
    show(f"feed copy + dumper&rapid, {k}", lambda x, c=c: c.get(x["mint"]) == "copy" or (SIG["dumper"](x) and SIG["rapidlaunch"](x)))
