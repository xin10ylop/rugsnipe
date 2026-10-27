# The lamport-level key on his most recent buys (out of time: the rule was read on Oct 21-25). His own transactions
# over the last HOURS (compact rows, as the month pull), each launch buy's wallet drop -> the guard's x, against his real
# tip: precision and recall of "keyed" (tip >= 2M or size >= 2 SOL).
#   python3 -I guardlive.py [HOURS]
import sys, json, time, collections
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
HOURS = float(sys.argv[1]) if len(sys.argv) > 1 else 24
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, common as C, pumpparse as PP
from rugsnipe_analysis import events as E
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; W = A
src = open(SP + "/month/pull_month.py").read()
exec(src[src.index("STD = {"):src.index("if os.path.exists(DONEF)")])
exec(src[src.index("def compact(tx):"):src.index("f = open(OUT")])
RENTS = tuple(int(x) for x in __import__("os").environ.get("RENTS", "1513840,2164080,1488440").split(","))
lo = time.time() - HOURS * 3600; sigs = []; before = None
while True:
    o = {"limit": 1000}
    if before: o["before"] = before
    pg = tri.rpc("getSignaturesForAddress", [A, o], cache=False) or []
    sigs += [x for x in pg if not x.get("err") and (x.get("blockTime") or 0) >= lo]
    if len(pg) < 1000 or (pg[-1].get("blockTime") or 0) < lo: break
    before = pg[-1]["signature"]
def one(x):
    try:
        tx = tri.rpc("getTransaction", [x["signature"], {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}], cache=False)
        return compact(tx) if tx else None
    except Exception:
        return None
with ThreadPoolExecutor(8) as ex:
    R = [r for r in ex.map(one, sigs) if r]
c = collections.Counter(); res = collections.Counter(); mods = collections.Counter()
for r in R:
    tr = [t for t in r.get("tr", []) if t[1] == A]
    if len(tr) != 1 or tr[0][2] != 1: continue
    t = tr[0]; sol = t[3]; pf = (t[9] + t[10]) if len(t) > 10 else 0
    tip = r.get("tip") or 0; keyed = tip >= 2e6 or sol >= 2e9
    drop = -r["d"]; guess = None
    rest = drop - sol - pf - tip - r["fee"]; mods[rest] += 1
    if sol >= 2e9: guess = True
    else:
        for Rn in RENTS:
            x = (drop - 5000 - Rn) % 100_000_000
            if x < 60_000_000: guess = x >= 1_999_000 and x % 10_000 in (0, 9999); break
    c[(guess, keyed)] += 1
tp, fp, fn, tn = c[(True, True)], c[(True, False)], c[(False, True)], c[(False, False)]
print(f"his buys in the last {HOURS:.0f} h: {sum(c.values())} (keyed {tp + fn}); unread {c[(None, True)] + c[(None, False)]}")
print(f"   guard key: precision {tp / max(tp + fp, 1):.1%}  recall {tp / max(tp + fn, 1):.1%}   (tp {tp} fp {fp} fn {fn} tn {tn})")
print("   his drop beyond SOL in, pump fees, tip and tx fee (exact lamports), top:", mods.most_common(5))
