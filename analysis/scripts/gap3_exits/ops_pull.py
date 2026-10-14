import json, sys, random, time
sys.path.insert(0, '/home/user/rugsnipe/analysis')
from rugsnipe_analysis.pumpapi import PumpApi, FRONTEND, normalize_trade
api = PumpApi(max_rps=1.0); sw = PumpApi(max_rps=6.0)
OPS = sys.argv[1].split(',')
K = int(sys.argv[2]) if len(sys.argv) > 2 else 15
T0 = 1791963000 - 16 * 86400
sel = []; fo = open('ops_trades.jsonl', 'a')
done = set()
try:
    for l in open('ops_trades.jsonl'): done.add(json.loads(l)['mint'])
except Exception: pass
for w in OPS:
    coins = []
    for p in range(6):
        page = api.get_json(f"{FRONTEND}/coins", {"offset": p * 50, "limit": 50, "sort": "created_timestamp", "order": "DESC", "includeNsfw": "true", "creator": w}, cache_key=f"creatorcoins50:{w}:{p}") or []
        coins += page
        if len(page) < 50 or page[-1]['created_timestamp'] / 1000 < T0: break
    coins = [c for c in coins if c['created_timestamp'] / 1000 >= T0 and c['created_timestamp'] / 1000 < 1791963000 - 3600]
    random.seed(w); random.shuffle(coins)
    print(w[:8], 'coins in window', len(coins), flush=True)
    for c in coins[:K]:
        mint = c['mint']; ct = c['created_timestamp'] // 1000
        seen = {}; complete = False; trunc = []
        for W, mp in ((900, 20), (300, 12), (60, 6)):
            raw, comp = sw.trades_before(mint, (ct + W) * 1000 + 999, max_pages=mp)
            for t in raw:
                n = normalize_trade(t); seen[(n['sig'], n['sub'], n['user'])] = n
            if comp: complete = True; break
            trunc.append(W)
        tr = sorted(seen.values(), key=lambda x: (x['slot'], x['txi'], x['sub']))
        dev = [t for t in tr if t['user'] == w and t['buy']]
        if not tr or not dev or dev[0]['slot'] != tr[0]['slot']:
            print('  skip (no dev buy at first slot)', mint[:8]); continue
        S0 = tr[0]['slot']; csig = dev[0]['sig']
        sel.append({'mint': mint, 'sig': csig, 'slot': S0, 'bt': tr[0]['ts'], 'rank': -1, 'creator': w, 'user': w, 'fee_payer': w, 'signers': [w], 'dev_buys': [], 'w': 1, 'ath_usd': c.get('ath_market_cap'), 'complete': c.get('complete'), 'op': w})
        if mint not in done:
            fo.write(json.dumps({'mint': mint, 'W0': 900, 'complete': complete, 'trunc': trunc, 'n': len(tr), 'trades': [[t['slot'], t['txi'], t['sub'], t['ts'], t['user'], int(t['buy']), round(t['sol'], 9), round(t['tok'], 6), t['mcap'], t['sig'], t['prog']] for t in tr]}, separators=(',', ':')) + '\n'); fo.flush()
json.dump(sel, open('ops_sel_%s.json' % OPS[0][:6], 'w'))
print('done', len(sel), api.stats, sw.stats)
