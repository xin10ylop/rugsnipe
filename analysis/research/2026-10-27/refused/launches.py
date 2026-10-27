# The real number of in-class launches a day (what a ghost on every launch costs). Every pump create signs with the
# mint authority TSLvdd1p..: count its successful signatures over the last N hours, then classify a random sample as
# the bot does (create_v2, not mayhem, not cashback, SOL quote). Also the real fee of our refused buys.
#   python3 -I launches.py [HOURS] [SAMPLE]
import sys, json, random, collections, struct, time
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, "/home/user/rugsnipe/analysis"); sys.path.insert(0, SP + "/rivalpnl")
import tri, common as C
from rugsnipe_analysis import events as E
HOURS = float(sys.argv[1]) if len(sys.argv) > 1 else 6; NS = int(sys.argv[2]) if len(sys.argv) > 2 else 300
MA = "TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM"; PUMP = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
L = json.load(open(SP + "/refused/live.json"))["mine"]
fees = [a["fee"] for v in L.values() for a in v if a.get("code") == "6042"]
print(f"our refused buys: {len(fees)}, fee {min(fees)}-{max(fees)} lamports")
now = time.time(); lo = now - HOURS * 3600; sigs = []; before = None
while True:
    o = {"limit": 1000}
    if before: o["before"] = before
    pg = tri.rpc("getSignaturesForAddress", [MA, o], cache=False) or []
    sigs += [x for x in pg if not x.get("err") and (x.get("blockTime") or 0) >= lo]
    if len(pg) < 1000 or (pg[-1].get("blockTime") or 0) < lo: break
    before = pg[-1]["signature"]
span = (max(x["blockTime"] for x in sigs) - min(x["blockTime"] for x in sigs)) / 3600
print(f"creates over the last {span:.2f} h: {len(sigs)} ({len(sigs) / span * 24:.0f} a day)")
random.seed(1); smp = random.sample(sigs, min(NS, len(sigs)))
def cls(x):
    tx = tri.rpc("getTransaction", [x["signature"], {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}], cache=False)
    if not tx: return "no tx"
    logs = " ".join(tx["meta"]["logMessages"] or [])
    v2 = "Instruction: CreateV2" in logs
    if not v2: return "create v1"
    m = tx["transaction"]["message"]; la = tx["meta"].get("loadedAddresses") or {}
    keys = m["accountKeys"] + la.get("writable", []) + la.get("readonly", [])
    tail = None
    for ix in m["instructions"] + [i for g in tx["meta"].get("innerInstructions") or [] for i in g["instructions"]]:
        if keys[ix["programIdIndex"]] != PUMP: continue
        d = C.b58d(ix["data"])
        try:
            o = 8
            for _ in range(3): n = struct.unpack_from("<I", d, o)[0]; o += 4 + n
            o += 32; tail = d[o:o + 2]
            break
        except Exception:
            continue
    ev = [e for e in E.extract_events(tx) if isinstance(e, E.CreateEvent)]
    sol = ev[0].sol_quoted if ev else None
    if tail is None or len(tail) < 2: return "v2, args unread"
    if tail[0]: return "v2 mayhem"
    if tail[1]: return "v2 cashback"
    if sol is False: return "v2 other quote"
    return "IN CLASS"
with ThreadPoolExecutor(8) as ex:
    c = collections.Counter(ex.map(cls, smp))
n = sum(c.values()); k = c["IN CLASS"]
print("sample", n, dict(c.most_common()))
print(f"in-class share {k / n:.1%} -> about {len(sigs) / span * 24 * k / n:.0f} in-class launches a day; "
      f"one ghost each at {min(fees)} lamports: {len(sigs) / span * 24 * k / n * min(fees) / 1e9:.3f} SOL a day")
