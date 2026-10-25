# The trades of given slots after the create for a list of coins: the curve's listing paged back to the create slot,
# then every succeeded transaction in [create + LO, create + HI], decoded (trade events of the mint). Not cached (disk).
#   python3 slots.py <mints.txt> <LO> <HI> <outdir>    -> <outdir>/<mint>.json {mint, cs, ci, lo, hi, reached, rows}
# rows: [slot, index, signer, [[user, is_buy, sol, tok, vsol, vtok, fee + creator fee]]] (the tape format of full/, fp/)
import sys, json, glob, os, time, threading
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP, common as C
from rugsnipe_analysis import events as E
tri.MIN_GAP = 0.008
LIST, LO, HI, OUT = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
os.makedirs(OUT, exist_ok=True)
def log(*a): print(time.strftime("%H:%M:%S"), *a, flush=True)
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
ms = [l.strip() for l in open(LIST) if l.strip() and l.strip() in cr]
def tx(sig): return tri.rpc("getTransaction", [sig, {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}], cache=False)
def decode(t, m):
    ks = PP.keys_of(t); ev = []
    for prog, accs, data, parsed, oi, inner in PP._ixs(t, ks):
        if prog != C.PUMP or data[:8] != E.EVENT_IX_TAG or data[8:16] != E.DISC_TRADE: continue
        try:
            e = E.decode_trade(data[8:])
            if e.mint == m: ev.append([e.user, int(e.is_buy), e.sol_amount, e.token_amount, e.virtual_sol_reserves, e.virtual_token_reserves, (e.fee or 0) + (e.creator_fee or 0)])
        except Exception: pass
    return [t["slot"], t.get("transactionIndex") or 0, ks[0], ev]
cnt = {"ok": 0, "partial": 0, "have": 0, "err": 0}; lock = threading.Lock()
def one(m):
    p = f"{OUT}/{m}.json"
    if os.path.exists(p): return "have"
    c = cr[m]; cs = c["slot"]; bc = C.bonding_curve(m)
    keep = []; before = None; pages = 0; reached = False
    while pages < 60:
        o = {"limit": 1000}
        if before: o["before"] = before
        s = tri.rpc("getSignaturesForAddress", [bc, o], cache=False) or []
        pages += 1
        keep += [x for x in s if cs + LO <= x["slot"] <= cs + HI and x.get("err") is None]
        if not s or len(s) < 1000 or s[-1]["slot"] < cs: reached = True; break
        before = s[-1]["signature"]
    rows = []
    for x in keep:
        t = tx(x["signature"])
        if t: rows.append(decode(t, m))
    rows.sort(key=lambda r: (r[0], r[1]))
    json.dump(dict(mint=m, cs=cs, ci=c["idx"], lo=LO, hi=HI, pages=pages, reached=reached, rows=rows), open(p + ".tmp", "w")); os.replace(p + ".tmp", p)
    return "ok" if reached else "partial"
def safe(m):
    try: r = one(m)
    except Exception: r = "err"
    with lock:
        cnt[r] += 1
        if sum(cnt.values()) % 1000 == 0: log(cnt)
log("coins", len(ms), "slots", LO, "..", HI)
with ThreadPoolExecutor(16) as ex: list(ex.map(safe, ms))
log("done", cnt)
