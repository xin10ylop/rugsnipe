"""For each mint: signatures touching the mint between create and window end; non-trade successful
txs -> getTransaction -> per-owner token deltas of this mint (transfers / consolidations)."""
import json, sys, threading, time
from concurrent.futures import ThreadPoolExecutor
sys.path.insert(0, '/home/user/rugsnipe/analysis')
from rugsnipe_analysis.rpc import RpcClient
EPS=("https://api.mainnet-beta.solana.com","https://api.mainnet.solana.com")
import rugsnipe_analysis.rpc as R
R.ARCHIVE_ENDPOINTS = EPS
rpc = RpcClient(endpoints=EPS, max_rps=float(sys.argv[2]) if len(sys.argv)>2 else 5, max_retries=10)
sel = {s['mint']:s for s in json.load(open('sel.json'))}
PUMP='6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P'
out='xfers.jsonl'; done=set()
try:
    for l in open(out): done.add(json.loads(l)['mint'])
except FileNotFoundError: pass
lock=threading.Lock(); fo=open(out,'a')
HORIZON=int(sys.argv[3]) if len(sys.argv)>3 else 900
def go(tr):
    mint=tr['mint']; s=sel[mint]
    trades=[t for t in tr['trades'] if t[3] <= s['bt']+HORIZON]
    tsigs={t[9] for t in tr['trades']}
    before = trades[-1][9] if trades else None
    sigs=[]; pages=0; complete=False
    # include the window-end trade itself is excluded by 'before'; fine (it's a trade)
    while pages < 12:
        page = rpc.get_signatures(mint, before=before, until=s['sig'], limit=1000)
        pages+=1; sigs.extend(page)
        if len(page)<1000: complete=True; break
        before=page[-1]['signature']
    cand=[x for x in sigs if x.get('err') is None and x['signature'] not in tsigs and x['signature']!=s['sig']]
    xs=[]
    for x in cand[:30]:
        tx=rpc.get_transaction(x['signature'])
        if not tx: continue
        m=tx.get('meta') or {}
        d={}
        for b in m.get('preTokenBalances') or []:
            if b.get('mint')==mint: d[b.get('owner')]=d.get(b.get('owner'),0)-float(b['uiTokenAmount']['uiAmount'] or 0)
        for b in m.get('postTokenBalances') or []:
            if b.get('mint')==mint: d[b.get('owner')]=d.get(b.get('owner'),0)+float(b['uiTokenAmount']['uiAmount'] or 0)
        d={k:round(v,6) for k,v in d.items() if abs(v)>1e-6}
        keys=[a['pubkey'] if isinstance(a,dict) else a for a in tx['transaction']['message']['accountKeys']]
        signers=[a['pubkey'] for a in tx['transaction']['message']['accountKeys'] if isinstance(a,dict) and a.get('signer')]
        ixt=[]
        for ix in tx['transaction']['message']['instructions']:
            p=ix.get('parsed'); ixt.append((ix.get('programId') or '')[:6]+':'+(p.get('type') if isinstance(p,dict) else ''))
        for inn in m.get('innerInstructions') or []:
            for ix in inn['instructions']:
                p=ix.get('parsed'); 
                if isinstance(p,dict): ixt.append((ix.get('programId') or '')[:6]+':'+p.get('type',''))
        xs.append({'sig':x['signature'],'slot':tx['slot'],'bt':tx.get('blockTime'),'d':d,'signers':signers,'pump':PUMP in keys,'ix':ixt[:12]})
    res={'mint':mint,'n_sigs':len(sigs),'pages':pages,'sig_complete':complete,'n_cand':len(cand),'xs':xs}
    with lock:
        fo.write(json.dumps(res,separators=(',',':'))+'\n'); fo.flush(); done.add(mint)
        if len(done)%25==0: print(time.strftime('%H:%M:%S'),len(done),rpc.stats,flush=True)
rows=[]
import os
order=json.load(open('xf_order.json')) if os.path.exists('xf_order.json') else None
allr={}
for l in open('trades.jsonl'):
    r=json.loads(l)
    if r['mint'] not in done: allr[r['mint']]=r
rows=[allr[m] for m in order if m in allr] if order else list(allr.values())
lim=int(sys.argv[1]) if len(sys.argv)>1 else len(rows)
with ThreadPoolExecutor(4) as ex:
    for f in [ex.submit(go,r) for r in rows[:lim]]:
        try: f.result()
        except Exception as e: print('fail',type(e).__name__,str(e)[:200],flush=True)
print('done',len(done),rpc.stats)
