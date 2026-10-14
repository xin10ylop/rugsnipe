import sys,json,base64,struct,time,re; sys.path.insert(0,'.')
from rpc import rpc; from sol import *; from v1build import *
BIG='5tzFkiKscXHK5ZXCGbXZxdw7gTjjD1mBwuoFbhUvuAi9'; BB='9M4giFFMxmFGXtc3feFzRai56WbBqehoSeRE5GK7gf7'; TIP='96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5'
MiB=1<<20
idl=json.load(open('/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/g5/docs/pump.json')); E={e['code']:e['name'] for e in idl['errors']}
sw=json.load(open('sweep_nonce.json'))
def pick(may):
    for x in reversed(sw):
        if x['err'] is None and x['curve']['mayhem']==may and x['curve']['quote'].startswith('1111') and not x['curve']['holder_reward']: return x['mint']
def curve(m):
    bc=pda([b'bonding-curve',b58d(m)],PUMP)[0]
    d=base64.b64decode(rpc('getAccountInfo',[bc,{'encoding':'base64'}])['result']['value']['data'][0])
    vt,vq,rt,rq,sup=struct.unpack_from('<5Q',d,8); return dict(vt=vt,vq=vq,rt=rt,rq=rq,sup=sup,complete=d[48])
def ix(user,m,a,b,pf,disc='07051dc4f5176550'):
    i=buy_v3(user,m,BB,a,b,disc=disc); i.data=i.data[:24]+({None:b'',True:b'\x01',False:b'\x00'}[pf]); return i
def run(label,user,m,a,b,pf,disc='07051dc4f5176550',ata=True):
    bh=rpc('getLatestBlockhash',[{'commitment':'processed'}])['result']['value']['blockhash']
    ixs=([create_ata_idem(user,user,m,T22)] if ata else [])+[ix(user,m,a,b,pf,disc),transfer(user,TIP,1_000_000)]
    tx=v1_tx(user,ixs,bh,prio=10_000,cul=1_400_000,lads=64*MiB)[0]
    v=rpc('simulateTransaction',[base64.b64encode(tx).decode(),{"encoding":"base64","sigVerify":False,"replaceRecentBlockhash":True,"commitment":"processed"}])['result']['value']
    err=v.get('err'); code=None
    if err and isinstance(err,dict) and 'InstructionError' in err:
        c=err['InstructionError'][1]; code=c.get('Custom') if isinstance(c,dict) else c
    logs=v.get('logs') or []
    pumpcu=[int(re.search(r'consumed (\d+)',l).group(1)) for l in logs if l.startswith('Program 6EF8rr') and 'consumed' in l]
    out=dict(label=label,mint=m[:6],amount=a,max_or_min=b,pf=pf,err=err,errname=E.get(code) if isinstance(code,int) else None,units=v.get('unitsConsumed'),pump_cu=pumpcu[-1:] ,
             ndata=sum(1 for l in logs if l.startswith('Program data:')),msgs=[l[:120] for l in logs if 'Program log:' in l and 'Instruction' not in l][:4])
    print(json.dumps(out)); return out
res=[]
for may in (0,1):
    m=pick(may); c=curve(m); print('MINT',m,'mayhem',may,c)
    # cost of ~0.01 SOL
    amt=c['vt']-c['vt']*c['vq']//(c['vq']+9_800_000)
    cost=c['vq']*amt//(c['vt']-amt)+1
    for pf in (True,False,None):
        res.append(run(f'slippage may={may}',BIG,m,amt,cost//2,pf))
    big=c['rt']+10_000_000_000_000  # 10M tokens past the curve
    for pf in (True,False,None):
        res.append(run(f'over-curve may={may}',BIG,m,big,500*10**9,pf))
    res.append(run(f'exactquote over min may={may}',BIG,m,10_000_000,amt*3,True,disc='e1f7501ed5b38488'))
    res.append(run(f'exactquote 200SOL may={may} pf=1',BIG,m,200*10**9,1,True,disc='e1f7501ed5b38488'))
    res.append(run(f'exactquote 200SOL may={may} pf=0',BIG,m,200*10**9,1,False,disc='e1f7501ed5b38488'))
    res.append(run(f'small buy fresh wallet (UVA init) may={may}',BIG,m,amt,cost*2,False))
json.dump(res,open('partial.json','w'))
