import json,urllib.request,time
EPS=['https://solana-rpc.publicnode.com','https://api.mainnet-beta.solana.com']
def rpc(m,p,ep=0,tries=8):
    for i in range(tries):
        try:
            q=urllib.request.Request(EPS[(ep+i)%len(EPS)],data=json.dumps({"jsonrpc":"2.0","id":1,"method":m,"params":p}).encode(),headers={'content-type':'application/json'})
            return json.load(urllib.request.urlopen(q,timeout=60))
        except Exception as e:
            last=e; time.sleep(3+2*i)
    raise last
