# Who funded each deployer wallet before its create: the most recent SOL inflows (from a wallet other than the
# deployer) among the last 25 transactions before the create signature, 2 levels (the funder's own funder).
#   python3 -I ftrace.py  -> dec/funders.json {creator_or_wallet@sig: {...}}
import sys, json, glob, collections, os, time, threading
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl")
import tri, pumpparse as PP
tri.MIN_GAP = 0.02
OUT = SP + "/dec/funders.json"
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
U = json.load(open(SP + "/fp/_universe.json"))
mints = [m for m in U["pick"] + U["ctrl"] if m in cr]
done = json.load(open(OUT)) if os.path.exists(OUT) else {}
lock = threading.Lock()
def inflows(w, before, limit=25):
    """[(src, sol, slot, age_txs)] newest first: SOL into w from another signer before `before`."""
    s = tri.rpc("getSignaturesForAddress", [w, {"limit": limit, "before": before}], cache=True) or []
    out = []
    for k, x in enumerate(s):
        if x.get("err"): continue
        t = tri.tx(x["signature"])
        if not t: continue
        ks = PP.keys_of(t); m = t["meta"]
        if w not in ks or ks[0] == w: continue
        i = ks.index(w); d = m["postBalances"][i] - m["preBalances"][i]
        if d >= 50_000_000: out.append((ks[0], d / 1e9, x["slot"], k, x["signature"]))
        if len(out) >= 2: break
    return out, len(s)
def one(m):
    c = cr[m]; w = c["signer"]
    if m in done: return "cached"
    f1, n1 = inflows(w, c["sig"])
    r = dict(w=w, n_before=n1, f=[(a, round(b, 3), c["slot"] - s, k) for a, b, s, k, _ in f1])
    if f1:
        f2, n2 = inflows(f1[0][0], f1[0][4], 12)
        r["f2"] = [(a, round(b, 3), c["slot"] - s, k) for a, b, s, k, _ in f2]; r["n2"] = n2
    with lock: done[m] = r
    return "ok"
cnt = collections.Counter(); t0 = time.time()
def safe(m):
    try: cnt[one(m)] += 1
    except Exception as e: cnt["error"] += 1; print("error", m[:8], tri.red(e)[:100], flush=True)
    s = sum(cnt.values())
    if s % 200 == 0:
        print(time.strftime("%H:%M:%S"), s, "of", len(mints), dict(cnt), flush=True)
        with lock: json.dump(done, open(OUT + ".tmp", "w")); os.replace(OUT + ".tmp", OUT)
with ThreadPoolExecutor(8) as ex: list(ex.map(safe, mints))
json.dump(done, open(OUT, "w"))
print("done", dict(cnt), flush=True)
