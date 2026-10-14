import json,base64
from rpc import rpc
def sim(tx,replace=True,label=''):
    r=rpc('simulateTransaction',[base64.b64encode(tx).decode(),{"encoding":"base64","sigVerify":False,"replaceRecentBlockhash":replace,"commitment":"processed"}])
    if 'error' in r: out=dict(label=label,rpc_error=r['error']); print(json.dumps(out)); return out
    v=r['result']['value']
    out=dict(label=label,size=len(tx),err=v.get('err'),units=v.get('unitsConsumed'),lads=v.get('loadedAccountsDataSize'),fee=v.get('fee'),
             logs_tail=(v.get('logs') or [])[-3:])
    print(json.dumps(out)); return out
