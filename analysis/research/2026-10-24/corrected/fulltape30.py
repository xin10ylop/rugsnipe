# Complete first-12-slot tapes (every succeeded transaction, real fee + creator fee) for the rule universe: tool host,
# template dev buy, a deployer (creator or signer) he traded in the 7 days before. Oct 23 all day, Oct 24 00-12.
#   python3 -I fulltape.py k/n        (shard)          -> scratchpad/full/<mint>.json
import sys, json, glob, os, time, collections, bisect
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP, common as C
from rugsnipe_analysis import events as E
K, N = map(int, (sys.argv[1] if len(sys.argv) > 1 else "0/1").split("/"))
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
def log(*a): print(time.strftime("%H:%M:%S"), *a, flush=True)
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
DC = collections.defaultdict(list); DS = collections.defaultdict(list)
for t in json.load(open(SP + "/month/AHwJ_T.json")):
    if t["stake"] < 0.1: continue
    DC[t["creator"]].append(t["bt"])
    if t["mint"] in cr: DS[cr[t["mint"]]["signer"]].append(t["bt"])
for d in (DC, DS):
    for v in d.values(): v.sort()
n_in = lambda d, k, lo, hi: bisect.bisect_left(d.get(k, []), hi) - bisect.bisect_left(d.get(k, []), lo)
U = []
for day in (23, 24):
    for r in json.load(open(SP + f"/pm/dataset_{day}.json")):
        if day == 24 and r["hour"] >= 12: continue
        c = cr.get(r["mint"])
        if not c or r["host"] not in TOOLS or not TEMPLATE(r["dev"]): continue
        if n_in(DC, c["creator"], c["bt"] - 7 * 86400, c["bt"]) + n_in(DS, c["signer"], c["bt"] - 7 * 86400, c["bt"]) == 0: continue
        U.append(c)
U.sort(key=lambda c: c["slot"]); U = U[::6]; mine = U[K::N]
log(f"universe {len(U)} coins; shard {K}/{N}: {len(mine)}")
def decode(t, m):
    ks = PP.keys_of(t); ev = []
    for prog, accs, data, parsed, oi, inner in PP._ixs(t, ks):
        if prog != C.PUMP or data[:8] != E.EVENT_IX_TAG or data[8:16] != E.DISC_TRADE: continue
        try:
            e = E.decode_trade(data[8:])
            if e.mint == m: ev.append([e.user, int(e.is_buy), e.sol_amount, e.token_amount, e.virtual_sol_reserves, e.virtual_token_reserves, (e.fee or 0) + (e.creator_fee or 0)])
        except Exception: pass
    return [t["slot"], t.get("transactionIndex") or 0, ks[0], ev]
def one(c):
    m = c["mint"]; p = f"{SP}/full30/{m}.json"
    if os.path.exists(p): return "have"
    bc = C.bonding_curve(m); keep = []; before = None; pages = 0; reached = False
    while pages < 800:
        o = {"limit": 1000}
        if before: o["before"] = before
        s = tri.rpc("getSignaturesForAddress", [bc, o], cache=False) or []
        pages += 1
        keep += [x for x in s if x["slot"] <= c["slot"] + 30 and x.get("err") is None]
        if not s or len(s) < 1000 or s[-1]["slot"] < c["slot"]: reached = True; break
        before = s[-1]["signature"]
    rows = []
    for x in keep:
        t = tri.tx(x["signature"])
        if t: rows.append(decode(t, m))
    rows.sort(key=lambda r: (r[0], r[1]))
    tmp = p + ".tmp"; json.dump(dict(mint=m, cs=c["slot"], ci=c["idx"] or 0, pages=pages, reached=reached, rows=rows), open(tmp, "w")); os.replace(tmp, p)
    return "pulled"
done = collections.Counter()
def safe(c):
    try: done[one(c)] += 1
    except Exception as e: done["error"] += 1; log("error", c["mint"][:8], tri.red(e)[:120])
    s = sum(done.values())
    if s % 100 == 0: log(f"{s} of {len(mine)}", dict(done))
with ThreadPoolExecutor(4) as ex: list(ex.map(safe, mine))
log("done", dict(done))
