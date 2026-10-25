# Both wallets' transactions Oct 21 00:00 UTC -> now: every buy (landed / failed, error code), every sell, the wallet's
# own SOL delta per transaction, the fill (sol, tokens, reserves after). -> five/walk_<wallet6>.json
import sys, json, os, time, datetime, threading
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP, common as C
from rugsnipe_analysis import events as E
tri.MIN_GAP = 0.012
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
WSOL = "So11111111111111111111111111111111111111112"
T0 = int(datetime.datetime(2026, 10, 21, 0, 0, tzinfo=datetime.timezone.utc).timestamp()); T1 = int(sys.argv[1]) if len(sys.argv) > 1 else int(time.time()) - 60
OUT = SP + "/dec/five"
def log(*a): print(time.strftime("%H:%M:%S"), *a, flush=True)
def sigs(addr, lo, hi, pages=200):
    out = []; before = None
    for _ in range(pages):
        o = {"limit": 1000}
        if before: o["before"] = before
        s = tri.rpc("getSignaturesForAddress", [addr, o], cache=False) or []
        out += [x for x in s if lo <= (x.get("blockTime") or 0) < hi]
        if len(s) < 1000 or (s[-1].get("blockTime") or 0) < lo: break
        before = s[-1]["signature"]
    return out
def walk(addr):
    P = f"{OUT}/walk_{addr[:6]}.json"
    if os.path.exists(P) and json.load(open(P))["T1"] == T1: return
    ss = sigs(addr, T0, T1); log(addr[:6], "signatures", len(ss))
    rows = []; lock = threading.Lock(); n = [0]
    def one(x):
        t = tri.tx(x["signature"])
        if not t: return
        ks = PP.keys_of(t)
        if ks[0] != addr: return
        meta = t.get("meta") or {}; err = meta.get("err")
        kind = None; m = None
        for prog, accs, data, parsed, oi, inner in PP._ixs(t, ks):
            if prog == C.PUMP and not inner and data and data[:8] in PP.IXR:
                nm = PP.IXR[data[:8]]
                if nm.startswith("buy"): kind = "buy"; m = m or (accs[2] if len(accs) > 2 else None)
                elif nm.startswith("sell"): kind = "sell"; m = m or (accs[2] if len(accs) > 2 else None)
        if not kind: return
        if m == WSOL or m is None:
            tb = [b["mint"] for b in (meta.get("postTokenBalances") or []) + (meta.get("preTokenBalances") or []) if b["mint"] != WSOL]
            m = tb[0] if tb else None
        if not m or m == WSOL: return
        fill = None
        for prog, accs, data, parsed, oi, inner in PP._ixs(t, ks):
            if prog == C.PUMP and data and data[:8] == E.EVENT_IX_TAG and data[8:16] == E.DISC_TRADE:
                try: e = E.decode_trade(data[8:])
                except Exception: continue
                if e.user == addr and e.mint == m:
                    fill = [e.sol_amount, e.token_amount, e.virtual_sol_reserves, e.virtual_token_reserves, (e.fee or 0) + (e.creator_fee or 0)]
        code = None
        if err:
            try: code = err["InstructionError"][1].get("Custom") if isinstance(err["InstructionError"][1], dict) else str(err["InstructionError"][1])
            except Exception: code = str(err)[:30]
        delta = meta["postBalances"][0] - meta["preBalances"][0]
        with lock:
            rows.append(dict(m=m, kind=kind, ok=err is None, code=code, slot=t["slot"], idx=t.get("transactionIndex") or 0, bt=x.get("blockTime"), delta=delta, fill=fill, fee=meta.get("fee", 0), sig=x["signature"]))
            n[0] += 1
            if n[0] % 2000 == 0: log(addr[:6], n[0], "of", len(ss))
    with ThreadPoolExecutor(10) as ex: list(ex.map(one, ss))
    json.dump(dict(T0=T0, T1=T1, rows=rows), open(P, "w"))
    log(addr[:6], "rows", len(rows))
walk(US); walk(A)
