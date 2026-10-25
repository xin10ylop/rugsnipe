# The create transaction (getTransaction, base64) of every candidate launch from Oct 23 00:00 (create_v2, not mayhem,
# dev buy >= 0.25 SOL): what the replay's gRPC path decodes. Reuses the 5-day export's; the rest fetched, not cached.
# -> ext/b64/<part>.jsonl lines {"mint","sig","tx"}
import sys, json, glob, os, time, threading
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); import tri
tri.MIN_GAP = 0.008
OUT = SP + "/ext/b64"; os.makedirs(OUT, exist_ok=True)
def log(*a): print(time.strftime("%H:%M:%S"), *a, flush=True)
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
cand = [m for m, c in cr.items() if "create_v2" in (c["ix"] or []) and not c.get("mayhem") and c["dev_sol"] >= 0.25]
have = set()
for p in glob.glob(OUT + "/*.jsonl"):
    for l in open(p):
        try: have.add(json.loads(l)["mint"])
        except Exception: pass
CT = json.load(open(SP + "/dec/five/replay/create_tx.json"))
part = open(f"{OUT}/{int(time.time())}.jsonl", "a"); lock = threading.Lock()
n_old = 0
for m in cand:
    if m in have: continue
    v = CT.get(m)
    if v and v[1]:
        part.write(json.dumps({"mint": m, "sig": v[0], "tx": v[1]}) + "\n"); have.add(m); n_old += 1
log("candidates", len(cand), "from the 5-day export", n_old, "to fetch", len(cand) - len(have))
todo = [m for m in cand if m not in have]
n = [0]
def one(m):
    sig = cr[m]["sig"]
    try: r = tri.rpc("getTransaction", [sig, {"encoding": "base64", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}], cache=False)
    except Exception: r = None
    with lock:
        if r: part.write(json.dumps({"mint": m, "sig": sig, "tx": r}) + "\n")
        n[0] += 1
        if n[0] % 2000 == 0: log(n[0], "of", len(todo)); part.flush()
with ThreadPoolExecutor(12) as ex: list(ex.map(one, todo))
part.close(); log("done")
