import json,sys,re,time,urllib.request
T=dict(l.split(None,1) for l in """64.130.50.88 Jito BE FRA
64.130.50.91 Helius fra-sender
198.13.140.105 Harmonic FRA Remote TPU
198.13.140.227 BAM fra-bam-2
198.13.140.16 DZ dz100a-fra2-tsw
64.130.32.214 Helius LaserStream FRA
64.130.47.135 Nozomi fra2
64.130.40.42 NextBlock FRA
64.130.52.119 Jito BE AMS
64.130.42.91 Helius ams-sender
63.254.166.232 Harmonic AMS Remote TPU
63.254.167.197 BAM ams-bam-2
64.130.52.8 DZ dz100a-ams2-tsw
64.130.46.242 Harmonic LON Remote TPU
88.216.192.51 BAM sqq-bam-2
109.94.96.69 Jito relayer (Latitude FRA)""".splitlines() for _ in [0])
locs=sys.argv[1].split(','); out={}
for loc in locs:
  for ip,name in T.items():
    for a in range(3):
      try:
        r=json.load(urllib.request.urlopen(urllib.request.Request("https://lg.teraswitch.com/api/query/",json.dumps({"query_location":loc,"query_type":"ping","query_vrf":"global","query_target":ip}).encode(),{"content-type":"application/json"}),timeout=60))
        o=r.get("output","")
        m=re.search(r"= ([\d.]+)/([\d.]+)/([\d.]+)",o); src=re.search(r"from (\S+) :",o); ttl=re.findall(r"ttl=(\d+)",o)
        out[f"{loc}|{ip}"]=dict(name=name,min=m and float(m.group(1)),avg=m and float(m.group(2)),max=m and float(m.group(3)),src=src and src.group(1),ttl=ttl[:1],ts=r.get("timestamp"),raw=None if m else o[:200])
        print(loc,ip,name,out[f"{loc}|{ip}"]["min"],out[f"{loc}|{ip}"]["avg"],ttl[:1],src and src.group(1),flush=True); break
      except Exception as e: print("ERR",loc,ip,e,flush=True); time.sleep(5)
    time.sleep(1)
json.dump(out,open(sys.argv[2],"w"),indent=1)
