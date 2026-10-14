import json,urllib.request,time
out={}
for off in range(0,600,50):
    u=f"https://frontend-api-v3.pump.fun/coins?offset={off}&limit=50&sort=created_timestamp&order=DESC&includeNsfw=true"
    for t in range(4):
        try:
            d=json.load(urllib.request.urlopen(urllib.request.Request(u,headers={'user-agent':'Mozilla/5.0','accept':'application/json'}),timeout=30)); break
        except Exception as e: print('retry',off,e); time.sleep(3)
    for c in d: out[c['mint']]=dict(mint=c['mint'],created=c['created_timestamp'],tp=c.get('token_program'),complete=c['complete'],program=c.get('program'),mcap=c.get('market_cap'))
    print(off,len(out)); time.sleep(0.5)
json.dump(list(out.values()),open('mints_api.json','w'))
