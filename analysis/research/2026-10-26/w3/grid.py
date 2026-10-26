# 3yrJE1A8's coins since Oct 24 (hold/log): the curve state at its buy slot + k for k in GRID (the last landed trade on
# the coin at or before that slot), to value a copy sold at buy + k. -> w3/grid.json {mint: {buy_slot, buy_post, sell1_slot,
# sell1_pre, states: {k: [v, t]}}}
import sys, json, os, collections, threading
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, common as C
from rugsnipe_analysis import events as E
tri.MIN_GAP = 0.012
K = 30_000_000_000 * 1_073_000_000_000_000
GRID = [5, 9, 14, 17, 19, 24, 29]
rows = [json.loads(l) for l in open(SP + "/hold/log/3yrJE1A8.jsonl")]
W = next(t[1] for r in rows for t in r.get("tr") or [] if t[1].startswith("3yrJE1A8"))
ev = collections.defaultdict(list)
for r in rows:
    for t in r.get("tr") or []:
        if t[1] == W: ev[t[0]].append((r["s"], r.get("i") or 0, t))
def stt(t): return [t[5] if t[5] > 0 else K // t[6], t[6]] if t[6] > 0 else None
def last_trade_state(sig, m):
    t = tri.rpc("getTransaction", [sig, {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}], cache=False)
    if not t: return None
    ks = t["transaction"]["message"]["accountKeys"] + (t["meta"].get("loadedAddresses") or {}).get("writable", []) + (t["meta"].get("loadedAddresses") or {}).get("readonly", [])
    st_ = None
    for g in t["meta"].get("innerInstructions") or []:
        for ix in g["instructions"]:
            if ks[ix["programIdIndex"]] != C.PUMP: continue
            raw = C.b58d(ix.get("data", ""))
            if raw[:8] == E.EVENT_IX_TAG and raw[8:16] == E.DISC_TRADE:
                try:
                    e = E.decode_trade(raw[8:])
                    if e.mint == m and e.virtual_token_reserves > 0: st_ = [e.virtual_sol_reserves, e.virtual_token_reserves]
                except Exception: pass
    return st_
OUT = {}; lock = threading.Lock()
def one(item):
    m, xs = item
    xs.sort(key=lambda x: (x[0], x[1]))
    b = next((x for x in xs if x[2][2] == 1), None)
    if not b: return
    s1 = next((x for x in xs if x[2][2] == 0 and x[:2] > b[:2]), None)
    bs = b[0]; hi = bs + max(GRID)
    bc = C.bonding_curve(m); before = None; sigs = []
    for _ in range(60):
        o = {"limit": 1000}
        if before: o["before"] = before
        page = tri.rpc("getSignaturesForAddress", [bc, o], cache=False) or []
        if not page: break
        sigs += [x for x in page if bs <= x["slot"] <= hi and not x.get("err")]
        if len(page) < 1000 or page[-1]["slot"] < bs: break
        before = page[-1]["signature"]
    states = {}
    for k in GRID:
        cand = [x for x in sigs if x["slot"] <= bs + k]          # newest first already
        for x in cand[:4]:
            s = last_trade_state(x["signature"], m)
            if s: states[k] = s; break
    rec = dict(buy_slot=bs, buy_post=stt(b[2]), sell1_slot=s1[0] if s1 else None, states=states)
    if s1:
        t = s1[2]; post = stt(t)
        if post: rec["sell1_pre"] = [post[0] + t[3], post[1] - t[4]]
    with lock: OUT[m] = rec
items = list(ev.items())
with ThreadPoolExecutor(16) as ex: list(ex.map(one, items))
json.dump(OUT, open(SP + "/w3/grid.json", "w"))
print(len(OUT), "coins")
