"""Pull swap-api trade tapes for a random sample of mayhem-mode launches (gap-1 creates).

Per launch:
  * the full tape from the create to +900 s (windows 900/300/60 s, union; coverage = the
    earliest window end whose walk reached the create),
  * one page (<=100 trades) ending at create + 1 h, 6 h, 24 h, 25 h, at the API ATH
    timestamp (+1 s) and ATH + 600 s, and the latest page.
Output: data/cache/mayhem/tapes.jsonl (one line per mint).  Raw pages are cached under
data/cache/mayhem/pumpapi/ so re-runs are free.

usage: mayhem_pull.py N_SAMPLE [MAX_RPS]   (seeded; resumable)
"""
import json, random, sys, threading, time
from concurrent.futures import ThreadPoolExecutor
sys.path.insert(0, '/home/user/rugsnipe/analysis')
from rugsnipe_analysis.pumpapi import PumpApi, normalize_trade

ROOT = '/home/user/rugsnipe/data/cache'
N = int(sys.argv[1]) if len(sys.argv) > 1 else 350
RPS = float(sys.argv[2]) if len(sys.argv) > 2 else 1.25
api = PumpApi(max_rps=RPS, cache_root=f'{ROOT}/mayhem')
OUT = f'{ROOT}/mayhem/tapes.jsonl'

creates = [json.loads(l) for l in open(f'{ROOT}/gap1/creates.jsonl')]
outc = {}
for l in open(f'{ROOT}/gap1/outcomes.jsonl'):
    o = json.loads(l); outc[o['mint']] = o
pop = [c for c in creates if c.get('kind') == 'create' and c.get('mayhem') and c.get('sol_quoted')]
pop.sort(key=lambda c: c['sig'])
rng = random.Random(20261014)
rng.shuffle(pop)
sample = pop[:N]
json.dump([{'mint': c['mint'], 'bt': c['bt'], 'slot': c['slot']} for c in sample],
          open(f'{ROOT}/mayhem/sample.json', 'w'))

def row(t):
    n = normalize_trade(t)
    return [n['slot'], n['txi'], n['sub'], n['ts'], n['user'], int(n['buy']), round(n['sol'], 9),
            round(n['tok'], 6), float(t.get('priceSol') or 0), n['sig'], n['prog']]

done = set()
try:
    for l in open(OUT): done.add(json.loads(l)['mint'])
except FileNotFoundError:
    pass
lock = threading.Lock(); fo = open(OUT, 'a')

def page_before(mint, ts):
    raw, comp = api.trades_before(mint, ts * 1000 + 999, max_pages=1)
    return {'ts': ts, 'complete': comp, 'trades': [row(t) for t in raw]}

FULL_N = 320   # first FULL_N of the shuffled sample get all checkpoints; the rest 1 h + latest
POS = {c['mint']: k for k, c in enumerate(sample)}

def go(c):
    mint, bt, slot = c['mint'], c['bt'], c['slot']
    full = POS[mint] < FULL_N
    seen = {}; cov = None; trunc = []
    for W, mp in ((900, 15), (300, 8), (60, 4)):
        raw, comp = api.trades_before(mint, (bt + W) * 1000 + 999, max_pages=mp)
        for t in raw:
            r = row(t); seen[(r[9], r[2], r[4])] = r
        if comp:
            cov = W if cov is None else cov
            break
        trunc.append(W)
    tr = sorted(seen.values(), key=lambda x: (x[0], x[1], x[2]))
    tr = [t for t in tr if t[0] >= slot]
    if cov is not None:
        tr = [t for t in tr if t[3] <= bt + cov]
    o = outc.get(mint, {})
    cps = {}
    for lab, ts in (('1h', bt + 3600), ('6h', bt + 6 * 3600), ('24h', bt + 24 * 3600), ('25h', bt + 25 * 3600)):
        if (full or lab == '1h') and ts < time.time() - 60:
            cps[lab] = page_before(mint, ts)
    if full and o.get('ath_ts'):
        cps['ath'] = page_before(mint, int(o['ath_ts']) + 1)
        cps['ath+600'] = page_before(mint, int(o['ath_ts']) + 600)
    raw, comp = api.trades_latest(mint, 1)
    cps['latest'] = {'ts': int(time.time()), 'complete': comp, 'trades': [row(t) for t in raw]}
    res = {'mint': mint, 'bt': bt, 'slot': slot, 'cov': cov, 'trunc': trunc, 'n': len(tr),
           'trades': tr, 'cp': cps, 'ath_usd': o.get('ath_usd'), 'ath_ts': o.get('ath_ts'),
           'sol_usd': o.get('sol_usd'), 'complete': o.get('complete'),
           'dev_buys': c.get('dev_buys'), 'full': full}
    with lock:
        fo.write(json.dumps(res, separators=(',', ':')) + '\n'); fo.flush(); done.add(mint)
        if len(done) % 25 == 0:
            print(time.strftime('%H:%M:%S'), len(done), api.stats, flush=True)

todo = [c for c in sample if c['mint'] not in done]
print('todo', len(todo), 'of', len(sample), flush=True)
with ThreadPoolExecutor(4) as ex:
    for f in [ex.submit(go, c) for c in todo]:
        try:
            f.result()
        except Exception as e:
            print('fail', repr(e)[:200], flush=True)
print('done', len(done), api.stats, flush=True)
