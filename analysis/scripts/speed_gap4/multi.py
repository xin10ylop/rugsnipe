import sys,json,base64,struct,time; sys.path.insert(0,'.')
from rpc import rpc; from sol import *; from v1build import *; from sim_fn import sim
USER='EW4dqBdFKBjjfTkdX1XeR9BnfrL1sUivVNaTJVJ18Tk2'; BUYBACK='9M4giFFMxmFGXtc3feFzRai56WbBqehoSeRE5GK7gf7'; TIP='96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5'
MiB=1<<20; out=[]
for MINT in json.load(open('mints.json')):
    try:
        bc=pda([b'bonding-curve',b58d(MINT)],PUMP)[0]
        acc=rpc('getAccountInfo',[bc,{'encoding':'base64'}])['result']['value']
        if not acc: print('nocurve',MINT); continue
        d=base64.b64decode(acc['data'][0]); vt,vs=struct.unpack_from('<QQ',d,8)
        mi=rpc('getAccountInfo',[MINT,{'encoding':'base64','dataSlice':{'offset':0,'length':0}}])['result']['value']
        tp=mi['owner']
        amount=int((vt - vt*vs//(vs+9_900_000))*0.9)
        bh=rpc('getLatestBlockhash',[{'commitment':'processed'}])['result']['value']['blockhash']
        core=[create_ata_idem(USER,USER,MINT,tp),buy_v3(USER,MINT,BUYBACK,amount,20_000_000,tp=tp),transfer(USER,TIP,1_000_000)]
        _,ab=pda([b58d(USER),b58d(tp),b58d(MINT)],ATA_PROG)
        r=sim(v1_tx(USER,core,bh,prio=10_000,cul=1_400_000,lads=64*MiB)[0],label=MINT)
        r.update(tp=tp[:6],mint_space=mi['space'],ata_bump=ab,cashback_byte=None); out.append(r)
    except Exception as e: print('ex',MINT,e)
    time.sleep(0.5)
json.dump(out,open('multi.json','w'))
