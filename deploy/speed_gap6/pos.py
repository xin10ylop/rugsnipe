import json, sys, time, random, urllib.request
RPC="https://api.mainnet-beta.solana.com"
VOTE="Vote111111111111111111111111111111111111111"
def rpc(m,p):
    for i in range(6):
        try:
            b=json.dumps({"jsonrpc":"2.0","id":1,"method":m,"params":p}).encode()
            r=json.load(urllib.request.urlopen(urllib.request.Request(RPC,b,{"content-type":"application/json"}),timeout=90))
            if "result" in r: return r["result"]
            print(r.get("error"),file=sys.stderr)
        except Exception as e: print(e,file=sys.stderr)
        time.sleep(3+4*i)
    return None
cr=[c for c in json.load(open('raw/creates.json')) if not c['err']]
byslot={}
for c in cr: byslot.setdefault(c['slot'],set()).add(c['sig'])
random.seed(7); slots=random.sample(sorted(byslot),int(sys.argv[1]))
out=[]
for s in slots:
    b=rpc("getBlock",[s,{"encoding":"json","transactionDetails":"accounts","rewards":False,"maxSupportedTransactionVersion":1,"commitment":"confirmed"}])
    if not b: continue
    txs=b["transactions"]; n=len(txs)
    isvote=[any(k["pubkey"]==VOTE for k in t["transaction"]["accountKeys"]) for t in txs]
    nv=[i for i in range(n) if not isvote[i]]
    rank={i:j for j,i in enumerate(nv)}
    for i,t in enumerate(txs):
        sig=t["transaction"]["signatures"][0]
        if sig in byslot[s]:
            out.append({"slot":s,"idx":i,"n":n,"nv_idx":rank.get(i),"nv":len(nv),"pos_all":i/max(1,n-1),"pos_nv":rank.get(i,0)/max(1,len(nv)-1),"bt":b.get("blockTime")})
    time.sleep(0.25)
json.dump(out,open('raw/positions.json','w')); print(len(out),"creates in",len(slots),"blocks",file=sys.stderr)
