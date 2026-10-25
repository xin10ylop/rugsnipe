# His intended bid on every buy attempt (the tip transfers and the compute-unit price in the transaction itself, so a
# failed buy shows it too) -> five/bids.json {sig: [tip_lamports, cu_price_micro, cu_limit, tip_accounts]}
import sys, json, collections, struct
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP, common as C
F = SP + "/dec/five"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
SYS = "11111111111111111111111111111111"; CB = "ComputeBudget111111111111111111111111111111"
out = {}
def one(r, who):
    t = tri.tx(r["sig"])
    if not t: return None
    ks = PP.keys_of(t); msg = t["transaction"]["message"]
    tip = 0; dst = collections.Counter(); cup = None; cul = None
    for ix in msg["instructions"]:
        prog = ks[ix["programIdIndex"]]; data = C.b58d(ix.get("data", ""))
        if prog == SYS and len(data) >= 12 and data[:4] == b"\x02\x00\x00\x00":
            lam = struct.unpack_from("<Q", data, 4)[0]
            accs = ix.get("accounts") or []
            if len(accs) >= 2 and ks[accs[0]] == who:
                tip += lam; dst[ks[accs[1]][:8]] += lam
        elif prog == CB and data:
            if data[0] == 3 and len(data) >= 9: cup = struct.unpack_from("<Q", data, 1)[0]
            elif data[0] == 2 and len(data) >= 5: cul = struct.unpack_from("<I", data, 1)[0]
    return r["sig"], [tip, cup, cul, dict(dst)]
for who in (A, US):
    rows = [r for r in json.load(open(f"{F}/walk_{who[:6]}.json"))["rows"] if r["kind"] == "buy"]
    with ThreadPoolExecutor(12) as ex:
        for res in ex.map(lambda r: one(r, who), rows):
            if res: out[res[0]] = res[1]
json.dump(out, open(F + "/bids.json", "w"))
# his bid levels
rows = [r for r in json.load(open(f"{F}/walk_AHwJSn.json"))["rows"] if r["kind"] == "buy"]
c = collections.Counter()
for r in rows:
    b = out.get(r["sig"])
    if not b: continue
    lvl = round(b[0] / 1e6, 1)
    c[(lvl, r["ok"])] += 1
print("his tips (M lamports) x landed:", sorted(c.items(), key=lambda kv: -kv[1])[:16])
dsts = collections.Counter()
for r in rows:
    b = out.get(r["sig"])
    if b:
        for d in b[3]: dsts[d] += 1
print("his tip accounts:", dsts.most_common(8))
rows = [r for r in json.load(open(f"{F}/walk_4g49xG.json"))["rows"] if r["kind"] == "buy"]
c = collections.Counter()
for r in rows:
    b = out.get(r["sig"])
    if b: c[(round(b[0] / 1e6, 1), r["ok"])] += 1
print("our tips (M lamports) x landed:", sorted(c.items(), key=lambda kv: -kv[1])[:12])
