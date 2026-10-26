# Every buy of ours since the start (landed or failed) from our wallet's own transactions, then for every coin we sent a
# buy on: all landed trades of its first 40 slots (slot, index, user, side, SOL, tokens, reserves after), plus our failed
# buy's slot and index. -> fails/coins.json
import sys, json, time, collections
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, "/home/user/rugsnipe/analysis/scripts/strategy"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import operators_refresh as orf, common as C
from rugsnipe_analysis import events as E
api = orf.Api(open(SP + "/month/.triton_url").read().strip(), 25.0)
US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
START = 1792966099
def sigs_of(addr, until_slot=None, lo_time=None):
    out, before = [], None
    for _ in range(80):
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
with ThreadPoolExecutor(12) as ex:
    txs = list(ex.map(get, ours))
mine = collections.defaultdict(list)          # mint -> our buy attempts
for s, tx in txs:
    if not tx: continue
    m, meta = tx["transaction"]["message"], tx["meta"]
    la = meta.get("loadedAddresses") or {}
    ks = list(m["accountKeys"]) + la.get("writable", []) + la.get("readonly", [])
    logs = " ".join(meta.get("logMessages") or [])
    if "Instruction: Buy" not in logs: continue
    pumps = [k for k in ks if k.endswith("pump")]
    if not pumps: continue
    mint = pumps[0]
    mine[mint].append(dict(slot=tx["slot"], idx=tx.get("transactionIndex"), err=meta.get("err") is not None,
                           code=("6042" if "6042" in logs else "other") if meta.get("err") else None,
                           bt=tx.get("blockTime"), fee=meta.get("fee"), sig=s["signature"]))
print("coins we sent a buy on", len(mine), "with a failed buy", sum(any(a["err"] for a in v) for v in mine.values()), flush=True)
def tape(mint):
    bc = orf.bonding_curve(mint)
    ss = sigs_of(bc)
    ss = [x for x in ss if not x.get("err")]
    if not ss: return mint, None
    c0 = min(x["slot"] for x in ss)
    ss = [x for x in ss if x["slot"] <= c0 + 40]
    rows = []
    for x in ss:
        tx = api.rpc("getTransaction", [x["signature"], orf.TX_OPTS], cache=True)
        if not tx: continue
        for e in trades(tx, mint):
            rows.append([tx["slot"], tx.get("transactionIndex") or 0, e.user, int(e.is_buy), e.sol_amount, e.token_amount,
                         e.virtual_sol_reserves, e.virtual_token_reserves])
    rows.sort(key=lambda r: (r[0], r[1]))
    return mint, dict(c0=c0, rows=rows)
with ThreadPoolExecutor(8) as ex:
    tapes = dict(ex.map(tape, list(mine)))
json.dump(dict(mine=mine, tapes=tapes), open(SP + "/fails/coins.json", "w"))
print("done", sum(1 for v in tapes.values() if v), "tapes", flush=True)
