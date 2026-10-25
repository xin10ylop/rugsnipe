# His buy attempts before Sep 24 (Aug 2026 -> Sep 24): each older signature's transaction -> (slot, mint, failed).
# -> hist/older_att.json; then the full attempt history Aug -> Oct 23 = this + month/AHwJSnFN.jsonl.
import sys, json, time, threading
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP, common as C
tri.MIN_GAP = 0.012
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"
S = json.load(open(SP + "/dec/hist/older_sigs.json"))
out = []; lock = threading.Lock()
def one(x):
    slot, bt, failed, sig = x
    t = tri.tx(sig)
    if not t: return
    ks = PP.keys_of(t)
    if ks[0] != A: return
    for prog, accs, data, parsed, oi, inner in PP._ixs(t, ks):
        if prog == C.PUMP and not inner and data[:8] in PP.IXR and PP.IXR[data[:8]].startswith("buy") and len(accs) > 2:
            with lock: out.append([slot, accs[2], failed])
            return
with ThreadPoolExecutor(8) as ex: list(ex.map(one, S))
json.dump(out, open(SP + "/dec/hist/older_att.json", "w"))
print("older buy attempts", len(out), "mints", len({m for s, m, f in out}))
