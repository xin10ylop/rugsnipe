# Simulate PumpSwap sell_v2 (17 accounts) on freshly migrated pump pools; read fee bps from SellEvent
import sys,json,base64,struct; sys.path.insert(0,'.')
from rpc import rpc; from sol import *; from v1build import *
AMM='pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA'; GC='ADyA8hdefvWN2dbGGWFotbzWxrAvLW83WG6QCVXvJKqw'; FC='5PHirr8joyTMp9JMm6nW7hNDVyEYdkzDqazxPD7RaTjx'
BB='9M4giFFMxmFGXtc3feFzRai56WbBqehoSeRE5GK7gf7'; SELLEV=bytes.fromhex('3e2f370aa503dc2a')
EVA=pda([b'__event_authority'],AMM)[0]
def pool_of(m):
    pa=pda([b'pool-authority',b58d(m)],PUMP)[0]
    return pda([b'pool',(0).to_bytes(2,'little'),b58d(pa),b58d(m),b58d(WSOL)],AMM)[0]
def dpool(d):
    o=8+1+2; creator=b58e(d[o:o+32]); o+=32; base=b58e(d[o:o+32]); o+=32; quote=b58e(d[o:o+32]); o+=64
    pbt=b58e(d[o:o+32]); o+=32; pqt=b58e(d[o:o+32]); o+=32; o+=8; cc=b58e(d[o:o+32]); o+=32; may=d[o]; o+=2
    vqr=int.from_bytes(d[o:o+16],'little',signed=True)
    return dict(base=base,quote=quote,pbt=pbt,pqt=pqt,may=may,vqr=vqr)
def bal(a): return int(rpc('getTokenAccountBalance',[a])['result']['value']['amount'])
mints=json.load(open(sys.argv[1])) if len(sys.argv)>1 else [c['mint'] for c in json.load(open('/home/user/rugsnipe/data/cache/graduated_window.json'))]
out=[]
for m in mints[:40]:
  try:
        p=pool_of(m); acc=rpc('getAccountInfo',[p,{'encoding':'base64'}])['result']['value']
        if not acc: continue
        P=dpool(base64.b64decode(acc['data'][0])); qb=bal(P['pqt']); bb=bal(P['pbt'])
        sup=int(rpc('getTokenSupply',[m])['result']['value']['amount'])
        mcap=(qb+P['vqr'])*sup/bb/1e9
        owner=None
        for sg in (rpc('getSignaturesForAddress',[p,{'limit':8}]).get('result') or []):
            if sg['err']: continue
            t=rpc('getTransaction',[sg['signature'],{'encoding':'json','maxSupportedTransactionVersion':1}]).get('result')
            if not t: continue
            for b in t['meta']['postTokenBalances']:
                if b['mint']==m and b.get('owner') not in (None,) and int(b['uiTokenAmount']['amount'])>10**9 and b['owner']!=pda([b'pool-authority',b58d(m)],PUMP)[0] and b['owner']!=p:
                    owner=b['owner']; tp=b['programId']; break
            if owner: break
        if not owner: continue
        ha=ata(owner,tp,m)
        ai=rpc('getAccountInfo',[ha,{'encoding':'jsonParsed'}])['result']['value']
        if not ai: continue
        amt=int(ai['data']['parsed']['info']['tokenAmount']['amount'])//10
        if amt==0 or ata(owner,tp,m)!=ha: continue
        if rpc('getAccountInfo',[owner,{'encoding':'base64','dataSlice':{'offset':0,'length':0}}])['result']['value']['owner']!=SYS: continue
        uva=pda([b'user_volume_accumulator',b58d(owner)],AMM)[0]
        metas=[(p,0,1),(owner,1,1),(GC,0,0),(m,0,0),(WSOL,0,0),(ha,0,1),(ata(owner,TOK,WSOL),0,1),(P['pbt'],0,1),(P['pqt'],0,1),(tp,0,0),(TOK,0,0),(SYS,0,0),(uva,0,1),(FC,0,0),(ata(BB,TOK,WSOL),0,1),(EVA,0,0),(AMM,0,0)]
        sell=Ix(AMM,[(k,bool(s),bool(w)) for k,s,w in metas],bytes.fromhex('5df6823ce7e940b2')+struct.pack('<QQ',amt,1))
        bh=rpc('getLatestBlockhash',[{'commitment':'processed'}])['result']['value']['blockhash']
        ixs=[create_ata_idem(owner,owner,WSOL,TOK),sell]
        tx=v1_tx(owner,ixs,bh,prio=10_000,cul=1_400_000,lads=64<<20)[0]
        v=rpc('simulateTransaction',[base64.b64encode(tx).decode(),{"encoding":"base64","sigVerify":False,"replaceRecentBlockhash":True}])['result']['value']
        ev=[base64.b64decode(l.split('Program data: ')[1]) for l in (v.get('logs') or []) if l.startswith('Program data: ')]
        ev=[e for e in ev if e[:8]==SELLEV]
        r=dict(mint=m,mcap_sol=round(mcap,1),may=P['may'],err=v.get('err'),units=v.get('unitsConsumed'),
               ammcu=[l for l in (v.get('logs') or []) if l.startswith('Program pAMM') and 'consumed' in l][-1:],size=len(tx))
        if ev:
            e=ev[0]; f=lambda o: struct.unpack_from('<Q',e,8+o)[0]
            r.update(quote_out=f(56),lp_bps=f(64),proto_bps=f(80),creator_bps=f(336),cashback_bps=f(352),buyback_bps=f(368),buyback_fee=f(376),proto_fee=f(88),user_out=f(104))
        print(json.dumps(r)); out.append(r)
  except Exception as ex: print('ex',m,ex)
json.dump(out,open(sys.argv[2] if len(sys.argv)>2 else 'amm_sell.json','w'))
