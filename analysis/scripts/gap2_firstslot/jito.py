import json, os, time, random, threading, requests
from concurrent.futures import ThreadPoolExecutor
smp = json.load(open('sample.json'))['sample']
OUT = 'jito.jsonl'
done = set()
if os.path.exists(OUT):
    for l in open(OUT): done.add(json.loads(l)['sig'])
lock = threading.Lock()
def get(url):
    d = 1
    for a in range(8):
        try:
            r = requests.get(url, timeout=30)
            if r.status_code == 429 or r.status_code >= 500: raise RuntimeError(r.status_code)
            return r.json()
        except Exception as e:
            time.sleep(d + random.random()); d = min(d * 2, 30)
    return {"error": "failed"}
def one(s):
    sig = s['sig']
    j = get(f"https://bundles.jito.wtf/api/v1/bundles/transaction/{sig}")
    rec = {"sig": sig, "raw": j}
    if isinstance(j, list) and j:
        bid = j[0]['bundle_id']
        b = get(f"https://bundles.jito.wtf/api/v1/bundles/bundle/{bid}")
        if isinstance(b, list) and b:
            b = b[0]
            rec.update({"bundle_id": bid, "txs": b.get('txSignatures'), "tip": b.get('landedTipLamports'),
                        "tippers": b.get('tippers'), "blockIndex": b.get('blockIndex'), "validator": b.get('validator')})
    with lock:
        with open(OUT, 'a') as f: f.write(json.dumps(rec) + '\n')
todo = [s for s in smp if s['sig'] not in done]
print(len(todo), flush=True)
with ThreadPoolExecutor(6) as ex: list(ex.map(one, todo))
print('done', flush=True)
