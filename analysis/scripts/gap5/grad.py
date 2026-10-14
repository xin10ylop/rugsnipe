# For graduated coins: slot of the completing trade (CompleteEvent) vs slot of migrate (PumpSwap pool creation)
import sys,json,base64,struct,time; sys.path.insert(0,'.')
from rpc import rpc; from sol import *
from concurrent.futures import ThreadPoolExecutor
COMPLETE=bytes.fromhex('5f72619cd42e9808'); TRADE=bytes.fromhex('bddb7fd34ee661ee'); POST=bytes.fromhex('6fb06d8b316cd5fb')
mints=[c['mint'] for c in json.load(open('/home/user/rugsnipe/data/cache/graduated_window.json'))]
if len(sys.argv)>1: mints+= [m for m in json.load(open(sys.argv[1])) if m not in mints]
def tx(s,ep):
    for t in range(5):
        r=rpc('getTransaction',[s,{'encoding':'json','maxSupportedTransactionVersion':1,'commitment':'confirmed'}],ep=ep)
        if r.get('result'): return r['result']
        time.sleep(2)
def ixname(d):
    # TradeEvent: skip fixed fields to ix_name string
    o=8+32+8+8+1+32+8+8*4+32+8+8+32+8+8+1+8+8+8+8
    n=struct.unpack_from('<I',d,o)[0]; return d[o+4:o+4+n].decode(errors='replace'), d[o+4+n]
def one(i_m):
    i,m=i_m; bc=pda([b'bonding-curve',b58d(m)],PUMP)[0]
    sigs=rpc('getSignaturesForAddress',[bc,{'limit':25}],ep=i).get('result') or []
    comp=mig=None; trade=None; nfail_between=0
    for s in sigs:
        if s['err'] is not None: continue
        t=tx(s['signature'],i)
        if not t: continue
        logs=t['meta']['logMessages'] or []
        if any('Instruction: Migrate' in l for l in logs) and mig is None:
            mig=dict(slot=t['slot'],sig=s['signature'],ix=[l for l in logs if 'Instruction: Migrate' in l][0][-20:],bt=t.get('blockTime'))
        datas=[base64.b64decode(l.split('Program data: ')[1]) for l in logs if l.startswith('Program data: ')]
        if any(d[:8]==COMPLETE for d in datas):
            tr=[d for d in datas if d[:8]==TRADE]
            nm,may=ixname(tr[-1]) if tr else (None,None)
            comp=dict(slot=t['slot'],sig=s['signature'],trade_ix=nm,mayhem=may,post=any(d[:8]==POST for d in datas),bt=t.get('blockTime'),
                      launcher=t['transaction']['message']['accountKeys'][t['transaction']['message']['instructions'][-1]['programIdIndex']] if 'accountKeys' in t['transaction']['message'] else None)
            break
    fails=sum(1 for s in sigs if s['err'] is not None and comp and mig and comp['slot']<=s['slot']<=mig['slot'])
    return dict(mint=m,comp=comp,mig=mig,fails_between=fails)
res=[]
def safe(a):
    try: return one(a)
    except Exception as e: return dict(mint=a[1],comp=None,mig=None,fails_between=None,exc=str(e))
with ThreadPoolExecutor(2) as ex:
    for r in ex.map(safe,enumerate(mints)):
        res.append(r); json.dump(res,open('grad.json','w'))
json.dump(res,open('grad.json','w'))
for r in res:
    c,m=r['comp'],r['mig']
    print(r['mint'][:8], c and c['trade_ix'], c and c['mayhem'], c and c['post'], (m['slot']-c['slot']) if c and m else None, m and m['ix'], r['fails_between'])
