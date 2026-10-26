# The bids around each copied buy: for every landed buy in the copied buy's slot and the next one, on every coin we sent
# a copy on, the buyer, its block index, its priority fee (compute-unit price x limit) and its tips (top-level system
# transfers from the signer, the curve excluded), and ours. Who lands between the copied buy and ours, and what they pay.
import sys, json, collections
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, "/home/user/rugsnipe/analysis/scripts/strategy"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import operators_refresh as orf, common as C
api = orf.Api(open(SP + "/month/.triton_url").read().strip(), 25.0)
D = json.load(open(SP + "/fails/coins.json")); EACH = {r["m"]: r for r in json.load(open(SP + "/fails/each.json"))}
SYS = "11111111111111111111111111111111"; CB = "ComputeBudget111111111111111111111111111111"
def fees(tx):
    m, meta = tx["transaction"]["message"], tx["meta"]
    la = meta.get("loadedAddresses") or {}; ks = list(m["accountKeys"]) + la.get("writable", []) + la.get("readonly", [])
    price = limit = 0; tip = 0
    for ix in m.get("instructions") or []:
        prog = ks[ix["programIdIndex"]]; d = orf.b58d(ix.get("data", ""))
        if prog == CB and d:
            if d[0] == 3 and len(d) >= 9: price = int.from_bytes(d[1:9], "little")
            if d[0] == 2 and len(d) >= 5: limit = int.from_bytes(d[1:5], "little")
        if prog == SYS and ix.get("accounts") and ks[ix["accounts"][0]] == ks[0] and len(d) == 12 and d[:4] == b"\x02\x00\x00\x00":
            tip += int.from_bytes(d[4:12], "little")
    return ks[0], price * (limit or 200_000) // 1_000_000, tip, meta.get("fee") or 0
def one(mint):
    e = EACH.get(mint[:8]); tp = D["tapes"].get(mint)
    if not e or not tp: return mint, None
    lead_slot = tp["c0"] + e["lead_at"]
    bc = orf.bonding_curve(mint); out = []; before = None; ss = []
    for _ in range(80):
        o = {"limit": 1000}
        if before: o["before"] = before
        page = api.rpc("getSignaturesForAddress", [bc, o]) or []
        ss += [x for x in page if lead_slot <= x["slot"] <= lead_slot + 1]
        if len(page) < 1000 or page[-1]["slot"] < lead_slot: break
        before = page[-1]["signature"]
    for x in ss:
        tx = api.rpc("getTransaction", [x["signature"], orf.TX_OPTS], cache=True)
        if not tx: continue
        signer, prio, tip, fee = fees(tx)
        out.append([tx["slot"] - tp["c0"], tx.get("transactionIndex") or 0, signer, prio, tip, fee, x.get("err") is not None])
    out.sort(key=lambda r: (r[0], r[1]))
    return mint, out
with ThreadPoolExecutor(8) as ex:
    B = dict(ex.map(one, [m for m in D["mine"] if m[:8] in EACH]))
json.dump(B, open(SP + "/fails/bids.json", "w"))
print("done", len(B))
