import json,sys,re,time,urllib.request,concurrent.futures as cf
ips=json.load(open('raw/top_ips.json'))
locs="frankfurt-fra2,amsterdam-ams3,london-lon1,tokyo-tyo2,newark-ewr1,singapore-sgp1,ashburn-iad1".split(',')
def q(job):
    loc,ip=job
    for a in range(2):
        try:
            r=json.load(urllib.request.urlopen(urllib.request.Request("https://lg.teraswitch.com/api/query/",json.dumps({"query_location":loc,"query_type":"ping","query_vrf":"global","query_target":ip}).encode(),{"content-type":"application/json"}),timeout=45))
            o=r.get("output","")
            m=re.search(r"= ([\d.]+)/([\d.]+)/([\d.]+)",o)
            return f"{loc}|{ip}",dict(min=m and float(m.group(1)),avg=m and float(m.group(2)),ts=r.get("timestamp"),note=None if m else o[-160:])
        except Exception as e:
            err=str(e); time.sleep(3)
    return f"{loc}|{ip}",dict(min=None,avg=None,note="ERR "+err)
jobs=[(l,i) for i in ips for l in locs]
out={}
with cf.ThreadPoolExecutor(int(sys.argv[1])) as ex:
    for f in cf.as_completed([ex.submit(q,j) for j in jobs]):
        k,v=f.result(); out[k]=v
        if len(out)%50==0: print(len(out),flush=True); json.dump(out,open('raw/lg_matrix.json','w'))
json.dump(out,open('raw/lg_matrix.json','w'),indent=0); print("done",len(out),sum(1 for v in out.values() if v['min'] is None),"failed")
