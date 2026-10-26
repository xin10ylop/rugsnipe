# The create signer of each of last night's key coins (his launch key buys since Oct 25 22:08), from the oldest
# signature of the bonding curve; then each operator's history with him: his trips in the month (to Oct 24 13:08) and
# the 5-day walk (Oct 21 - 25 18:00), so B (within 7 days) / C / D (new) can be told apart.
import sys, json, collections, datetime
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, "/home/user/rugsnipe/analysis/scripts/strategy"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import operators_refresh as orf
api = orf.Api(open(SP + "/month/.triton_url").read().strip(), 25.0)
N = json.load(open(SP + "/live1/recon.json"))
K = [c for c in N["coins"] if c["launch"] and c["key"]]
def signer(m):
    curve = orf.bonding_curve(m); before = None; last = None
    for _ in range(30):
        o = {"limit": 1000}
        if before: o["before"] = before
        page = api.rpc("getSignaturesForAddress", [curve, o]) or []
        if not page: break
        last = page[-1]; before = last["signature"]
        if len(page) < 1000: break
    if not last: return m, None
    tx = api.rpc("getTransaction", [last["signature"], orf.TX_OPTS])
    if not tx: return m, None
    return m, tx["transaction"]["message"]["accountKeys"][0]
with ThreadPoolExecutor(8) as ex: S = dict(ex.map(signer, [c["m"] for c in K]))
T = json.load(open(SP + "/month/AHwJ_T.json"))
W = json.load(open(SP + "/dec/five/walk_AHwJSn.json"))["rows"]
import glob
cr = {}
for p in glob.glob(SP + "/dec/five/coin/*.json"):
    d = json.load(open(p))
    if d.get("create"): cr[d["mint"]] = d["create"]["signer"]
hist = collections.defaultdict(list)
for t in T: hist[t["creator"]].append(t["bt"])
for r in W:
    if r["kind"] == "buy" and r["ok"] and r["m"] in cr: hist[cr[r["m"]]].append(r["bt"])
A8 = open(SP + "/oprule/op8zn.txt").read().strip()
out = []
for c in K:
    s = S.get(c["m"]); h = sorted(hist.get(s, [])) if s else []
    last = max(h) if h else None
    out.append(dict(m=c["m"], signer=s, n24=c["n24"], his=c["net"], copy=(c["copy"] or [None])[0], n_hist=len(h),
                    last_days=None if last is None else round((N["start"] - last) / 86400, 1), is8zn=s == A8))
for o in sorted(out, key=lambda o: (o["n24"] >= 2, o["n_hist"])):
    print(f"{o['m'][:8]} signer {str(o['signer'])[:6]} n24 {o['n24']:2d} his-history {o['n_hist']:3d} last {o['last_days']} d ago  his {o['his']:+.3f} copy {o['copy'] if o['copy'] is None else round(o['copy'], 3)}" + ("  <- 8ZN71X" if o["is8zn"] else ""))
json.dump(out, open(SP + "/oprule/night_signers.json", "w"))
g = collections.defaultdict(lambda: [0, 0.0, 0.0])
for o in out:
    k = "A >=2 in 24h" if o["n24"] >= 2 else ("B/C known before" if o["n_hist"] else "D new")
    g[k][0] += 1; g[k][1] += o["his"]; g[k][2] += o["copy"] or 0
print({k: (v[0], round(v[1], 3), round(v[2], 3)) for k, v in g.items()})
