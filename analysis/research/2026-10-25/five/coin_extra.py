# Coin records (as coin5b.py) for launches neither wallet touched: the listing paged from the newest back to the create
# slot (they are mostly small), then the create slot and the next in full and one sample per later slot.
import sys, json, os, time, collections, threading
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/dec/five")
import coin5b as K
import tri, common as C
F = SP + "/dec/five"
ex = json.load(open(F + "/signer_creates.json"))["extra"]
def one(m):
    p = f"{F}/coin/{m}.json"
    if os.path.exists(p): return "have"
    cs, csig = ex[m]
    bc = C.bonding_curve(m); sigs = []; before = None; reached = False
    for _ in range(20):
        o = {"limit": 1000}
        if before: o["before"] = before
        s = tri.rpc("getSignaturesForAddress", [bc, o], cache=False) or []
        sigs += s
        if len(s) < 1000 or s[-1]["slot"] <= cs: reached = True; break
        before = s[-1]["signature"]
    sigs = [x for x in sigs if x["slot"] <= cs + 12]
    if not any(x["signature"] == csig for x in sigs): sigs.append(dict(signature=csig, slot=cs))
    out = dict(mint=m, cs=cs, reached=reached, extra=True); rows = []; failed = []
    for x in sigs:
        if x["slot"] <= cs + 1:
            t = tri.tx(x["signature"])
            if t: K.decode(t, m, out, rows, failed)
    by = collections.defaultdict(list)
    for x in sigs:
        if cs + 1 < x["slot"] <= cs + 12 and not x.get("err"): by[x["slot"]].append(x["signature"])
    for sl, ss in by.items():
        t = tri.tx(ss[0])
        if t: K.decode(t, m, out, rows, failed)
    rows.sort(key=lambda r: (r[0], r[1])); out["rows"] = rows; out["failed"] = sorted(failed)
    json.dump(out, open(p + ".tmp", "w")); os.replace(p + ".tmp", p)
    return "ok" if "create" in out else "nocreate"
cnt = collections.Counter(); lock = threading.Lock()
def safe(m):
    try: r = one(m)
    except Exception as e: r = "error"
    with lock:
        cnt[r] += 1
        if sum(cnt.values()) % 200 == 0: print(time.strftime("%H:%M:%S"), dict(cnt), flush=True)
with ThreadPoolExecutor(12) as exr: list(exr.map(safe, sorted(ex)))
print("done", dict(cnt), flush=True)
