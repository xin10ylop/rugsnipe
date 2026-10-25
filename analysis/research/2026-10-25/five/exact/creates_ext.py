# Every pump.fun create from T0 to now, decoded as univ/creates.py does (mint, creator, signer, name/symbol/uri, mayhem,
# dev buy, create ix), one listing pass back from now, then the transactions (not cached: disk).
# -> univ/creates_<hour>.json for each whole hour, univ/creates_<hour>.partial.json for the current one
import sys, json, time, collections, threading
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP, common as C
from rugsnipe_analysis import events as E
tri.MIN_GAP = 0.008
MA = "TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM"
T0 = int(sys.argv[1]); T1 = int(sys.argv[2]) if len(sys.argv) > 2 else int(time.time())
def log(*a): print(time.strftime("%H:%M:%S"), *a, flush=True)
sigs = []; before = None; pages = 0
while True:
    o = {"limit": 1000}
    if before: o["before"] = before
    s = tri.rpc("getSignaturesForAddress", [MA, o], cache=False) or []
    pages += 1
    if not s: break
    before = s[-1]["signature"]
    sigs += [x for x in s if T0 <= (x.get("blockTime") or 0) < T1 and not x.get("err")]
    if (s[-1].get("blockTime") or 0) < T0: break
    if pages % 10 == 0: log("pages", pages, "sigs", len(sigs))
log("create signatures", len(sigs), "pages", pages)
def tx(sig): return tri.rpc("getTransaction", [sig, {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}], cache=False)
def one(x):
    try: t = tx(x["signature"])
    except Exception: return None
    if not t: return None
    ks = PP.keys_of(t); meta = t.get("meta") or {}
    cr = None; trades = []
    for grp in meta.get("innerInstructions") or []:
        for ix in grp["instructions"]:
            if ks[ix["programIdIndex"]] != C.PUMP: continue
            raw = C.b58d(ix.get("data", ""))
            if raw[:8] != E.EVENT_IX_TAG: continue
            try:
                if raw[8:16] == E.DISC_CREATE:
                    e = E.decode_create(raw[8:]); cr = dict(mint=e.mint, creator=e.creator, user=e.user, name=e.name, symbol=e.symbol, uri=e.uri, mayhem=getattr(e, "is_mayhem_mode", getattr(e, "mayhem", None)))
                elif raw[8:16] == E.DISC_TRADE:
                    e = E.decode_trade(raw[8:]); trades.append((e.user, e.is_buy, e.sol_amount))
            except Exception: pass
    if not cr: return None
    a = PP.analyze(t)
    cr.update(sig=x["signature"], slot=t["slot"], idx=t.get("transactionIndex"), bt=t.get("blockTime"), signer=ks[0], nsig=t["transaction"]["message"]["header"]["numRequiredSignatures"],
              ix=a["pump_ix"], dev_sol=sum(s for u, b, s in trades if b) / 1e9, n_buys_in_tx=sum(1 for u, b, s in trades if b), tip=a["tip_total"])
    return cr
by = collections.defaultdict(list); n = [0]; lock = threading.Lock()
with ThreadPoolExecutor(16) as ex:
    for cr in ex.map(one, sigs):
        n[0] += 1
        if n[0] % 2000 == 0: log("txs", n[0], "of", len(sigs))
        if cr: by[cr["bt"] // 3600 * 3600].append(cr)
now_h = int(time.time()) // 3600 * 3600
for h, xs in sorted(by.items()):
    name = f"{SP}/univ/creates_{h}.json" if h + 3600 <= T1 else f"{SP}/univ/creates_{h}.partial.json"
    json.dump(sorted(xs, key=lambda c: (c["slot"], c["idx"] or 0)), open(name, "w"))
    log("wrote", name.rsplit("/", 1)[1], len(xs))
log("done", sum(len(v) for v in by.values()))
