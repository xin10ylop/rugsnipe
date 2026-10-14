import sys,json,base64,struct; sys.path.insert(0,'.')
from rpc import rpc; from sol import *; from v1build import *
USER='EW4dqBdFKBjjfTkdX1XeR9BnfrL1sUivVNaTJVJ18Tk2'; NONCE='EzeKnQSYWC9euvwR6m3ZF5bvS47KaoGpViGsWHcn8r5M'
BUYBACK='9M4giFFMxmFGXtc3feFzRai56WbBqehoSeRE5GK7gf7'; TIP='96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5'
MINT=sys.argv[1] if len(sys.argv)>1 else 'AyXXuvqg4Qwrpuab3dN4mc34fbD8cY9773KSkiYNpump'
bc=pda([b'bonding-curve',b58d(MINT)],PUMP)[0]
d=base64.b64decode(rpc('getAccountInfo',[bc,{'encoding':'base64'}])['result']['value']['data'][0])
vt,vs=struct.unpack_from('<QQ',d,8)
spend=10_000_000; amount=int((vt - vt*vs//(vs+int(spend*0.99)))*0.9); maxc=20_000_000
nonce_val=rpc('getAccountInfo',[NONCE,{'encoding':'jsonParsed'}])['result']['value']['data']['parsed']['info']['blockhash']
bh=rpc('getLatestBlockhash',[{'commitment':'processed'}])['result']['value']['blockhash']
core=[create_ata_idem(USER,USER,MINT,T22),buy_v3(USER,MINT,BUYBACK,amount,maxc),transfer(USER,TIP,1_000_000)]
def sim(tx,replace=True,label=''):
    r=rpc('simulateTransaction',[base64.b64encode(tx).decode(),{"encoding":"base64","sigVerify":False,"replaceRecentBlockhash":replace,"commitment":"processed"}])
    if 'error' in r: out=dict(label=label,rpc_error=r['error']); print(json.dumps(out)); return out
    v=r['result']['value']
    out=dict(label=label,size=len(tx),err=v.get('err'),units=v.get('unitsConsumed'),lads=v.get('loadedAccountsDataSize'),fee=v.get('fee'),
             logs_tail=(v.get('logs') or [])[-3:])
    print(json.dumps(out)); return out
res=[]
MiB=1<<20
res.append(sim(v1_tx(USER,core,bh,prio=100_000,cul=1_400_000,lads=64*MiB)[0],label='v1 core, loose limits'))
res.append(sim(v1_tx(USER,[advance_nonce(NONCE,USER)]+core,nonce_val,prio=100_000,cul=1_400_000,lads=64*MiB)[0],replace=False,label='v1 nonce, loose limits'))
res.append(sim(v1_tx(USER,core,bh)[0],label='v1 core, mask=0 (nothing set)'))
res.append(sim(v1_tx(USER,core,bh,prio=100_000,cul=1_400_000)[0],label='v1 core, CU set, LADS unset'))
res.append(sim(v1_tx(USER,core,bh,prio=100_000,lads=64*MiB)[0],label='v1 core, LADS set, CU unset'))
res.append(sim(v1_tx(USER,core,bh,prio=100_000,cul=1_400_000,lads=64*MiB)[0],label='v1 core repeat'))
res.append(sim(legacy_tx(USER,[cb_limit(1_400_000),cb_price(1000),cb_lads(64*MiB)]+core,bh)[0],label='legacy core + 3 CB ixs'))
res.append(sim(v1_tx(USER,[cb_limit(1_400_000),cb_price(1000)]+core,bh,prio=100_000,cul=1_400_000,lads=64*MiB)[0],label='v1 core + 2 CB ixs (ignored)'))
json.dump(dict(mint=MINT,amount=amount,res=res),open(f'sim_{MINT[:6]}.json','w'))
