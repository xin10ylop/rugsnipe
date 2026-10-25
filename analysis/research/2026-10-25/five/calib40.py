import json, statistics as st, collections, sys
sys.argv = ["x", "big/auction40.json", "nogroups"]
exec(open("big/ev_bid2.py").read().split("print(f\"{'bid':>5}")[0])
rows = {x["m"]: x for x in json.load(open("big/conv_rows.json"))}
us = [m for m, x in AU.items() if x["win"] == "US" and x["us_ok"]]
pairs = [(AU[m]["us_net"], value(m, 0.49, AU[m]["win_bid"])) for m in us]
pairs = [(a, b) for a, b in pairs if b is not None]
print(f"our live wins at <=40 with a rebuilt race: {len(pairs)}  live net {sum(a for a,b in pairs):+.3f}  model {sum(b for a,b in pairs):+.3f}  (per win {st.mean(a for a,b in pairs):+.4f} vs {st.mean(b for a,b in pairs):+.4f})")
# our sends: predicted at our own bid vs actual
sent = [m for m, x in AU.items() if x.get("us_att")]
def ourbid(x):
    o = [l[1] for l in x["lost"] if l[0] == "US"]
    return x["win_bid"] if x["win"] == "US" else (max(o) if o else None)
c = collections.Counter()
for m in sent:
    x = AU[m]; b = ourbid(x)
    if b is None: c["no bid seen"] += 1; continue
    pred = b > other_max(x); act = x["win"] == "US"
    c[(pred, act)] += 1
print("our sends: (model says we win, we won) ->", dict(c))
# the live P&L by day for our wins vs model
byd = collections.defaultdict(lambda: [0, 0.0, 0.0])
for m in us:
    v = value(m, 0.49, AU[m]["win_bid"])
    if v is None: continue
    d = R[m]["day"]; byd[d][0] += 1; byd[d][1] += AU[m]["us_net"]; byd[d][2] += v
for d in sorted(byd): print(f"   {d}: wins {byd[d][0]:3d} live {byd[d][1]:+.3f} model {byd[d][2]:+.3f}")
