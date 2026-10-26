# Out of sample: a wallet's landed transactions since T0 (Oct 25 18:00 UTC by default), compact rows as the month pull
# (pump trades with reserves, tips, fees) -> oos/<wallet[:8]>.jsonl. Failed signatures are skipped (no call); raw
# transactions are not cached (disk).   python3 -I oos/pull.py WALLET [T0]
import sys, os, json
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, common as C, pumpparse as PP
from rugsnipe_analysis import events as E
W = sys.argv[1]; T0 = int(sys.argv[2]) if len(sys.argv) > 2 else 1792951200
src = open(SP + "/month/pull_month.py").read()
exec(src[src.index("STD = {"):src.index("if os.path.exists(DONEF)")])
exec(src[src.index("def compact(tx):"):src.index("f = open(OUT")])
OUT = f"{SP}/oos/{W[:8]}.jsonl"
sigs, before = [], None
while True:
    o = {"limit": 1000}
    if before: o["before"] = before
    page = tri.rpc("getSignaturesForAddress", [W, o], cache=False) or []
    sigs += [x["signature"] for x in page if (x.get("blockTime") or 0) >= T0 and not x.get("err")]
    if len(page) < 1000 or (page[-1].get("blockTime") or 0) < T0: break
    before = page[-1]["signature"]
print(W[:8], len(sigs), "landed signatures", flush=True)
def one(s):
    try:
        tx = tri.rpc("getTransaction", [s, {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}], cache=False)
        return json.dumps(compact(tx), separators=(",", ":")) if tx else None
    except Exception:
        return None
with ThreadPoolExecutor(12) as ex, open(OUT + ".tmp", "w") as f:
    for line in ex.map(one, sigs):
        if line: f.write(line + "\n")
os.replace(OUT + ".tmp", OUT)
print(W[:8], "done", flush=True)
