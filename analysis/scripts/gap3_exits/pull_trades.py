import json, sys, threading, time
from concurrent.futures import ThreadPoolExecutor
sys.path.insert(0, '/home/user/rugsnipe/analysis')
from rugsnipe_analysis.pumpapi import PumpApi, normalize_trade
api = PumpApi(max_rps=float(sys.argv[2]) if len(sys.argv)>2 else 4.0)
sel = json.load(open('sel.json'))
lim = int(sys.argv[1]) if len(sys.argv)>1 else len(sel)
out = 'trades.jsonl'
done=set()
try:
    for l in open(out): done.add(json.loads(l)['mint'])
except FileNotFoundError: pass
lock=threading.Lock(); fo=open(out,'a')
def go(s):
    mint, bt, slot = s['mint'], s['bt'], s['slot']
    seen={}; complete=False; trunc=[]
    for W, mp in ((900, 20), (300, 12), (60, 6)):
        raw, comp = api.trades_before(mint, (bt+W)*1000+999, max_pages=mp)
        for t in raw:
            n = normalize_trade(t); seen[(n['sig'], n['sub'], n['user'])] = n
        if comp:
            complete = True
            if W == 900: break
        else:
            trunc.append(W)
        if complete: break
    tr = sorted(seen.values(), key=lambda x:(x['slot'],x['txi'],x['sub']))
    tr = [t for t in tr if t['slot']>=slot]
    # contiguous coverage: from creation to the earliest window end that was complete
    res = {'mint':mint, 'W0':900, 'complete':complete, 'trunc':trunc, 'n':len(tr),
           'trades':[[t['slot'],t['txi'],t['sub'],t['ts'],t['user'],int(t['buy']),round(t['sol'],9),round(t['tok'],6),t['mcap'],t['sig'],t['prog']] for t in tr]}
    with lock:
        fo.write(json.dumps(res,separators=(',',':'))+'\n'); fo.flush(); done.add(mint)
        if len(done)%50==0: print(time.strftime('%H:%M:%S'), len(done), api.stats, flush=True)
todo=[s for s in sel[:lim] if s['mint'] not in done]
with ThreadPoolExecutor(8) as ex:
    for f in [ex.submit(go,s) for s in todo]:
        try: f.result()
        except Exception as e: print('fail', e, flush=True)
print('done', len(done), api.stats)
