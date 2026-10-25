# A wallet's transactions in [T0, T1) over standard JSON-RPC (Triton; Helius is out of credits): signatures, then
# getTransaction for each, as the compact rows of month/pull_month.py -> six/<wallet[:8]>_recent.jsonl.
#   python3 -I six/pull_rpc.py WALLET T0 T1
import sys, os, json, time
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, common as C, pumpparse as PP
from rugsnipe_analysis import events as E
W, T0, T1 = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
src = open(SP + "/month/pull_month.py").read()
exec(src[src.index("STD = {"):src.index("if os.path.exists(DONEF)")])
exec(src[src.index("def compact(tx):"):src.index("f = open(OUT")])
OUT = f"{SP}/six/{W[:8]}_recent.jsonl"
sigs, before = [], None
while True:
    o = {"limit": 1000}
    if before: o["before"] = before
    page = tri.rpc("getSignaturesForAddress", [W, o], cache=False) or []
    sigs += [x["signature"] for x in page if T0 <= (x.get("blockTime") or 0) < T1]
    if len(page) < 1000 or (page[-1].get("blockTime") or 0) < T0: break
    before = page[-1]["signature"]
print(len(sigs), "signatures", flush=True)
def one(s):
    try:
        tx = tri.tx(s)
        return json.dumps(compact(tx), separators=(",", ":")) if tx else None
    except Exception as e:
        return None
with ThreadPoolExecutor(12) as ex, open(OUT, "w") as f:
    for k, line in enumerate(ex.map(one, sigs), 1):
        if line: f.write(line + "\n")
        if k % 500 == 0: print(k, "of", len(sigs), flush=True)
print("done", flush=True)
