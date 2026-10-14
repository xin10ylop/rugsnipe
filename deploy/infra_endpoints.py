#!/usr/bin/env python3
"""Resolve Solana sender/feed endpoints and geolocate them (ipinfo.io). Usage: python3 -I deploy/infra_endpoints.py out.json"""
import socket, json, subprocess, sys
hosts = """
jito ny.mainnet.block-engine.jito.wtf
jito amsterdam.mainnet.block-engine.jito.wtf
jito frankfurt.mainnet.block-engine.jito.wtf
jito london.mainnet.block-engine.jito.wtf
jito dublin.mainnet.block-engine.jito.wtf
jito tokyo.mainnet.block-engine.jito.wtf
jito singapore.mainnet.block-engine.jito.wtf
jito slc.mainnet.block-engine.jito.wtf
helius-sender ewr-sender.helius-rpc.com
helius-sender fra-sender.helius-rpc.com
helius-sender ams-sender.helius-rpc.com
helius-sender lon-sender.helius-rpc.com
helius-sender slc-sender.helius-rpc.com
helius-sender sg-sender.helius-rpc.com
helius-sender tyo-sender.helius-rpc.com
laserstream laserstream-mainnet-ewr.helius-rpc.com
laserstream laserstream-mainnet-pitt.helius-rpc.com
laserstream laserstream-mainnet-fra.helius-rpc.com
laserstream laserstream-mainnet-ams.helius-rpc.com
laserstream laserstream-mainnet-lon.helius-rpc.com
laserstream laserstream-mainnet-lax.helius-rpc.com
laserstream laserstream-mainnet-tyo.helius-rpc.com
laserstream laserstream-mainnet-sgp.helius-rpc.com
nozomi pit1.nozomi.temporal.xyz
nozomi ewr1.nozomi.temporal.xyz
nozomi ash1.nozomi.temporal.xyz
nozomi lax1.nozomi.temporal.xyz
nozomi fra2.nozomi.temporal.xyz
nozomi ams1.nozomi.temporal.xyz
nozomi lon1.nozomi.temporal.xyz
nozomi tyo1.nozomi.temporal.xyz
nozomi sgp1.nozomi.temporal.xyz
0slot ny.0slot.trade
0slot de1.0slot.trade
0slot de2.0slot.trade
0slot ams.0slot.trade
0slot la.0slot.trade
0slot jp.0slot.trade
nextblock ny.nextblock.io
nextblock fra.nextblock.io
nextblock ams.nextblock.io
nextblock london.nextblock.io
nextblock tokyo.nextblock.io
nextblock slc.nextblock.io
bloxroute ny.solana.dex.blxrbdn.com
bloxroute germany.solana.dex.blxrbdn.com
bloxroute amsterdam.solana.dex.blxrbdn.com
bloxroute uk.solana.dex.blxrbdn.com
bloxroute tokyo.solana.dex.blxrbdn.com
astralane fr.gateway.astralane.io
astralane ams.gateway.astralane.io
astralane ny.gateway.astralane.io
astralane jp.gateway.astralane.io
blockrazor frankfurt.solana.blockrazor.xyz
blockrazor newyork.solana.blockrazor.xyz
triton ams.rpcpool.com
triton fra.rpcpool.com
"""
out=[]
for line in hosts.strip().splitlines():
    prov,h=line.split()
    try: ip=socket.gethostbyname(h)
    except Exception as e:
        out.append(dict(provider=prov,host=h,ip=None)); print(prov,h,'NXDOMAIN'); continue
    try:
        j=json.loads(subprocess.run(['curl','-sS','-m','10',f'https://ipinfo.io/{ip}/json'],capture_output=True,text=True).stdout)
    except Exception: j={}
    r=dict(provider=prov,host=h,ip=ip,city=j.get('city'),country=j.get('country'),org=j.get('org'))
    out.append(r); print('%-13s %-42s %-16s %-22s %s %s'%(prov,h,ip,r['city'],r['country'],r['org']))
json.dump(out,open(sys.argv[1],'w'),indent=1)
