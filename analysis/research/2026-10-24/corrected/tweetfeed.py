# His Twitter use, rebuilt: a tweet feed that delivers each tweet LAG s after it is posted, and a coin whose ticker or name
# appears in a tweet posted in the last WIN s (text, author or quoted tweet), with no earlier coin matched to that tweet,
# is called "first". Measured on the rule coins (bt_rows.json, hold 10, net 1.2%): how many it calls, how often it is
# right, what they are worth, and his own attempt rate on them. Tweets: only those behind a coin (the real feed has more,
# so false matches against coin-less tweets are not counted here).
import json, os, re, collections, glob, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
TW = {}
for f in os.listdir(SP + "/univ/tweets"):
    try: j = json.load(open(SP + "/univ/tweets/" + f))
    except Exception: continue
    if j.get("code") == 200: TW[int(f[:-5])] = j
norm = lambda s: re.sub(r"[^a-z0-9]", "", (s or "").lower())
tms = lambda sid: ((sid >> 22) + 1288834974657) / 1000
blob = {sid: norm(" ".join([t.get("text") or "", (t.get("author") or {}).get("screen_name") or "", ((t.get("quote") or {}) or {}).get("screen_name") or ""])) for sid, t in TW.items()}
U = {x["mint"]: x for x in json.load(open(SP + "/pm/bt_rows.json"))}
feats = [r for d in (23, 24) for r in json.load(open(SP + f"/pm/dataset_{d}.json")) if r["host"] in ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io") and r["dev"] >= 0.95 and not (d == 24 and r["hour"] >= 12)]
feats.sort(key=lambda r: (r["slot"]))
times = sorted((tms(s), s) for s in TW)
import bisect
def matches(c, t_create, lag, win):
    sym, name = norm(c["symbol"]), norm(c["name"])
    lo = bisect.bisect_left(times, (t_create - win, 0)); hi = bisect.bisect_right(times, (t_create - lag, 1 << 63))
    out = []
    for tt, sid in times[lo:hi]:
        b = blob[sid]
        if (len(sym) >= 3 and sym in b) or (len(name) >= 4 and name in b): out.append(sid)
    return out
def run(lag, win):
    used = set(); res = []
    for r in feats:
        c = cr.get(r["mint"])
        if not c: continue
        ms = [s for s in matches(c, c["bt"], lag, win)]
        fresh = [s for s in ms if s not in used]
        called = bool(fresh)
        for s in ms: used.add(s)
        x = U.get(r["mint"])
        if x is not None: res.append((called, x["first_fresh"], x["vC"].get("10"), x["he"], x["delay"]))
    return res
print(f"tweets with text: {len(TW)}")
for lag, win in ((1, 10), (2, 10), (2, 15), (3, 15), (2, 30)):
    res = [r for r in run(lag, win) if r[2] is not None]
    called = [r for r in res if r[0]]; rest = [r for r in res if not r[0]]
    m = lambda g: 100 * (sum(r[2] for r in g) / max(1, len(g)) - 0.012)
    prec = sum(r[1] for r in called) / max(1, len(called)); rec = sum(r[1] for r in called) / max(1, sum(r[1] for r in res))
    print(f"feed lag {lag}s, window {win:2d}s: called first {len(called):4d} of {len(res)} ({len(called)/36:.1f}/h)  truly first {100*prec:3.0f}%  "
          f"finds {100*rec:3.0f}% of the first coins  value {m(called):+5.2f}% vs the rest {m(rest):+5.2f}%  he attempted {100*sum(r[3] for r in called)/max(1,len(called)):3.0f}% vs {100*sum(r[3] for r in rest)/max(1,len(rest)):3.0f}%")
