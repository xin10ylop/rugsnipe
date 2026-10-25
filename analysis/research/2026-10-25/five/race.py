# The full auction for the first spot behind the bundle on a set of coins: every transaction in the create slot and
# the next (landed or failed) that touches the curve, with its signer, block index, tip transfers and priority fee.
# Reads the RPC cache coin5b filled; a miss is fetched without caching (disk). -> big/race.json {mint: [rows]}
import sys, json, os, struct, hashlib, collections
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP, common as C
tri.MIN_GAP = 0.02
SYS = "11111111111111111111111111111111"; CB = "ComputeBudget111111111111111111111111111111"
def cached(method, params):
    h = hashlib.sha1(json.dumps([method, params]).encode()).hexdigest(); p = os.path.join(tri.CACHE, h + ".json")
    if os.path.exists(p): return json.load(open(p))
    return tri.rpc(method, params, cache=False)
def tx(sig): return cached("getTransaction", [sig, {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}])
def page(bc, before): return cached("getSignaturesForAddress", [bc, {"limit": 1000, "before": before}]) or []
W = {}
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
for w in (A, US):
    for r in json.load(open(f"walk_{w[:6]}.json"))["rows"]:
        if r["kind"] == "buy": W.setdefault(r["m"], []).append(r)
def bid_of(t):
    ks = PP.keys_of(t); msg = t["transaction"]["message"]; payer = ks[0]
    tip = 0; dst = {}; cup = 0; cul = 200000
    for ix in msg["instructions"]:
        prog = ks[ix["programIdIndex"]]; data = C.b58d(ix.get("data", ""))
        if prog == SYS and len(data) >= 12 and data[:4] == b"\x02\x00\x00\x00":
            accs = ix.get("accounts") or []
            if len(accs) >= 2 and ks[accs[0]] == payer:
                lam = struct.unpack_from("<Q", data, 4)[0]; tip += lam; dst[ks[accs[1]][:8]] = lam
        elif prog == CB and data:
            if data[0] == 3 and len(data) >= 9: cup = struct.unpack_from("<Q", data, 1)[0]
            elif data[0] == 2 and len(data) >= 5: cul = struct.unpack_from("<I", data, 1)[0]
    return payer, tip, cup * cul // 1_000_000, dst
def one(m):
    d = json.load(open(f"coin/{m}.json")); cs = d["cs"]; bc = C.bonding_curve(m)
    mine = sorted(W.get(m, []), key=lambda r: (r["slot"], r["idx"]))
    if not mine: return m, None
    lst = page(bc, mine[0]["sig"])
    sigs = [x for x in lst if x["slot"] <= cs + 1] + [dict(signature=r["sig"], slot=r["slot"]) for r in mine if r["slot"] <= cs + 1]
    seen = set(); out = []
    for x in sigs:
        s = x["signature"]
        if s in seen: continue
        seen.add(s)
        t = tx(s)
        if not t: continue
        payer, tip, prio, dst = bid_of(t)
        out.append(dict(slot=t["slot"] - cs, idx=t.get("transactionIndex") or 0, payer=payer, tip=tip, prio=prio, ok=(t.get("meta") or {}).get("err") is None,
                        fee=(t.get("meta") or {}).get("fee"), dst=dst))
    out.sort(key=lambda r: (r["slot"], r["idx"]))
    return m, out
if __name__ == "__main__":
    ms = [l.strip() for l in open(sys.argv[1])]
    P = "big/race.json"
    res = json.load(open(P)) if os.path.exists(P) else {}
    todo = [m for m in ms if m not in res]
    print("coins", len(todo), flush=True)
    with ThreadPoolExecutor(8) as ex:
        for i, (m, o) in enumerate(ex.map(one, todo)):
            res[m] = o
            if i % 100 == 0: print(i, flush=True); json.dump(res, open(P, "w"))
    json.dump(res, open(P, "w")); print("done", len(res))
