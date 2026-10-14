import json, os, sys, time, threading, requests, random
from concurrent.futures import ThreadPoolExecutor
sys.path.insert(0, '/home/user/rugsnipe/analysis')
from rugsnipe_analysis import funding as F, labels as L
import core
smp = json.load(open('sample.json'))['sample']
OUT = 'labels.jsonl'
done = set()
if os.path.exists(OUT):
    for l in open(OUT): done.add(json.loads(l)['sig'])
lock = threading.Lock()
_loc = threading.local()
def api_prior(creator, t_ms):
    s = getattr(_loc, 's', None)
    if s is None:
        s = requests.Session(); s.headers.update({"origin": "https://pump.fun", "accept": "application/json"}); _loc.s = s
    url = f"https://frontend-api-v3.pump.fun/coins?creator={creator}&limit=50&offset=0&sort=created_timestamp&order=ASC&includeNsfw=true"
    d = 1
    for a in range(8):
        try:
            r = s.get(url, timeout=30)
            if r.status_code == 429 or r.status_code >= 500: raise RuntimeError(r.status_code)
            js = r.json()
            if isinstance(js, list):
                ts = [c.get('created_timestamp') or 0 for c in js]
                return {"n_page": len(js), "prior": sum(1 for t in ts if t < t_ms - 1000), "first_ts": min(ts) if ts else None}
            return {"error": str(js)[:100]}
        except Exception as e:
            time.sleep(d + random.random()); d = min(2 * d, 30)
    return {"error": "failed"}
def one(s):
    a = core.analyze(s)
    if not a or a.get('status') != 'ok':
        rec = {"sig": s['sig'], "status": (a or {}).get('status', 'noblock')}
    else:
        rec = {"sig": s['sig'], "status": "ok", "creator": a['creator'], "signer": a['signer'], "bt": a['bt']}
        rec['api'] = api_prior(a['creator'], a['bt'] * 1000)
        if a['signer'] != a['creator']:
            rec['api_signer'] = api_prior(a['signer'], a['bt'] * 1000)
        try:
            f = F.first_funding(a['signer'], max_pages=1, scan_max=3, until_time=a['bt'])
            lab = L.get_label(f.funder) if f.funder else None
            mf = (f.main or {}).get('funder')
            mlab = L.get_label(mf) if mf else None
            rec['fund'] = {"status": f.status, "funder": f.funder, "amt": f.amount_sol, "first_sig_time": f.first_sig_time,
                           "n_sigs": f.wallet_sig_count, "complete": f.wallet_history_complete,
                           "label": (lab.entity, lab.category) if lab else None,
                           "main_funder": mf, "main_amt": (f.main or {}).get('amount_sol'),
                           "main_label": (mlab.entity, mlab.category) if mlab else None}
        except Exception as e:
            rec['fund'] = {"status": "error", "e": str(e)[:200]}
    with lock:
        with open(OUT, 'a') as fh: fh.write(json.dumps(rec) + '\n')
todo = [s for s in smp if s['sig'] not in done and core.load_block(s['slot'])]
print(len(todo), flush=True)
with ThreadPoolExecutor(8) as ex: list(ex.map(one, todo))
print('done', flush=True)
