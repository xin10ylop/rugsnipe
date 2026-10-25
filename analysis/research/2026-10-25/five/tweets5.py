# The tweet each coin names in its metadata (the bot's tweet check, crates/rugsnipe/src/tweet.rs): status id or none,
# for every coin of the 5-day set on any http(s) host. -> five/tweets.json {mint: sid | 0 (no tweet) | -1 (fetch failed)}
import json, glob, os, re, time, threading, urllib.request
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"; F = SP + "/dec/five"
P = F + "/tweets.json"
out = json.load(open(P)) if os.path.exists(P) else {}
todo = []
for p in glob.glob(F + "/coin/*.json"):
    d = json.load(open(p)); c = d.get("create")
    if c and c.get("uri", "").startswith("http") and d["mint"] not in out: todo.append((d["mint"], c["uri"]))
print("to fetch", len(todo), flush=True)
RX = re.compile(r'(?:twitter\.com|x\.com)/[^/"\s]+/status(?:es)?/(\d{8,20})')
lock = threading.Lock(); n = [0]
def one(a):
    m, u = a
    try:
        b = urllib.request.urlopen(urllib.request.Request(u, headers={"User-Agent": "Mozilla/5.0"}), timeout=10).read(65536).decode("utf-8", "replace")
        mm = RX.search(b.replace("\\/", "/"))
        v = int(mm.group(1)) if mm else 0
    except Exception:
        v = -1
    with lock:
        out[m] = v; n[0] += 1
        if n[0] % 500 == 0:
            print(time.strftime("%H:%M:%S"), n[0], flush=True); json.dump(out, open(P, "w"))
with ThreadPoolExecutor(16) as ex: list(ex.map(one, todo))
json.dump(out, open(P, "w"))
print("done", len(out), "tweets", sum(1 for v in out.values() if v > 0), "none", sum(1 for v in out.values() if v == 0), "failed", sum(1 for v in out.values() if v < 0), flush=True)
