import sys,base64,struct,json; sys.path.insert(0,'.')
from rpc import rpc
def dec(addr):
    d=base64.b64decode(rpc('getAccountInfo',[addr,{'encoding':'base64'}])['result']['value']['data'][0])
    o=8; bump=d[o]; o+=1; o+=32
    fees=lambda o: (struct.unpack_from('<QQQ',d,o),o+24)
    flat,o=fees(o)
    def tiers(o):
        n=struct.unpack_from('<I',d,o)[0]; o+=4; out=[]
        for i in range(n):
            lo,hi=struct.unpack_from('<QQ',d,o); th=lo|(hi<<64); f,o2=fees(o+16); o=o2; out.append((th/1e9,f))
        return out,o
    t,o=tiers(o); st,o=tiers(o); ex,o=fees(o)
    return dict(flat=flat,tiers=t,stable=st,exotic=ex,len=len(d))
for name,a in [('pump','8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt'),('pumpswap','5PHirr8joyTMp9JMm6nW7hNDVyEYdkzDqazxPD7RaTjx')]:
    r=dec(a); print(name,'len',r['len'],'flat',r['flat'],'exotic',r['exotic'])
    for th,f in r['tiers']: print('  tier mcap>=%.3f SOL lp/proto/creator=%s'%(th,f))
    print('  stable tiers:',len(r['stable']), r['stable'][:3])
