# Live, at our real positions: every buy we sent since the copy went live (Oct 25 22:08 UTC), landed or failed (pump
# 6042 = refused by our bound), from our wallet's own transactions; for each coin, every landed trade from the create to
# create + 60 (slot, index, user, side, SOL, tokens, reserves after).  -> SP/refused/live.json
#   python3 -I pull_live.py [START_UNIX]     (fails/pull.py, generalised)
import sys, json, collections
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, "/home/user/rugsnipe/analysis/scripts/strategy"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import operators_refresh as orf, common as C
from rugsnipe_analysis import events as E
api = orf.Api(open(SP + "/month/.triton_url").read().strip(), float(__import__("os").environ.get("RPS", "25")))
US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
START = int(sys.argv[1]) if len(sys.argv) > 1 else 1792966099
SPAN = 60
def sigs_of(addr, lo_time=None):
    out, before = [], None
    for _ in range(200):
        o = {"limit": 1000}
        if before: o["before"] = before
        page = api.rpc("getSignaturesForAddress", [addr, o]) or []
        out += page
        if len(page) < 1000: break
        if lo_time and (page[-1].get("blockTime") or 0) < lo_time: break
        before = page[-1]["signature"]
    return out
def trades(tx, mint):
    out = []
    for grp in (tx.get("meta") or {}).get("innerInstructions") or []:
        for ix in grp["instructions"]:
            raw = C.b58d(ix.get("data", ""))
            if raw[:8] == E.EVENT_IX_TAG and raw[8:16] == E.DISC_TRADE:
                try:
                    e = E.decode_trade(raw[8:])
                    if e.mint == mint: out.append(e)
                except Exception:
                    pass
    return out
ours = [s for s in sigs_of(US, lo_time=START) if (s.get("blockTime") or 0) >= START]
print("our signatures", len(ours), flush=True)
def get(s):
    return s, api.rpc("getTransaction", [s["signature"], orf.TX_OPTS], cache=True)
with ThreadPoolExecutor(8) as ex:
    txs = list(ex.map(get, ours))
mine = collections.defaultdict(list)
for s, tx in txs:
    if not tx: continue
    m, meta = tx["transaction"]["message"], tx["meta"]
    la = meta.get("loadedAddresses") or {}
    ks = list(m["accountKeys"]) + la.get("writable", []) + la.get("readonly", [])
    logs = " ".join(meta.get("logMessages") or [])
    if "Instruction: Buy" not in logs: continue
    pumps = [k for k in ks if k.endswith("pump")]
    if not pumps: continue
    mine[pumps[0]].append(dict(slot=tx["slot"], idx=tx.get("transactionIndex"), err=meta.get("err") is not None,
                               code=("6042" if "6042" in logs else "other") if meta.get("err") else None,
                               bt=tx.get("blockTime"), fee=meta.get("fee"), sig=s["signature"], d=None))
print("coins we sent a buy on", len(mine), "refused (6042)", sum(any(a["code"] == "6042" for a in v) for v in mine.values()), flush=True)
json.dump(dict(mine=mine), open(SP + "/refused/live.json", "w"))
ms = sorted(mine); json.dump(ms, open(SP + "/refused/live_mints.json", "w"))
print("saved; tapes: python3 -I tapes.py SP/refused/live_mints.json", flush=True)
