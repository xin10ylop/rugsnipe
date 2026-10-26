# How the rival (CWiNFA) picks: every pump.fun create of Oct 24 00:00 - Oct 25 18:00 (univ), labelled by whether the
# rival bought it within a slot of the create (its own transactions, hold/log/CWiNFAEk.jsonl), against what is known at
# the create: launch tool (uri host), dev buy, mayhem, create_v2, the create's own tip, the signer's earlier creates
# that day, the rival's own earlier buys of the signer (24 h), his heat on the signer. Take rate and the rival's
# chain-true result per coin (SOL change of its transactions on the coin, tips and fees in) per value.
import json, glob, collections, bisect, statistics as st
from urllib.parse import urlparse
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
R = "CWiNFAEk4M1y6SjvnZkmxWDmhqSmnKFcnop1WoDW3S2J"; A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"
T0, T1 = 1792800000, 1792951200
U = []
for p in sorted(glob.glob(SP + "/univ/creates_17928*.json") + glob.glob(SP + "/univ/creates_17929*.json")):
    if "partial" in p: continue
    U += [c for c in json.load(open(p)) if T0 <= c["bt"] < T1]
U.sort(key=lambda c: c["bt"])
rows = [json.loads(l) for l in open(SP + "/hold/log/CWiNFAEk.jsonl")]
buy = {}; net = collections.defaultdict(int); tipof = {}
WSOL = "So11111111111111111111111111111111111111112"
for r in rows:
    tok = {m: v for m, v in (r.get("tok") or {}).items() if m != WSOL}
    if len(tok) == 1:
        (m, dt), = tok.items(); net[m] += r["d"]
        if dt > 0 and m not in buy: buy[m] = r["s"]; tipof[m] = r.get("tip") or 0
walk = json.load(open(SP + "/dec/five/walk_AHwJSn.json"))["rows"]
crs = {c["mint"]: c["signer"] for c in U}
hl = collections.defaultdict(list)
for r in walk:
    if r["kind"] == "buy" and r["ok"] and r["m"] in crs: hl[crs[r["m"]]].append(r["bt"])
for v in hl.values(): v.sort()
def host(u):
    try: return urlparse(u).netloc.lower() or "?"
    except Exception: return "?"
sig_creates = collections.defaultdict(list); riv_sig = collections.defaultdict(list); X = []
for c in U:
    s = c["signer"]; t = c["bt"]
    took = c["mint"] in buy and buy[c["mint"]] <= c["slot"] + 1
    prev_c = sum(1 for x in sig_creates[s] if x >= t - 86400); sig_creates[s].append(t)
    prev_r = sum(1 for x in riv_sig[s] if x >= t - 86400)
    if took: riv_sig[s].append(t)
    v = hl.get(s, []); heat = bisect.bisect_left(v, t) - bisect.bisect_left(v, t - 86400)
    X.append(dict(took=took, net=net.get(c["mint"], 0) / 1e9 if took else None, host=host(c.get("uri") or ""), dev=c.get("dev_sol") or 0,
                  mayhem=bool(c.get("mayhem")), v2="create_v2" in (c.get("ix") or []), ctip=c.get("tip") or 0, prev_c=prev_c, prev_r=prev_r, heat=heat))
n = len(X); k = sum(x["took"] for x in X)
print(f"creates {n}, the rival took {k} ({k / n:.1%}); its chain-true net on them {sum(x['net'] for x in X if x['took']):+.2f} SOL")
def table(lab, f, top=10):
    g = collections.defaultdict(lambda: [0, 0, 0.0])
    for x in X:
        key = f(x); g[key][0] += 1
        if x["took"]: g[key][1] += 1; g[key][2] += x["net"]
    print(f"== {lab}")
    for key, (a, b, v) in sorted(g.items(), key=lambda kv: -kv[1][1])[:top]:
        print(f"   {str(key):34} creates {a:6d}  taken {b:5d} ({b / a:6.1%})  share of its buys {b / max(k, 1):5.1%}  its net {v:+7.2f}  per buy {v / max(b, 1):+.4f}")
table("launch tool (uri host)", lambda x: x["host"])
table("dev buy (SOL)", lambda x: "<0.25" if x["dev"] < 0.25 else "0.25-1" if x["dev"] < 1 else "1-2" if x["dev"] < 2 else "2-4" if x["dev"] < 4 else "4+")
table("mayhem / create_v2", lambda x: f"mayhem={x['mayhem']} v2={x['v2']}")
table("the create's own tip", lambda x: "0" if x["ctip"] <= 0 else "<1M" if x["ctip"] < 1e6 else "1-5M" if x["ctip"] < 5e6 else "5M+")
table("signer's creates in the 24 h before", lambda x: min(x["prev_c"], 10))
table("the rival's own buys of the signer in the 24 h before", lambda x: min(x["prev_r"], 10))
table("his heat on the signer (his landed buys, 24 h)", lambda x: min(x["heat"], 5))
