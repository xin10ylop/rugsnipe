# What his tip depends on: tweet age at the create, the coin's rank among coins for the same tweet, his result on the
# earlier coins of that tweet (failed / landed), and the signer.
import json, collections, statistics as st, bisect
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
rows = json.load(open("big/conv_rows.json")); R = json.load(open("an_rows.json")); T = json.load(open("tweets.json"))
W = {w: json.load(open(f"walk_{w[:6]}.json"))["rows"] for w in (A, US)}
pairs = sorted((r["bt"], r["slot"]) for w in W.values() for r in w if r.get("bt")); ps = [p[1] for p in pairs]
def time_of(slot):
    i = bisect.bisect_left(ps, slot); i = min(max(i, 1), len(pairs) - 1)
    (t0, s0), (t1, s1) = pairs[i - 1], pairs[i]
    return t0 + (slot - s0) * (t1 - t0) / max(1, s1 - s0)
EP = 1288834974657
X = {x["m"]: x for x in rows}
tw = collections.defaultdict(list)
for x in sorted(rows, key=lambda x: R[x["m"]]["cs"]):
    sid = T.get(x["m"])
    if sid and sid > 0: tw[sid].append(x["m"])
out = []
for sid, ms in tw.items():
    tt = ((sid >> 22) + EP) / 1000
    for i, m in enumerate(ms):
        x = X[m]; age = time_of(R[m]["cs"]) - tt
        prev = [X[p] for p in ms[:i]]
        out.append(dict(m=m, age=age, rank=i + 1, n_prev_att=sum(p["him_att"] for p in prev), n_prev_fail=sum(p["him_att"] and not p["him_ok"] for p in prev),
                        n_prev_land=sum(p["him_ok"] for p in prev), prev_v10=max([p["v10"] or 0 for p in prev], default=None),
                        tip=x["him_tip"], att=x["him_att"], ok=x["him_ok"], v10=x["v10"], net=x["him_net"], pb=x["pb"], signer=x["signer"]))
json.dump(out, open("big/tipwhy_rows.json", "w"))
def tab(title, keyf, sel):
    g = collections.defaultdict(list)
    for o in sel: g[keyf(o)].append(o)
    print(f"\n== {title}")
    print(f"{'bucket':24} {'coins':>5} {'tried':>5} {'tip>=5M':>7} {'med tip':>7} {'landed':>6} {'v10':>7} {'v10 tried':>9} {'his net':>8}")
    for k in sorted(g, key=str):
        xs = g[k]; tr = [o for o in xs if o["att"]]
        v = [o["v10"] for o in xs if o["v10"] is not None]; vt = [o["v10"] for o in tr if o["v10"] is not None]
        print(f"{str(k):24} {len(xs):5d} {len(tr):5d} {sum(o['tip'] >= 5 for o in tr):7d} {st.median([o['tip'] for o in tr]) if tr else 0:7.1f} {sum(o['ok'] for o in xs):6d} "
              f"{100*st.mean(v) if v else 0:+6.1f}% {100*st.mean(vt) if vt else 0:+8.1f}% {sum(o['net'] for o in xs):+8.2f}")
def ageb(o):
    a = o["age"]
    return "a <30s" if a < 30 else "b 30-60s" if a < 60 else "c 1-2m" if a < 120 else "d 2-5m" if a < 300 else "e 5-15m" if a < 900 else "f 15-60m" if a < 3600 else "g >1h"
big = [o for o in out if o["pb"] > 40]
tab("above 40 vSOL with a tweet: tweet age at the create", ageb, big)
tab("above 40 vSOL with a tweet: rank of the coin within its tweet", lambda o: min(o["rank"], 5), big)
tab("above 40 vSOL with a tweet: his earlier tries on the same tweet", lambda o: f"prev att {min(o['n_prev_att'],3)} fail {min(o['n_prev_fail'],3)}", big)
tab("ALL with a tweet: tweet age", ageb, out)
tab("ALL with a tweet: rank", lambda o: min(o["rank"], 5), out)
