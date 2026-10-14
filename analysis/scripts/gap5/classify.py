import sys,json,base64,struct,collections; sys.path.insert(0,'.')
from sol import *
CV2=bytes.fromhex('d6904cec5f8b31b4'); CR=bytes.fromhex('181ec828051c0777'); CEV=bytes.fromhex('1b72a94ddeeb6376')
txs=json.load(open('creates_raw.json'))
def keys(t):
    m=t['transaction']['message']; k=list(m['accountKeys'])
    la=t['meta'].get('loadedAddresses') or {}
    return k+la.get('writable',[])+la.get('readonly',[])
def parse_args(d):
    o=8; out={}
    for f in ('name','symbol','uri'):
        n=struct.unpack_from('<I',d,o)[0]; out['L'+f]=n; o+=4+n
    out['creator']=b58e(d[o:o+32]); o+=32; out['mayhem_off']=o; out['mayhem']=d[o]; o+=1
    out['tail_bytes']=d[o:].hex(); return out
def cev(d):
    o=8
    for f in range(3): n=struct.unpack_from('<I',d,o)[0]; o+=4+n
    o+=32*4+8+8*4+32; return d[o]
rows=[]; C=collections.Counter()
for t in txs:
    K=keys(t); m=t['transaction']['message']
    ixs=[(True,i) for i in m['instructions']]+[(False,i) for g in (t['meta'].get('innerInstructions') or []) for i in g['instructions']]
    evs=[base64.b64decode(l.split('Program data: ')[1]) for l in t['meta']['logMessages'] if l.startswith('Program data: ')]
    ev_may=[cev(e) for e in evs if e[:8]==CEV]
    found=False
    for top,ix in ixs:
        if K[ix['programIdIndex']]!=PUMP: continue
        d=b58d(ix['data'])
        if d[:8]==CV2:
            a=parse_args(d); acc=[K[i] for i in ix['accounts']]; tail=acc[16:]
            q='none' if not tail else ('WSOL' if tail[0]==WSOL else ('USDC' if tail[0].startswith('EPjF') else 'pumpcoin/other'))
            rows.append(dict(kind='create_v2',top=top,ver=t.get('version'),ntail=len(tail),q=q,qc=(tail[3] if len(tail)>3 else None),mayhem=a['mayhem'],ev_may=ev_may[0] if ev_may else None,
                             off=a['mayhem_off'],tailb=a['tail_bytes'],signer=K[0],creator=a['creator'],user=acc[5],nacc=len(acc),tp=acc[7])); found=True
        elif d[:8]==CR:
            a=parse_args(d); acc=[K[i] for i in ix['accounts']]
            rows.append(dict(kind='create',top=top,ver=t.get('version'),nacc=len(acc),acc=acc,args=a,datalen=len(d),sig=t['transaction']['signatures'][0],ev_may=ev_may[0] if ev_may else None,tp=acc[9] if len(acc)>9 else None)); found=True
    if not found: C['no pump create ix found']+=1
json.dump(rows,open('creates_class.json','w'))
print('txs',len(txs),C)
print(collections.Counter((r['kind'],r['top']) for r in rows))
v2=[r for r in rows if r['kind']=='create_v2']
print('tail count x quote',collections.Counter((r['ntail'],r['q']) for r in v2))
print('quote_control at tail[3]',collections.Counter(r['qc'] for r in v2 if r['ntail']>3))
print('mayhem arg==event',sum(1 for r in v2 if r['mayhem']==r['ev_may']),'/',len(v2),' mayhem share',sum(r['mayhem'] for r in v2))
print('trailing option bytes',collections.Counter(r['tailb'] for r in v2).most_common(8))
print('creator!=signer',sum(1 for r in v2 if r['creator']!=r['signer']),'creator!=user',sum(1 for r in v2 if r['creator']!=r['user']),'user!=signer',sum(1 for r in v2 if r['user']!=r['signer']))
print('versions',collections.Counter(r['ver'] for r in rows))
for r in rows:
    if r['kind']=='create': print(json.dumps(r)[:1500])
