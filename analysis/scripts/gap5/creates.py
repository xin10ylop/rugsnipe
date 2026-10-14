# Pull ~400 recent create txs (signatures of the pump mint authority) and classify create ix layouts
import sys,json,base64,struct,time; sys.path.insert(0,'.')
from rpc import rpc
from concurrent.futures import ThreadPoolExecutor
MA='TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM'
sigs=[]; before=None
while len(sigs)<450:
    p={'limit':1000}; 
    if before: p['before']=before
    r=rpc('getSignaturesForAddress',[MA,p])['result']; 
    if not r: break
    before=r[-1]['signature']; sigs+= [s for s in r if s['err'] is None]
sigs=sigs[:450]
def get(i_s):
    i,s=i_s
    for t in range(5):
        r=rpc('getTransaction',[s['signature'],{'encoding':'json','maxSupportedTransactionVersion':1,'commitment':'confirmed'}],ep=i)
        if r.get('result'): return r['result']
        time.sleep(2)
with ThreadPoolExecutor(4) as ex: txs=list(ex.map(get,enumerate(sigs)))
json.dump([t for t in txs if t],open('creates_raw.json','w')); print('got',sum(1 for t in txs if t))
