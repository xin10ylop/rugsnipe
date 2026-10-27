# Every landed curve trade from its buy to its sell for its trips with a first buy of at least MIN_SOL (SOL value),
# both quotes: the whole price path, for exits of our own (fixed holds, trailing stops, take-profit ladders).
#   python3 -I full_paths.py WALLET MIN_SOL   ->  <w8>_fpaths.json  (resumable)
import sys, json, os, time
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, common as C
from rugsnipe_analysis import events as E
W = sys.argv[1]; MIN = float(sys.argv[2]); CAP = int(sys.argv[3]) if len(sys.argv) > 3 else 40
MAX = float(os.environ.get("MAX_SOL", "1e9")); KINDS = tuple(os.environ.get("KINDS", "pump, other quote|pump, SOL quote").split("|"))
rows = [json.loads(l) for l in open(f"{SP}/h9long/{W[:8]}.jsonl")]
sig_of = {(t[0], r["s"], t[2]): r["sig"] for r in rows for t in r["tr"] if t[1] == W}
T = json.load(open(f"{SP}/h9long/{W[:8]}_trips.json"))
pick = [x for x in T if not x["open"] and len(x["tr"]) == 2 and x["tr"][0]["buy"] and not x["tr"][1]["buy"]
        and MIN <= (x["tr"][0]["v"] or 0) < MAX and x["kind"] in KINDS]
OUT = f"{SP}/h9long/{W[:8]}_fpaths.json"
have = json.load(open(OUT)) if os.path.exists(OUT) else {}
TXO = {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}
def trades(tx, m):
    out = []
    ks = tx["transaction"]["message"]["accountKeys"] + (tx["meta"].get("loadedAddresses") or {}).get("writable", []) + (tx["meta"].get("loadedAddresses") or {}).get("readonly", [])
    for g in tx["meta"].get("innerInstructions") or []:
        for ix in g["instructions"]:
            if ks[ix["programIdIndex"]] != C.PUMP: continue
            raw = C.b58d(ix.get("data", ""))
            if raw[:8] == E.EVENT_IX_TAG and raw[8:16] == E.DISC_TRADE:
                try: e = E.decode_trade(raw[8:])
                except Exception: continue
                if e.mint == m: out.append([tx["slot"], tx.get("transactionIndex") or 0, e.user[:8], int(e.is_buy), e.token_amount, e.virtual_token_reserves, tx.get("blockTime")])
    return out
def one(x):
    m = x["m"]
    if m in have: return m, have[m]
    b, s = x["tr"]
    bs, ss = sig_of.get((m, b["s"], 1)), sig_of.get((m, s["s"], 0))
    if not bs or not ss: return m, None
    bc = C.bonding_curve(m); sigs, before = [], ss
    for _ in range(10):
        page = tri.rpc("getSignaturesForAddress", [bc, {"limit": 1000, "before": before, "until": bs}], cache=False) or []
        sigs += [p["signature"] for p in page if not p.get("err")]
        if len(page) < 1000: break
        before = page[-1]["signature"]
    if len(sigs) > 3000: return m, dict(skip=len(sigs))
    # sigs are newest first: keep the first 8 after its buy, the last 4 before its sell, and an even sample between
    # (at most CAP transactions a trip); the path's states between samples are not seen
    old = sigs[::-1]
    if len(old) > CAP:
        mid = old[8:-4]; k = max(1, len(mid) // (CAP - 12))
        old = old[:8] + mid[::k] + old[-4:]
    tr = []
    for sg in old:
        for k in range(2):
            try:
                tx = tri.rpc("getTransaction", [sg, TXO], cache=False)
                if tx: tr += trades(tx, m)
                break
            except Exception: pass
    tr.sort(key=lambda r: (r[0], r[1]))
    return m, dict(path=tr, n=len(sigs), sampled=len(sigs) > CAP)
n = 0
print(time.strftime("%H:%M:%S"), len(pick), "trips", flush=True)
with ThreadPoolExecutor(11) as ex:
    for m, v in ex.map(one, pick):
        have[m] = v; n += 1
        if n % 50 == 0: print(time.strftime("%H:%M:%S"), n, "/", len(pick), flush=True); json.dump(have, open(OUT, "w"))
json.dump(have, open(OUT, "w")); print("done", n, flush=True)
