# Each night launch's bundle wallets: their last SOL inflow before the create (source, amount, slots before), from the
# wallets' own transactions in the window. -> cell/bfund.json {mint: [[wallet, source, sol, slots_before], ...]}
import sys, json, os, time, collections, threading, bisect
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP
tri.MIN_GAP = 0.015
N = SP + "/dec/night"; TW, T1 = 1792843200, 1792919600
S = json.load(open(SP + "/dec/cell/prebuy.json")); ops = json.load(open(N + "/ops.json"))
W = sorted({w for m, d in S.items() for w in ops[d["op"]][m]["wallets"]})
inflow = {}; lock = threading.Lock()
def one(w):
    out = []; before = None
    for page in range(12):
        o = {"limit": 1000}
        if before: o["before"] = before
        s = tri.rpc("getSignaturesForAddress", [w, o], cache=False) or []
        for x in s:
            if (x.get("blockTime") or 0) < TW - 3600 or (x.get("blockTime") or 0) >= T1 or x.get("err"): continue
            t = tri.tx(x["signature"])
            if not t: continue
            for src, dst, lam, inner in PP.analyze(t)["sysxfers"]:
                if dst == w and src != w and lam >= 10_000_000: out.append([t["slot"], src, lam / 1e9])
        if len(s) < 1000 or (s[-1].get("blockTime") or 0) < TW - 3600: break
        before = s[-1]["signature"]
    with lock: inflow[w] = sorted(out)
with ThreadPoolExecutor(8) as ex: list(ex.map(one, W))
res = {}
for m, d in S.items():
    rows = []
    for w in ops[d["op"]][m]["wallets"]:
        xs = [x for x in inflow.get(w, []) if x[0] < d["cs"]]
        if xs: rows.append([w, xs[-1][1], round(xs[-1][2], 3), d["cs"] - xs[-1][0]])
        else: rows.append([w, None, None, None])
    res[m] = rows
json.dump(res, open(SP + "/dec/cell/bfund.json", "w")); json.dump(inflow, open(SP + "/dec/cell/inflow.json", "w"))
print("wallets", len(W), "with inflows", sum(1 for v in inflow.values() if v))
