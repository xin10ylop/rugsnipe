# Each night bundle wallet's real launches: the distinct mints it bought (its own pump buys) from Oct 24 12:00, with
# the slot of its first buy of each. -> dec/night/wbuys.json {wallet: [[slot, mint], ...]}
import sys, json, os, time, collections, threading
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/dec/night.py").read()
exec(src.split("# 1. his attempts")[0])
WL = json.load(open(OUT + "/wallets.json"))
P = OUT + "/wbuys.json"
done = json.load(open(P)) if os.path.exists(P) else {}
lock = threading.Lock()
def one(w):
    if w in done: return
    s, _ = sigs_range(w, TW, T1, 20)
    first = {}
    for x in s:
        if x.get("err"): continue
        t = tri.rpc("getTransaction", [x["signature"], {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}], cache=False)
        if not t: continue
        ks = PP.keys_of(t)
        for prog, accs, data, parsed, oi, inner in PP._ixs(t, ks):
            if prog == C.PUMP and data[:8] == E.EVENT_IX_TAG and data[8:16] == E.DISC_TRADE:
                try:
                    e = E.decode_trade(data[8:])
                    if e.user == w and e.is_buy and (e.mint not in first or t["slot"] < first[e.mint][0]):
                        first[e.mint] = (t["slot"], (e.virtual_sol_reserves - e.sol_amount) / 1e9, e.sol_amount / 1e9)
                except Exception: pass
    with lock: done[w] = sorted([v[0], m, round(v[1], 3), round(v[2], 3)] for m, v in first.items())
n = [0]
def safe(w):
    try: one(w)
    except Exception as e: log("error", w[:8], tri.red(e)[:100])
    n[0] += 1
    if n[0] % 50 == 0:
        log("wallets", n[0], "of", len(WL))
        with lock: json.dump(done, open(P + ".tmp", "w")); os.replace(P + ".tmp", P)
with ThreadPoolExecutor(8) as ex: list(ex.map(safe, sorted(WL)))
json.dump(done, open(P, "w"))
log("wbuys done", len(done))
