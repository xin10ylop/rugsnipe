# Public-RPC helper (no key): getTransaction / getSignaturesForAddress with a disk cache, throttle and endpoint rotation.
import json, os, time, hashlib, urllib.request, urllib.error, itertools, threading
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
CACHE = SP + "/month/pubcache"; os.makedirs(CACHE, exist_ok=True)
URLS = itertools.cycle(["https://solana-rpc.publicnode.com", "https://api.mainnet-beta.solana.com"])
_lock = threading.Lock(); _last = [0.0]; MIN_GAP = 0.6
def rpc(method, params, cache=True):
    h = hashlib.sha1(json.dumps([method, params]).encode()).hexdigest(); p = os.path.join(CACHE, h + ".json")
    if cache and os.path.exists(p): return json.load(open(p))
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    for i in range(6):
        with _lock:
            w = _last[0] + MIN_GAP - time.time()
            if w > 0: time.sleep(w)
            _last[0] = time.time(); url = next(URLS)
        try:
            with urllib.request.urlopen(urllib.request.Request(url, body, {"Content-Type": "application/json"}), timeout=60) as r: j = json.load(r)
            if "error" in j: raise RuntimeError(str(j["error"])[:200])
            res = j["result"]; break
        except Exception as e:
            if i == 5: raise
            time.sleep(1.5 * (i + 1))
    if cache and res is not None: json.dump(res, open(p, "w"))
    return res
def tx(sig): return rpc("getTransaction", [sig, {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}])
