import sys,json,base64,struct; sys.path.insert(0,'.')
from rpc import rpc; from sol import *; from v1build import *; from sim_fn import sim
pre=open('sim.py').read(); pre=pre[:pre.index('def sim')]
exec(pre)
keys,hdr=compile_keys(USER,[advance_nonce(NONCE,USER)]+core)
# per account sizes, plus programdata for upgradeable programs
infos=rpc('getMultipleAccounts',[keys,{'encoding':'base64','dataSlice':{'offset':0,'length':0}}])['result']['value']
tot=0; rows=[]
for k,i in zip(keys,infos):
    if i is None: rows.append((k,'MISSING',0)); continue
    sz=i['space']; extra=0; note=i['owner'][:8]
    if i['owner']=='BPFLoaderUpgradeab1e11111111111111111111111':
        full=rpc('getAccountInfo',[k,{'encoding':'base64'}])['result']['value']; pd=b58e(base64.b64decode(full['data'][0])[4:36])
        pdi=rpc('getAccountInfo',[pd,{'encoding':'base64','dataSlice':{'offset':0,'length':0}}])['result']['value']; extra=pdi['space']+64; note+=' programdata '+str(pdi['space'])
    rows.append((k,note,sz)); tot+=sz+64+extra
for r in rows: print(r)
print('sum(space+64)+programdata+64 =',tot)
MiB=1<<20
r=sim(v1_tx(USER,core,bh,prio=10_000,cul=1_400_000,lads=64*MiB)[0],label='probe'); U=r['units']; L=r['lads']
for cul,lads,lab in [(U,L,'exact CU, exact LADS'),(U-1,L,'CU-1'),(U,L-1,'LADS-1'),(U+2000,L+8192,'margin')]:
    sim(v1_tx(USER,core,bh,prio=10_000,cul=cul,lads=lads)[0],label=lab+f' cul={cul} lads={lads}')
# buy_exact_quote_in_v3 variant
core2=[create_ata_idem(USER,USER,MINT,T22),buy_v3(USER,MINT,BUYBACK,10_000_000,1,disc='e1f7501ed5b38488'),transfer(USER,TIP,1_000_000)]
sim(v1_tx(USER,core2,bh,prio=10_000,cul=1_400_000,lads=64*MiB)[0],label='v1 buy_exact_quote_in_v3 core')
sim(v1_tx(USER,[advance_nonce(NONCE,USER)]+core2,nonce_val,prio=10_000,cul=1_400_000,lads=64*MiB)[0],replace=False,label='v1 nonce buy_exact_quote_in_v3')
# no ATA create (ATA would already exist) -> expect failure since ATA missing; just to show buy alone
sim(v1_tx(USER,[buy_v3(USER,MINT,BUYBACK,amount,maxc),transfer(USER,TIP,1_000_000)],bh,prio=10_000,cul=1_400_000,lads=64*MiB)[0],label='v1 without ATA create (ATA missing)')
# nonce with stale lifetime
sim(v1_tx(USER,[advance_nonce(NONCE,USER)]+core,bh,prio=10_000,cul=1_400_000,lads=64*MiB)[0],replace=False,label='v1 nonce ix but lifetime=recent blockhash')
