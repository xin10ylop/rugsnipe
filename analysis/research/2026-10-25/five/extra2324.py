# Coin records for the Oct 23 00:00 - Oct 24 11:00 launches of listed signers that neither wallet touched: from the
# existing full tapes where there is one (full/, fp/), else fetched (the listing paged back to the create slot); the
# create transaction in base64 for the replay. -> five/coin/<mint>.json (extra=True), five/replay/create_tx.json
import sys, json, os, time, collections, threading, shutil
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/dec/five"); sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import coin5b as K
import tri, pumpparse as PP, common as C
tri.MIN_GAP = 0.01
F = SP + "/dec/five"
EX = json.load(open(F + "/extra_2324.json"))
CTP = F + "/replay/create_tx.json"; CT = json.load(open(CTP)) if os.path.exists(CTP) else {}
lock = threading.Lock(); cnt = collections.Counter()
def from_tape(c, tape):
    rows = []
    for s, i, sg, evs in tape["rows"]:
        for (u, b, sol, tok, vs, vt, f) in evs: rows.append([s, i, u, int(b), sol, tok, vs, vt, f, sg])
    return rows
def one(c):
    m = c["mint"]; p = f"{F}/coin/{m}.json"
    if shutil.disk_usage(F).free < 1_500_000_000: return "disk"
    tx = CT.get(m, [None, None])[1] or tri.rpc("getTransaction", [c["sig"], {"encoding": "base64", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}])
    with lock: CT[m] = [c["sig"], tx]
    if os.path.exists(p): return "have"
    out = None
    for d in ("full", "fp"):
        tp = f"{SP}/{d}/{m}.json"
        if os.path.exists(tp):
            tape = json.load(open(tp))
            out = dict(mint=m, cs=c["slot"], reached=True, extra=True, rows=from_tape(c, tape), failed=[])
            break
    if out is None:
        bc = C.bonding_curve(m); sigs = []; before = None; reached = False
        for _ in range(12):
            o = {"limit": 1000}
            if before: o["before"] = before
            s = tri.rpc("getSignaturesForAddress", [bc, o], cache=False) or []
            sigs += s
            if len(s) < 1000 or s[-1]["slot"] <= c["slot"]: reached = True; break
            before = s[-1]["signature"]
        cs = c["slot"]; rows = []; failed = []; tmp = {}
        sigs = [x for x in sigs if x["slot"] <= cs + 12]
        for x in sigs:
            if x["slot"] <= cs + 1:
                t = tri.rpc("getTransaction", [x["signature"], {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}], cache=False)
                if t: K.decode(t, m, tmp, rows, failed)
        by = collections.defaultdict(list)
        for x in sigs:
            if cs + 1 < x["slot"] and not x.get("err"): by[x["slot"]].append(x["signature"])
        for sl, ss in by.items():
            t = tri.rpc("getTransaction", [ss[0], {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}], cache=False)
            if t: K.decode(t, m, tmp, rows, failed)
        rows.sort(key=lambda r: (r[0], r[1]))
        out = dict(mint=m, cs=cs, reached=reached, extra=True, rows=rows, failed=sorted(failed))
    # the create record from the create transaction itself (as coin5b does)
    t = tri.rpc("getTransaction", [c["sig"], {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}])
    if t:
        tmp = {}; r2 = []; f2 = []
        K.decode(t, m, tmp, r2, f2)
        if "create" in tmp: out["create"] = tmp["create"]
        if not out["rows"]: out["rows"] = r2
    if "create" not in out: return "nocreate"
    json.dump(out, open(p + ".tmp", "w")); os.replace(p + ".tmp", p)
    return "ok"
def safe(c):
    try: r = one(c)
    except Exception as e: r = "error"
    with lock:
        cnt[r] += 1
        if sum(cnt.values()) % 250 == 0: print(time.strftime("%H:%M:%S"), dict(cnt), flush=True)
with ThreadPoolExecutor(12) as ex: list(ex.map(safe, EX))
json.dump(CT, open(CTP, "w"))
print("done", dict(cnt), flush=True)
