# A copy call the bot can make with no tweet feed, from its own tweet book: the tweet check reads every tool coin's
# metadata (its tweet link) about a second after the create; fetching that tweet's text once (fxtwitter) lets the hot
# loop match each later coin's ticker (3+) or name (4+) against the texts of the tweets earlier coins already used.
# A match = a copy (another coin on a story already taken). Variant B: match against the earlier coins' own tickers and
# names instead (no tweet text). Live "exit" policy and hold 10, net of 1.2%, 0.5 SOL; all first buyers and where WE
# were the first buyer (our real fills: the adverse selection).
import json, collections, sys, bisect
exec(open("/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/pm/tweetfeed.py").read().split("print(f\"tweets with text")[0])
LAT = float(sys.argv[1]) if len(sys.argv) > 1 else 3.0     # s from an earlier coin's create to its tweet text in the book
WIN = float(sys.argv[2]) if len(sys.argv) > 2 else 1800.0
print(f"tool coins (dev >= 0.95): {len(feats)}, with a tweet link: {sum(1 for r in feats if r['sid'])}, of them with the text: {sum(1 for r in feats if r['sid'] and int(r['sid']) in TW)}")
book = []   # (available_at, sid, coin keys)
callA, callB = {}, {}
avail = []  # sorted by available time
for r in feats:
    c = cr.get(r["mint"]); t = c["bt"] if c else r["bt"]
    sym, name = norm(r["sym"]), norm(r["name"])
    # matches against the book as of t
    hitA = hitB = False
    for at, sid, s2, n2 in reversed(avail):
        if at > t: continue
        if t - tms(sid) > WIN: continue
        b = blob.get(sid)
        if b and ((len(sym) >= 3 and sym in b) or (len(name) >= 4 and name in b)): hitA = True
        if (len(sym) >= 3 and sym == s2) or (len(name) >= 4 and name == n2): hitB = True
        if hitA and hitB: break
    callA[r["mint"]] = hitA; callB[r["mint"]] = hitB
    if r["sid"]:
        avail.append((t + LAT, int(r["sid"]), sym, norm(r["name"])))
        if len(avail) > 6000: avail = avail[-6000:]
def V(x):
    if not x["first_fresh"]:
        v = x["vC"].get(str(max(3, x["d"] + 1)))
        if v is not None: return v
    return x["vC"].get("10")
H10 = lambda x: x["vC"].get("10")
rows = [x for x in U.values() if x["vC"].get("10") is not None and x["mint"] in callA]
PER = [("all", lambda x: True), ("23 AM", lambda x: x["day"] == 23 and x["hour"] < 12), ("23 PM", lambda x: x["day"] == 23 and x["hour"] >= 12), ("24 AM", lambda x: x["day"] == 24)]
def show(name, f, val, sub=lambda x: True):
    out = []
    for lab, sel in PER:
        g = [x for x in rows if sel(x) and sub(x)]; sk = [x for x in g if f(x)]; kp = [x for x in g if not f(x)]
        vs = [val(x) - 0.012 for x in sk]; vk = [val(x) - 0.012 for x in kp]
        ba = sum(val(x) - 0.012 <= -0.3 for x in g); bs = sum(v <= -0.3 for v in vs)
        out.append(f"{lab}: {len(sk):3d} skip {100*sum(vs)/max(1,len(vs)):+6.2f}% kept {100*sum(vk)/max(1,len(vk)):+6.2f}% big {bs:2d}/{ba:2d} {0.5*-sum(vs):+6.2f}")
    print(f"   {name:30s} " + " | ".join(out))
for lab, f in (("A: in a used tweet's text", lambda x: callA[x["mint"]]), ("B: an earlier coin's name", lambda x: callB[x["mint"]])):
    g = rows; sk = [x for x in g if f(x)]
    print(f"== {lab}: calls {len(sk)} of {len(g)}; of them true copies {100*sum(not x['first_fresh'] for x in sk)/max(1,len(sk)):.0f}%; "
          f"catches {100*sum(not x['first_fresh'] for x in sk)/max(1,sum(not x['first_fresh'] for x in g)):.0f}% of the copies, {100*sum(x['first_fresh'] for x in sk)/max(1,sum(x['first_fresh'] for x in g)):.0f}% of the first coins")
    show("all first buyers, exit", f, V)
    show("all first buyers, hold 10", f, H10)
    show("WE were first, exit", f, V, lambda x: x["fo_who"] == "us")
    show("WE were first, hold 10", f, H10, lambda x: x["fo_who"] == "us")
