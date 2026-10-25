# Coin metadata (the URI JSON) for the coins above 40 vSOL and every coin of the big tool signers: twitter handle,
# image host, description, website -> big/meta.json {mint: {...} | {"err": ...}}
import json, os, sys, glob, time, threading, urllib.request
from concurrent.futures import ThreadPoolExecutor
P = "big/meta.json"
out = json.load(open(P)) if os.path.exists(P) else {}
R = json.load(open("an_rows.json"))
want = [m for m, r in R.items() if (r.get("pb") or 0) > 40 or r["signer"].startswith(("8ZN71X", "7naFFw", "JDQKDr", "DRXPaf", "whamNN", "twamXG"))]
if len(sys.argv) > 1 and sys.argv[1] == "all": want = list(R)
todo = []
for m in want:
    if m in out and "err" not in out[m]: continue
    d = json.load(open(f"coin/{m}.json")); c = d.get("create") or {}
    if c.get("uri", "").startswith("http"): todo.append((m, c["uri"]))
print("to fetch", len(todo), flush=True)
lock = threading.Lock(); n = [0]
def one(a):
    m, u = a
    v = None
    for k in range(3):
        try:
            b = urllib.request.urlopen(urllib.request.Request(u, headers={"User-Agent": "Mozilla/5.0"}), timeout=10).read(65536)
            j = json.loads(b.decode("utf-8", "replace"))
            v = {k2: (str(j.get(k2))[:300] if j.get(k2) is not None else None) for k2 in ("name", "symbol", "description", "image", "twitter", "website", "telegram", "createdOn", "showName")}
            break
        except Exception as e:
            v = {"err": str(e)[:80]}; time.sleep(1 + k)
    with lock:
        out[m] = v; n[0] += 1
        if n[0] % 200 == 0:
            print(time.strftime("%H:%M:%S"), n[0], flush=True); json.dump(out, open(P, "w"))
with ThreadPoolExecutor(8) as ex: list(ex.map(one, todo))
json.dump(out, open(P, "w"))
print("done", len(out), "errors", sum(1 for v in out.values() if "err" in v), flush=True)
