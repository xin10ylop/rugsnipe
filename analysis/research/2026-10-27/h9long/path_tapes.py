# Full curve paths (every landed trade from its buy to its sell) for trips that ended on its take-profit or stop-loss
# (held < 1700 s), to test our own trigger one step ahead of its: the first trade past 2.45x / 0.71x of its post-buy
# price, then our sell at the end of the next slot.   python3 -I path_tapes.py WALLET N_TP N_SL -> <w8>_paths.json
import sys, json, random, os, time
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, common as C
from rugsnipe_analysis import events as E
exec(open("/home/user/rugsnipe/analysis/research/2026-10-27/h9long/entry_tapes.py").read().split("def one(x):")[0].split("rows = [json.loads")[0])
W = sys.argv[1]; NT, NS = int(sys.argv[2]), int(sys.argv[3])
rows = [json.loads(l) for l in open(f"{SP}/h9long/{W[:8]}.jsonl")]
sig_of = {(t[0], r["s"], t[2]): r["sig"] for r in rows for t in r["tr"] if t[1] == W}
X = {r["m"]: r for r in json.load(open(f"{SP}/h9long/{W[:8]}_exits.json"))}
T = {x["m"]: x for x in json.load(open(f"{SP}/h9long/{W[:8]}_trips.json"))}
tp = [m for m, r in X.items() if r["sec"] < 1700 and r["marg"] >= 2.4 and r["kind"] == "pump, other quote"]
sl = [m for m, r in X.items() if r["sec"] < 1700 and r["marg"] < 0.75 and r["kind"] == "pump, other quote"]
random.seed(11); pick = random.sample(tp, min(NT, len(tp))) + random.sample(sl, min(NS, len(sl)))
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
                if e.mint == m: out.append([tx["slot"], tx.get("transactionIndex") or 0, e.user, int(e.is_buy), e.token_amount, e.virtual_token_reserves])
    return out
def one(m):
    b, s = T[m]["tr"]
    bs, ss = sig_of.get((m, b["s"], 1)), sig_of.get((m, s["s"], 0))
    if not bs or not ss: return m, None
    bc = C.bonding_curve(m); sigs, before = [], ss
    for _ in range(5):
        page = tri.rpc("getSignaturesForAddress", [bc, {"limit": 1000, "before": before, "until": bs}], cache=False) or []
        sigs += [p["signature"] for p in page if not p.get("err")]
        if len(page) < 1000: break
        before = page[-1]["signature"]
    if len(sigs) > 500: return m, dict(skip=len(sigs))
    tr = []
    for sg in sigs:
        try:
            tx = tri.rpc("getTransaction", [sg, TXO], cache=False)
            if tx: tr += trades(tx, m)
        except Exception: pass
    tr.sort(key=lambda r: (r[0], r[1]))
    return m, dict(b=b, s=s, path=tr, kind="tp" if m in tp else "sl")
out = {}
with ThreadPoolExecutor(4) as ex:
    for m, v in ex.map(one, pick): out[m] = v
json.dump(out, open(f"{SP}/h9long/{W[:8]}_paths.json", "w"))
print("done", len(out), sum(1 for v in out.values() if v and "path" in v), flush=True)
