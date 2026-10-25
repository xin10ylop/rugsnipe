# The coin mode flags (bytes after the creator in the create args: mayhem, cashback, creator_fee_bps, holder_reward) of
# every bundled Oct 23-24 coin, from its create transaction. -> cell/modes2324.json {mint: tail hex}
import sys, json, struct, glob, os, threading
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP, common as C
tri.MIN_GAP = 0.012
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
BW = json.load(open(SP + "/dec/bwall.json"))
ms = [m for m, b in BW.items() if b["bw"] and m in cr]
OUT = SP + "/dec/cell/modes2324.json"
res = json.load(open(OUT)) if os.path.exists(OUT) else {}
lock = threading.Lock()
def rstr(b, o):
    n = struct.unpack_from("<I", b, o)[0]; return o + 4 + n
def one(m):
    if m in res: return
    t = tri.tx(cr[m]["sig"])
    tail = None
    if t:
        ks = PP.keys_of(t)
        for prog, accs, data, parsed, oi, inner in PP._ixs(t, ks):
            if prog == C.PUMP and data[:8] in PP.IXR and PP.IXR[data[:8]].startswith("create") and accs and accs[0] == m:
                o = 8
                for _ in range(3): o = rstr(data, o)
                tail = data[o + 32:].hex(); break
    with lock: res[m] = tail
with ThreadPoolExecutor(8) as ex: list(ex.map(one, ms))
json.dump(res, open(OUT, "w"))
print("coins", len(ms), "decoded", sum(1 for v in res.values() if v is not None))
