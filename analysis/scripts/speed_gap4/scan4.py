import json,sys,urllib.request,time
# usage: scan4.py tips.json start_slot n_blocks step out.jsonl
tips=json.load(open(sys.argv[1])); start=int(sys.argv[2]); n=int(sys.argv[3]); step=int(sys.argv[4]); out=open(sys.argv[5],'a')
a2p={a:p for p,l in tips.items() for a in l}
PUMP='6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P'; SYS='11111111111111111111111111111111'
B58='123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
def b58d(s):
    n=0
    for c in s: n=n*58+B58.index(c)
    b=n.to_bytes((n.bit_length()+7)//8,'big') if n else b''
    return b'\0'*(len(s)-len(s.lstrip('1')))+b
def rpc(m,p):
    r=urllib.request.Request('https://api.mainnet-beta.solana.com',data=json.dumps({"jsonrpc":"2.0","id":1,"method":m,"params":p}).encode(),headers={'content-type':'application/json'})
    return json.load(urllib.request.urlopen(r,timeout=90))
done=0; s=start
while done<n:
    try: b=rpc('getBlock',[s,{"encoding":"json","maxSupportedTransactionVersion":1,"transactionDetails":"full","rewards":False,"commitment":"finalized"}])
    except Exception as e:
        print('err',s,e,flush=True); time.sleep(3); continue
    if not b.get('result'):
        print('skip',s,(b.get('error') or {}).get('code'),flush=True); s-=step; time.sleep(1); continue
    for idx,t in enumerate(b['result']['transactions']):
        m=t['transaction']['message']; meta=t['meta'] or {}
        keys=list(m['accountKeys'])
        la=meta.get('loadedAddresses') or {}
        allkeys=keys+la.get('writable',[])+la.get('readonly',[])
        provs=sorted({a2p[k] for k in keys if k in a2p}); provs_alt=sorted({a2p[k] for k in allkeys[len(keys):] if k in a2p})
        ixs=m['instructions']
        nonce=False
        if ixs:
            i0=ixs[0]
            if allkeys[i0['programIdIndex']]==SYS:
                d=b58d(i0['data'])
                nonce=d[:4]==b'\x04\x00\x00\x00'
        pump=PUMP in allkeys
        tc=m.get('transactionConfig')
        is_vote = 'Vote111111111111111111111111111111111111111' in keys
        if is_vote: continue
        rec=dict(slot=s,idx=idx,v=str(t.get('version')),provs=provs,provs_alt=provs_alt,nonce=nonce,pump=pump,err=meta.get('err') is not None,
                 cu=meta.get('computeUnitsConsumed'),cost=meta.get('costUnits'),fee=meta.get('fee'),nsig=len(t['transaction']['signatures']),
                 tc=tc,alt=bool(m.get('addressTableLookups')),signer=keys[0],sig=t['transaction']['signatures'][0])
        if (pump or provs) and t.get('version')==1:
            # keep instruction summary for pump v1 rows
            rec['ix']=[(allkeys[i['programIdIndex']][:8],len(i['accounts']),b58d(i['data'])[:8].hex()) for i in ixs]
        out.write(json.dumps(rec)+'\n')
    out.flush(); done+=1; print('ok',s,len(b['result']['transactions']),flush=True)
    s-=step; time.sleep(1.0)
