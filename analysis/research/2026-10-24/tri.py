# Triton RPC helper: URL read from month/.triton_url (0600, scratchpad only); every error message is redacted.
import json, os, time, hashlib, urllib.request, threading
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
URL = open(SP + "/month/.triton_url").read().strip()
TOKEN = URL.rsplit("/", 1)[-1]
CACHE = SP + "/month/tricache"; os.makedirs(CACHE, exist_ok=True)
_lock = threading.Lock(); _last = [0.0]; MIN_GAP = 0.055
def red(s): return str(s).replace(TOKEN, "<token>")
def rpc(method, params, cache=True):
    h = hashlib.sha1(json.dumps([method, params]).encode()).hexdigest(); p = os.path.join(CACHE, h + ".json")
    if cache and os.path.exists(p): return json.load(open(p))
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    for i in range(6):
        with _lock:
            w = _last[0] + MIN_GAP - time.time()
            if w > 0: time.sleep(w)
            _last[0] = time.time()
        try:
            with urllib.request.urlopen(urllib.request.Request(URL, body, {"Content-Type": "application/json"}), timeout=60) as r: j = json.load(r)
            if "error" in j: raise RuntimeError(red(j["error"])[:200])
            res = j["result"]; break
        except Exception as e:
            if i == 5: raise RuntimeError(red(e)) from None
            time.sleep(1.0 * (i + 1))
    if cache and res is not None: json.dump(res, open(p, "w"))
    return res
def tx(sig): return rpc("getTransaction", [sig, {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}])
def sigs(addr, before=None, limit=1000, until=None):
    o = {"limit": limit}
    if before: o["before"] = before
    if until: o["until"] = until
    return rpc("getSignaturesForAddress", [addr, o], cache=False) or []
