# For a sample of its one-buy one-sell trips: every curve transaction between its buy and its sell (signatures), and the
# trades of the slots right after its buy (its slot, +1, +2) and right before its sell (decoded): the price we would
# buy at one slot behind it, who buys right after it, and what happens just before its exit.
#   python3 -I entry_tapes.py WALLET N_OTHER N_SOL   ->  <w8>_etapes.json
import sys, json, random, os, time
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, common as C
from rugsnipe_analysis import events as E
W = sys.argv[1]; NO, NS = int(sys.argv[2]), int(sys.argv[3])
TX = {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}
rows = [json.loads(l) for l in open(f"{SP}/h9long/{W[:8]}.jsonl")]
sig_of = {}
for r in rows:
    for t in r["tr"]:
        if t[1] == W: sig_of[(t[0], r["s"], t[2])] = r["sig"]
T = json.load(open(f"{SP}/h9long/{W[:8]}_trips.json"))
trips = [x for x in T if not x["open"] and len(x["tr"]) == 2 and x["tr"][0]["buy"] and not x["tr"][1]["buy"]]
random.seed(7)
NEAR_SELL = True
if NO < 0:                                   # every trip
    pick = [x for x in trips if x["kind"] == "pump, other quote"]       # the profitable kind; entry slots only
    NEAR_SELL = False
else:
    pick = random.sample([x for x in trips if x["kind"] == "pump, other quote"], NO) + random.sample([x for x in trips if x["kind"] == "pump, SOL quote"], NS)
OUT = f"{SP}/h9long/{W[:8]}_etapes.json"
have = json.load(open(OUT)) if os.path.exists(OUT) else {}
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
                if e.mint == m: out.append([tx["slot"], tx.get("transactionIndex") or 0, e.user, int(e.is_buy), e.token_amount, e.virtual_token_reserves, e.sol_amount])
    return out
def one(x):
    m = x["m"]
    if m in have: return m, have[m]
    b, s = x["tr"]
    bs, ss = sig_of.get((m, b["s"], 1)), sig_of.get((m, s["s"], 0))
    if not bs or not ss: return m, None
    bc = C.bonding_curve(m)
    sigs, before = [], ss
    for _ in range(30):
        page = tri.rpc("getSignaturesForAddress", [bc, {"limit": 1000, "before": before, "until": bs}], cache=False) or []
        sigs += [[p["signature"], p["slot"]] for p in page if not p.get("err")]
        if len(page) < 1000: break
        before = page[-1]["signature"]
    full = len(sigs) < 29000
    near = [p for p in sigs if p[1] <= b["s"] + 2] + ([p for p in sigs if p[1] >= s["s"] - 1] if NEAR_SELL else [])
    near = list({p[0]: p for p in near}.values())[:120]
    tr = []
    for p in near:
        try:
            tx = tri.rpc("getTransaction", [p[0], TX], cache=False)
            if tx: tr += trades(tx, m)
        except Exception: pass
    tr.sort(key=lambda r: (r[0], r[1]))
    return m, dict(n_between=len(sigs), complete=full, slots=sorted({p[1] for p in sigs}), near=tr, b=b, s=s, kind=x["kind"])
n = 0
with ThreadPoolExecutor(8) as ex:
    for m, v in ex.map(one, pick):
        have[m] = v; n += 1
        if n % 50 == 0:
            print(time.strftime("%H:%M:%S"), n, "/", len(pick), flush=True); json.dump(have, open(OUT, "w"))
json.dump(have, open(OUT, "w")); print("done", n, flush=True)
