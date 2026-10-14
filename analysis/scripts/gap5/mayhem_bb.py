import sys,json,base64,struct; sys.path.insert(0,'.')
from rpc import rpc; from sol import *; from v1build import *
U='EW4dqBdFKBjjfTkdX1XeR9BnfrL1sUivVNaTJVJ18Tk2'; TIP='96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5'
sw=json.load(open('sweep_nonce.json'))
mm=[x['mint'] for x in sw if x['err'] is None and x['curve']['mayhem'] and x['curve']['quote'].startswith('1111')][:2]
nm=[x['mint'] for x in sw if x['err'] is None and not x['curve']['mayhem'] and x['curve']['quote'].startswith('1111')][:1]
cands={'buyback#1':'9M4giFFMxmFGXtc3feFzRai56WbBqehoSeRE5GK7gf7','reserved(GesfTA)':'GesfTA3X2arioaHp8bbKdjG9vJtskViWACZoYvxp4twS','reserved#2':'4budycTjhs9fD6xw62VBducVTNgMgJJ5BgtKq7mAZwn6','normal(62qc)':'62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV','buyback WSOL ATA':ata('9M4giFFMxmFGXtc3feFzRai56WbBqehoSeRE5GK7gf7',TOK,WSOL)}
for m in mm+nm:
    bc=pda([b'bonding-curve',b58d(m)],PUMP)[0]; d=base64.b64decode(rpc('getAccountInfo',[bc,{'encoding':'base64'}])['result']['value']['data'][0]); vt,vq=struct.unpack_from('<QQ',d,8)
    amt=int((vt-vt*vq//(vq+9_800_000))*0.95)
    for k,bb in cands.items():
        bh=rpc('getLatestBlockhash',[{'commitment':'processed'}])['result']['value']['blockhash']
        tx=v1_tx(U,[create_ata_idem(U,U,m,T22),buy_v3(U,m,bb,amt,20_000_000),transfer(U,TIP,1_000_000)],bh,prio=10_000,cul=1_400_000,lads=64<<20)[0]
        v=rpc('simulateTransaction',[base64.b64encode(tx).decode(),{"encoding":"base64","sigVerify":False,"replaceRecentBlockhash":True}])['result']['value']
        print(m[:6],'mayhem' if d[81] else 'normal',k,v['err'],[l[:110] for l in (v['logs'] or []) if 'Error' in l][:1])
