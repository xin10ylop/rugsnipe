# The curve after a dump: for each event (flush/events_*.json, or an events file given), every landed signature of the
# coin's curve from the dump slot to + WIN slots, grouped in buckets of B slots; the last transaction of each bucket is
# fetched and its last trade on the coin gives the curve state at the bucket's end. Also how many landed and failed
# transactions each bucket had (activity). -> flush/path/<mint>.json  {dump_slot, buckets: [[slot, v, t, n_ok, n_err]], trunc}
#   python3 -I flush/paths.py EVENTS.json [WIN] [B]
import sys, json, os, collections, threading, time
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, common as C
from rugsnipe_analysis import events as E
tri.MIN_GAP = 0.012
EV = json.load(open(sys.argv[1])); WIN = int(sys.argv[2]) if len(sys.argv) > 2 else 450; B = int(sys.argv[3]) if len(sys.argv) > 3 else 3
OUT = SP + "/flush/path"; os.makedirs(OUT, exist_ok=True)
def coin_slots(m):
    for base in ("wide", "oos"):
        p = f"{SP}/{base}/coin/{m}.json"
        if os.path.exists(p): return json.load(open(p))["cs"]
    return None
def one(e):
    m = e["m"]; p = f"{OUT}/{m}.json"
    if os.path.exists(p): return "have"
    cs = coin_slots(m); s0 = cs + e["off"]; s1 = s0 + WIN
    bc = C.bonding_curve(m); before = None; sigs = []; trunc = True
    for _ in range(60):
        o = {"limit": 1000}
        if before: o["before"] = before
        page = tri.rpc("getSignaturesForAddress", [bc, o], cache=False) or []
        if not page: trunc = False; break
        sigs += [x for x in page if s0 <= x["slot"] <= s1]
        if len(page) < 1000 or page[-1]["slot"] < s0: trunc = False; break
        before = page[-1]["signature"]
    by = collections.defaultdict(list)
    for x in sigs: by[(x["slot"] - s0) // B].append(x)
    out = []
    for k in sorted(by):
        xs = by[k]; ok = [x for x in xs if not x.get("err")]
        if not ok: out.append([s0 + k * B, None, None, 0, len(xs)]); continue
        # newest slot first; within a slot the listing is newest first. Step back until a trade on the coin decodes.
        order = sorted(range(len(ok)), key=lambda i: (-ok[i]["slot"], i))
        v = tt = None
        for i in order[:4]:
            t = tri.rpc("getTransaction", [ok[i]["signature"], {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}], cache=False)
            if not t: continue
            ks = t["transaction"]["message"]["accountKeys"] + (t["meta"].get("loadedAddresses") or {}).get("writable", []) + (t["meta"].get("loadedAddresses") or {}).get("readonly", [])
            for g in t["meta"].get("innerInstructions") or []:
                for ix in g["instructions"]:
                    if ks[ix["programIdIndex"]] != C.PUMP: continue
                    raw = C.b58d(ix.get("data", ""))
                    if raw[:8] == E.EVENT_IX_TAG and raw[8:16] == E.DISC_TRADE:
                        try:
                            ev = E.decode_trade(raw[8:])
                            if ev.mint == m: v, tt = ev.virtual_sol_reserves, ev.virtual_token_reserves
                        except Exception: pass
            if v is not None: break
        out.append([s0 + k * B, v, tt, len(ok), len(xs) - len(ok)])
    json.dump(dict(mint=m, dump_slot=s0, win=WIN, b=B, buckets=out, trunc=trunc), open(p + ".tmp", "w")); os.replace(p + ".tmp", p)
    return "trunc" if trunc else "ok"
cnt = collections.Counter(); lock = threading.Lock()
def safe(e):
    try: r = one(e)
    except Exception: r = "error"
    with lock:
        cnt[r] += 1
        if sum(cnt.values()) % 100 == 0: print(time.strftime("%H:%M:%S"), dict(cnt), flush=True)
with ThreadPoolExecutor(16) as ex: list(ex.map(safe, EV))
print(time.strftime("%H:%M:%S"), "done", dict(cnt), flush=True)
