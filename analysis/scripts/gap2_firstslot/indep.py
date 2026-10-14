import json, sys, threading, collections
from concurrent.futures import ThreadPoolExecutor
sys.path.insert(0, '/home/user/rugsnipe/analysis')
from rugsnipe_analysis import funding as F, labels as L
rows = json.load(open('rows.json'))
labs = {json.loads(l)['sig']: json.loads(l) for l in open('labels.jsonl')}
reuse = json.load(open('wallet_reuse.json'))
tasks = {}
for a in rows:
    lb = labs.get(a['sig'], {}); f = lb.get('fund') or {}
    cset = {a['creator'], a['signer'], f.get('funder'), f.get('main_funder')} | set(a['bundle_wallets'])
    cset.discard(None)
    for x in a['r0'] + a['r1']:
        if x['buy'] and not x['owner_linked'] and x['ok'] and reuse.get(x['signer'], 0) <= 2:
            tasks.setdefault(x['signer'], []).append((a['sig'], a['bt'], sorted(cset)))
print('wallets to check', len(tasks), flush=True)
out = {}
lock = threading.Lock()
def one(w):
    bt = min(t[1] for t in tasks[w])
    try:
        f = F.first_funding(w, max_pages=1, scan_max=3, until_time=bt)
        rec = {"status": f.status, "funder": f.funder, "main": (f.main or {}).get('funder'), "first": f.first_sig_time, "n": f.wallet_sig_count,
               "label": (L.get_label(f.funder).category if f.funder and L.get_label(f.funder) else None)}
    except Exception as e:
        rec = {"status": "error", "e": str(e)[:100]}
    with lock: out[w] = rec
with ThreadPoolExecutor(8) as ex: list(ex.map(one, list(tasks)))
json.dump({"tasks": tasks, "fund": out}, open('indep.json', 'w'))
# summarize
c = collections.Counter(); linked = set()
for w, ts in tasks.items():
    r = out.get(w, {})
    for sig, bt, cset in ts:
        fresh = bool(r.get('first') and r['first'] >= bt - 24 * 3600 and (r.get('n') or 9999) <= 50)
        lk = (r.get('funder') in cset) or (r.get('main') in cset)
        c[('linked' if lk else 'fresh_unlinked' if fresh else 'old_unlinked', r.get('status'))] += 1
        if lk: linked.add(w)
print(c)
json.dump(sorted(linked), open('linked_wallets.json', 'w'))
print('linked wallets', len(linked))
