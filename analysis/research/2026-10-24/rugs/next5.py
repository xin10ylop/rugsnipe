# The 5 accounts to add to the feed: greedy, each step the author whose posts add the most correct copy calls on the
# rule coins (live "exit" value, net 1.2%), the feed simulated at a 0.4 s lag and a 30 min window. Current: the 5 below.
import json, collections, sys
exec(open("/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/pm/tweetfeed.py").read().split("print(f\"tweets with text")[0])
au = lambda sid: (((TW.get(sid) or {}).get("author") or {}).get("screen_name") or "").lower()
CUR = {"devvvooooor", "igetsjiggy6767", "thedev1738", "ddddddd8a", "lilbruhfish"}
def V(x):
    if not x["first_fresh"]:
        v = x["vC"].get(str(max(3, x["d"] + 1)))
        if v is not None: return v
    return x["vC"].get("10")
pre = [(r, cr.get(r["mint"])) for r in feats]
pre = [(r, c, matches(c, c["bt"], 0.4, 1800)) for r, c in pre if c]
def calls(auth):
    used = set(); out = {}
    for r, c, ms in pre:
        ms = [s for s in ms if au(s) in auth]
        if ms: out[r["mint"]] = "copy" if all(s in used for s in ms) else "first"
        for s in ms: used.add(s)
    return out
rows = [x for x in U.values() if x["vC"].get("10") is not None]
def score(auth, sel=lambda x: True):
    c = calls(auth); g = [x for x in rows if sel(x) and c.get(x["mint"]) == "copy"]
    right = sum(not x["first_fresh"] for x in g)
    return len(g), right, sum(V(x) - 0.012 <= -0.3 for x in g), (sum(V(x) - 0.012 for x in g) / len(g) if g else 0)
cand = collections.Counter(au(int(r["sid"])) for r in feats if r["sid"] and int(r["sid"]) in TW)
cand = [a for a, n in cand.most_common(120) if a and a not in CUR]
base = score(CUR)
print(f"current 5: copy calls {base[0]}, right {base[1]}, big losses among them {base[2]}, their value {100*base[3]:+.2f}%")
cur = set(CUR); picks = []
for step in range(8):
    best = None
    for a in cand:
        if a in cur: continue
        n, right, big, m = score(cur | {a})
        gain = right - score(cur)[1] - 0.5 * (n - right - (score(cur)[0] - score(cur)[1]))
        if best is None or gain > best[0]: best = (gain, a, n, right, big, m)
    cur.add(best[1]); picks.append(best[1])
    n24 = score(cur, lambda x: x["day"] == 24)
    print(f"+ {best[1]:18s} copy calls {best[2]:3d}, right {best[3]:3d} ({100*best[3]/max(1,best[2]):.0f}%), big {best[4]}, value {100*best[5]:+.2f}% | Oct 24 alone: calls {n24[0]}, right {n24[1]}")
