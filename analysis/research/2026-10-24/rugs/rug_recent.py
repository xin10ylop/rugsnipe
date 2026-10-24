# Copies arrive 20-150 s after the first coin on their tweet. Call a coin a copy when its ticker (3+) or name (4+) equals
# that of a tool coin created in the last W s (a different deployer or the same); no tweet text, no feed.
import json, collections, sys
exec(open("/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/pm/rug_combo.py").read().split("train = lambda x")[0])
H10 = lambda x: x["vC"].get("10")
PER = [("all", lambda x: True), ("23 AM", lambda x: x["day"] == 23 and x["hour"] < 12), ("23 PM", lambda x: x["day"] == 23 and x["hour"] >= 12), ("24 AM", lambda x: x["day"] == 24)]
def show(name, f, val, sub=lambda x: True):
    out = []
    for lab, sel in PER:
        g = [x for x in rows if sel(x) and sub(x)]; sk = [x for x in g if f(x)]; kp = [x for x in g if not f(x)]
        vs = [val(x) - 0.012 for x in sk]; vk = [val(x) - 0.012 for x in kp]
        ba = sum(val(x) - 0.012 <= -0.3 for x in g); bs = sum(v <= -0.3 for v in vs)
        out.append(f"{lab}: {len(sk):3d} skip {100*sum(vs)/max(1,len(vs)):+6.2f}% kept {100*sum(vk)/max(1,len(vk)):+5.2f}% big {bs:2d}/{ba:2d} {0.5*-sum(vs):+6.2f}")
    print(f"   {name:16s} " + " | ".join(out))
for W in (60, 150, 300):
    for mode in ("ticker", "ticker or name"):
        recent = collections.deque(); hit = {}
        for r in feats:
            c = cr.get(r["mint"]); t = c["bt"] if c else r["bt"]
            while recent and t - recent[0][0] > W: recent.popleft()
            s, n = norm(r["sym"]), norm(r["name"])
            hit[r["mint"]] = any((len(s) >= 3 and s == s2) or (mode != "ticker" and len(n) >= 4 and n == n2) for _, s2, n2 in recent)
            recent.append((t, s, n))
        f = lambda x, hit=hit: hit.get(x["mint"], False)
        sk = [x for x in rows if f(x)]
        print(f"== same {mode} as a tool coin in the last {W} s: calls {len(sk)}, copies {100*sum(not x['first_fresh'] for x in sk)/max(1,len(sk)):.0f}%, "
              f"catches {100*sum(not x['first_fresh'] for x in sk)/sum(not x['first_fresh'] for x in rows):.0f}% of copies, {100*sum(x['first_fresh'] for x in sk)/sum(x['first_fresh'] for x in rows):.0f}% of first coins")
        show("all, exit", f, V); show("WE first, exit", f, V, lambda x: x["fo_who"] == "us"); show("WE first, hold 10", f, H10, lambda x: x["fo_who"] == "us")
