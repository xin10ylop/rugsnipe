# Worst-case CU sweep: v1 [AdvanceNonce, ATA-idempotent(T22), buy_v3, tip] on many live curves (simulateTransaction, sigVerify off)
import sys,json,base64,struct,time; sys.path.insert(0,'.')
from rpc import rpc; from sol import *; from v1build import *
from concurrent.futures import ThreadPoolExecutor
USER='EW4dqBdFKBjjfTkdX1XeR9BnfrL1sUivVNaTJVJ18Tk2'; NONCE='EzeKnQSYWC9euvwR6m3ZF5bvS47KaoGpViGsWHcn8r5M'
TIP='96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5'
BUYBACKS=['5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD','9M4giFFMxmFGXtc3feFzRai56WbBqehoSeRE5GK7gf7','GXPFM2caqTtQYC2cJ5yJRi9VDkpsYZXzYdwYpGnLmtDL','3BpXnfJaUTiwXnJNe7Ej1rcbzqTTQUvLShZaWazebsVR','5cjcW9wExnJJiqgLjq7DEG75Pm6JBgE1hNv4B2vHXUW6','EHAAiTxcdDwQ3U4bU6YcMsQGaekdzLS3B5SmYo46kJtL','5eHhjP8JaYkz83CWwvGU2uMUXefd3AazWGx4gpcuEEYD','A7hAgCzFw14fejgCp387JUJRMNyz4j89JKnhtKU8piqW']
MiB=1<<20
mints=[m['mint'] for m in json.load(open(sys.argv[1]))]
def curve_of(m): return pda([b'bonding-curve',b58d(m)],PUMP)
def decode_curve(d):
    vt,vq,rt,rq,sup=struct.unpack_from('<5Q',d,8); comp=d[48]; creator=b58e(d[49:81]); may=d[81]; cb=d[82]
    q=b58e(d[83:115]) if len(d)>=115 else '1'*32
    hr=d[124] if len(d)>124 else None
    return dict(vt=vt,vq=vq,rt=rt,rq=rq,complete=comp,mayhem=may,cashback=cb,quote=q,holder_reward=hr,len=len(d))
info={}
for i in range(0,len(mints),50):
    ch=mints[i:i+50]; bcs=[curve_of(m)[0] for m in ch]
    r=rpc('getMultipleAccounts',[bcs+ch,{'encoding':'base64'}])['result']['value']
    for j,m in enumerate(ch):
        a=r[j]; mi=r[50+j] if len(ch)==50 else r[len(ch)+j]
        if not a or not mi: continue
        c=decode_curve(base64.b64decode(a['data'][0])); c['tp']=mi['owner']; c['mint_len']=len(base64.b64decode(mi['data'][0])); info[m]=c
print('decoded',len(info))
nonce_val=rpc('getAccountInfo',[NONCE,{'encoding':'jsonParsed'}])['result']['value']['data']['parsed']['info']['blockhash']
def one(args):
    k,m=args; c=info[m]; tp=c['tp']
    bb=BUYBACKS[k%8]
    amt=int((c['vt']-c['vt']*c['vq']//(c['vq']+9_800_000))*0.97)
    ixs=[advance_nonce(NONCE,USER),create_ata_idem(USER,USER,m,tp),buy_v3(USER,m,bb,max(amt,1),20_000_000,tp=tp),transfer(USER,TIP,1_000_000)]
    tx=v1_tx(USER,ixs,nonce_val,prio=10_000,cul=1_400_000,lads=64*MiB)[0]
    for t in range(4):
        r=rpc('simulateTransaction',[base64.b64encode(tx).decode(),{"encoding":"base64","sigVerify":False,"replaceRecentBlockhash":False,"commitment":"processed"}],ep=k)
        if 'result' in r: break
        time.sleep(2)
    v=r.get('result',{}).get('value',{})
    bumps=dict(bc=curve_of(m)[1],abc=pda([b58d(curve_of(m)[0]),b58d(tp),b58d(m)],ATA_PROG)[1],
               uata=pda([b58d(USER),b58d(tp),b58d(m)],ATA_PROG)[1])
    logs=v.get('logs') or []
    per=[l for l in logs if ' consumed ' in l]
    return dict(mint=m,buyback=bb,err=v.get('err'),units=v.get('unitsConsumed'),lads=v.get('loadedAccountsDataSize'),size=len(tx),bumps=bumps,
                curve={k2:c[k2] for k2 in ('complete','mayhem','cashback','quote','holder_reward','len','tp','mint_len','rq')},per_ix=per,
                errlog=[l for l in logs if 'Error' in l or 'failed' in l][:3],rpc_err=r.get('error'))
todo=[(k,m) for k,m in enumerate(info) if not info[m]['complete']]
with ThreadPoolExecutor(4) as ex: res=list(ex.map(one,todo))
json.dump(res,open(sys.argv[2],'w'))
print('done',len(res))
