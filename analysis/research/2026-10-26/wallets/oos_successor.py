# Is a wallet that received a finalist's SOL a trading wallet now (its successor)? Its newest 300 signatures, a sample
# of 25 landed transactions: pump buys / sells, sizes, first and last time.   python3 -I oos/successor.py ADDR ...
import sys, json, time, datetime
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, common as C
from rugsnipe_analysis import events as E
def ts(t): return datetime.datetime.fromtimestamp(t, datetime.timezone.utc).strftime("%m-%d %H:%M") if t else "-"
for a in sys.argv[1:]:
    s = tri.rpc("getSignaturesForAddress", [a, {"limit": 300}], cache=False) or []
    ok = [x for x in s if not x.get("err")]
    def one(x):
        t = tri.rpc("getTransaction", [x["signature"], {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}], cache=False)
        out = []
        if not t: return out
        ks = t["transaction"]["message"]["accountKeys"] + (t["meta"].get("loadedAddresses") or {}).get("writable", []) + (t["meta"].get("loadedAddresses") or {}).get("readonly", [])
        for g in t["meta"].get("innerInstructions") or []:
            for ix in g["instructions"]:
                if ks[ix["programIdIndex"]] != C.PUMP: continue
                raw = C.b58d(ix.get("data", ""))
                if raw[:8] == E.EVENT_IX_TAG and raw[8:16] == E.DISC_TRADE:
                    try:
                        e = E.decode_trade(raw[8:])
                        if e.user == a: out.append((int(e.is_buy), e.sol_amount / 1e9))
                    except Exception: pass
        return out
    with ThreadPoolExecutor(8) as ex: tr = [y for r in ex.map(one, ok[:25]) for y in r]
    b = [v for k, v in tr if k == 1]
    print(f"{a[:8]}: {len(s)} sigs (newest {ts(s[0].get('blockTime')) if s else '-'}, oldest of them {ts(s[-1].get('blockTime')) if s else '-'}), failed {len(s) - len(ok)}; "
          f"sample of 25: {len(b)} pump buys (median {sorted(b)[len(b) // 2] if b else 0:.2f} SOL), {len(tr) - len(b)} sells")
