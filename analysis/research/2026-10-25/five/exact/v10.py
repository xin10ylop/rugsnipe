# The curve state at create + 6 and + 10 for coins whose tape stops before: the last succeeded transaction at or
# before each slot (the listing paged back to the create), decoded. Not cached.
#   python3 v10.py <mints.txt>   -> ext/v10/<mint>.json {mint, cs, at: {"6": [slot, idx, vs, vt], "10": [...]}}
import sys, json, glob, os, time, threading
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP, common as C
from rugsnipe_analysis import events as E
tri.MIN_GAP = 0.008
OUT = SP + "/ext/v10"; os.makedirs(OUT, exist_ok=True)
def log(*a): print(time.strftime("%H:%M:%S"), *a, flush=True)
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
ms = [l.strip() for l in open(sys.argv[1]) if l.strip() in cr]
def state_of(sig, m):
    t = tri.rpc("getTransaction", [sig, {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}], cache=False)
    if not t: return None
    ks = PP.keys_of(t); last = None
    for prog, accs, data, parsed, oi, inner in PP._ixs(t, ks):
        if prog != C.PUMP or data[:8] != E.EVENT_IX_TAG or data[8:16] != E.DISC_TRADE: continue
        try:
            e = E.decode_trade(data[8:])
            if e.mint == m: last = [t["slot"], t.get("transactionIndex") or 0, e.virtual_sol_reserves, e.virtual_token_reserves]
        except Exception: pass
    return last
cnt = {"ok": 0, "err": 0, "have": 0}; lock = threading.Lock()
def one(m):
    p = f"{OUT}/{m}.json"
    if os.path.exists(p): return "have"
    cs = cr[m]["slot"]; bc = C.bonding_curve(m); allx = []; before = None
    for _ in range(60):
        o = {"limit": 1000}
        if before: o["before"] = before
        s = tri.rpc("getSignaturesForAddress", [bc, o], cache=False) or []
        allx += [x for x in s if cs <= x["slot"] <= cs + 10 and x.get("err") is None]
        if not s or len(s) < 1000 or s[-1]["slot"] < cs: break
        before = s[-1]["signature"]
    at = {}
    for k in (6, 10):
        xs = [x for x in allx if x["slot"] <= cs + k]
        if not xs: continue
        top = max(x["slot"] for x in xs)
        # the listing is newest first within a slot: the first one listed at the top slot is the last executed
        sig = next(x["signature"] for x in allx if x["slot"] == top)
        st = state_of(sig, m)
        if st: at[str(k)] = st
    json.dump(dict(mint=m, cs=cs, at=at), open(p, "w"))
    return "ok"
def safe(m):
    try: r = one(m)
    except Exception: r = "err"
    with lock:
        cnt[r] += 1
        if sum(cnt.values()) % 500 == 0: log(cnt)
log("coins", len(ms))
with ThreadPoolExecutor(16) as ex: list(ex.map(safe, ms))
log("done", cnt)
