import sys,json,base64,struct,re,time; sys.path.insert(0,'.')
from rpc import rpc; from sol import *; from v1build import *
idl=json.load(open('/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/g5/docs/pump.json'))
SELL3=[bytes(i['discriminator']).hex() for i in idl['instructions'] if i['name']=='sell_v3'][0]; print('sell_v3 disc',SELL3)
U='EW4dqBdFKBjjfTkdX1XeR9BnfrL1sUivVNaTJVJ18Tk2'; BB='9M4giFFMxmFGXtc3feFzRai56WbBqehoSeRE5GK7gf7'
sw=json.load(open('sweep_nonce.json'))
mints=[x['mint'] for x in sw if x['err'] is None and x['curve']['quote'].startswith('1111')][:60:2]
out=[]
for m in mints:
    try:
        bc=pda([b'bonding-curve',b58d(m)],PUMP)[0]; d=base64.b64decode(rpc('getAccountInfo',[bc,{'encoding':'base64'}])['result']['value']['data'][0]); vt,vq=struct.unpack_from('<QQ',d,8)
        amt=int((vt-vt*vq//(vq+9_800_000))*0.95)
        sell=buy_v3(U,m,BB,amt,1,disc=SELL3); sell.data=sell.data[:24]
        close=Ix(T22,[(ata(U,T22,m),False,True),(U,False,True),(U,True,False)],bytes([9]))
        bh=rpc('getLatestBlockhash',[{'commitment':'processed'}])['result']['value']['blockhash']
        tx=v1_tx(U,[create_ata_idem(U,U,m,T22),buy_v3(U,m,BB,amt,20_000_000),sell,close],bh,prio=10_000,cul=1_400_000,lads=64<<20)[0]
        v=rpc('simulateTransaction',[base64.b64encode(tx).decode(),{"encoding":"base64","sigVerify":False,"replaceRecentBlockhash":True}])['result']['value']
        pc=[int(re.search(r'consumed (\d+)',l).group(1)) for l in v['logs'] if re.match(r'Program (6EF8rr\w+|TokenzQ\w+) consumed',l)]
        # top-level order: ATA(..), pump buy, pump sell, token close -> take pump entries
        pumps=[int(re.search(r'consumed (\d+)',l).group(1)) for l in v['logs'] if l.startswith('Program 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P consumed')]
        cl=[int(re.search(r'consumed (\d+)',l).group(1)) for l in v['logs'] if l.startswith('Program TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb consumed')]
        bcb=pda([b'bonding-curve',b58d(m)],PUMP)[1]; ab=pda([b58d(bc),b58d(T22),b58d(m)],ATA_PROG)[1]; aq=pda([b58d(bc),b58d(TOK),b58d(WSOL)],ATA_PROG)[1]
        r=dict(mint=m,err=v['err'],units=v['unitsConsumed'],pump=pumps,t22_last=cl[-1:],S3=(255-bcb)+(255-ab)+(255-aq),may=d[81]); print(json.dumps(r)); out.append(r)
    except Exception as e: print('ex',m,e)
json.dump(out,open('sellsweep.json','w'))
