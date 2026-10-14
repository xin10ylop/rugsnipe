import json, sys, time, urllib.request
RPC="https://api.mainnet-beta.solana.com"
MA="TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM"
def rpc(m,p):
    for i in range(6):
        try:
            b=json.dumps({"jsonrpc":"2.0","id":1,"method":m,"params":p}).encode()
            r=json.load(urllib.request.urlopen(urllib.request.Request(RPC,b,{"content-type":"application/json"}),timeout=60))
            if "result" in r: return r["result"]
            print(r.get("error"),file=sys.stderr)
        except Exception as e: print(e,file=sys.stderr)
        time.sleep(2+3*i)
    raise SystemExit("rpc fail")
stop=int(sys.argv[1]); out=[]; before=sys.argv[3] if len(sys.argv)>3 else None
while True:
    p={"limit":1000}
    if before: p["before"]=before
    r=rpc("getSignaturesForAddress",[MA,p])
    if not r: break
    for x in r: out.append({"sig":x["signature"],"slot":x["slot"],"err":x["err"] is not None,"bt":x.get("blockTime")})
    before=r[-1]["signature"]
    print(len(out), r[-1]["slot"], file=sys.stderr)
    if r[-1]["slot"]<stop: break
    time.sleep(0.3)
json.dump(out,open(sys.argv[2],"w"))
