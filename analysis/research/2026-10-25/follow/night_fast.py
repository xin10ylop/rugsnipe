# Faster create slots for the warm-up coins (his attempts outside the night's reconciliation): the bonding curve's
# transactions before his own attempt are the create and its bundle; one call instead of paging back from today.
import sys, json, os, time, collections
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/dec/night.py").read()
exec(src.split("# 1. his attempts")[0])
exec("def decode" + src.split("def decode")[1].split("def coin(m):")[0])
R = [r for r in json.load(open(SP + "/live/recon.json"))["res"] if "note" not in r]
night = {r["mint"] for r in R}
ss, _ = sigs_range(A, TW, T1, 100)
todo = []
for x in ss:
    t = tri.tx(x["signature"])
    if not t: continue
    ks = PP.keys_of(t)
    if ks[0] != A: continue
    m = None
    for prog, accs, data, parsed, oi, inner in PP._ixs(t, ks):
        if prog == C.PUMP and not inner and data[:8] in PP.IXR and PP.IXR[data[:8]].startswith("buy") and len(accs) > 2: m = accs[2]
    if not m or not m.endswith("pump"):
        mm = [k for k in ks if k.endswith("pump")]; m = mm[0] if mm else m
    if m and m not in night: todo.append((m, x["signature"]))
log("warm-up coins", len(todo))
def one(ms):
    m, sig = ms; p = f"{OUT}/coins/{m}.json"
    if os.path.exists(p): return "have"
    bc = C.bonding_curve(m)
    s = tri.rpc("getSignaturesForAddress", [bc, {"limit": 100, "before": sig}], cache=False) or []
    if not s: return "empty"
    cs = min(x["slot"] for x in s)
    rows = []
    for x in s:
        if x["slot"] == cs and x.get("err") is None:
            t = tri.tx(x["signature"])
            if t: rows.append(decode(t, m))
    rows.sort(key=lambda r: (r[0], r[1]))
    cr = next((r for r in rows if r[4]), None); c = None
    if cr:
        evs = cr[3]; c = dict(slot=cr[0], idx=cr[1], signer=cr[2], user=evs[0][0] if evs else cr[2], creator=evs[0][7] if evs else cr[2])
    json.dump(dict(mint=m, cs=cs, reached=len(s) < 100, create=c, rows=[r[:4] for r in rows]), open(p + ".tmp", "w")); os.replace(p + ".tmp", p)
    return "ok" if c else "no create"
cnt = collections.Counter()
def safe(ms):
    try: cnt[one(ms)] += 1
    except Exception as e: cnt["error"] += 1
    if sum(cnt.values()) % 200 == 0: log("warm", sum(cnt.values()), dict(cnt))
with ThreadPoolExecutor(8) as ex: list(ex.map(safe, todo))
log("warm done", dict(cnt))
