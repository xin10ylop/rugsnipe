# One coin's first slots by prefix of the mint (found through our wallet's recent transactions): every tx, who, buy/sell.
import sys, json, time
exec(open("recon.py").read().split("# 1. both wallets since T0")[0].replace("T0, T1 = int(sys.argv[1]), int(sys.argv[2])", "T0, T1 = int(sys.argv[2]), int(time.time()) + 60"))
pre = sys.argv[1]; m = None
for sg in sigs_since(US, T0, tries=4):
    t = tri.tx(sg)
    if not t: continue
    d = decode(t)
    for x in d["tr"]:
        if x["mint"].startswith(pre): m = x["mint"]
    if not m and d["mint_ix"] and d["mint_ix"].startswith(pre): m = d["mint_ix"]
    if m: break
print("mint", m)
bc = C.bonding_curve(m); sg = []; before = None
for _ in range(20):
    o = {"limit": 1000, "commitment": "confirmed"}
    if before: o["before"] = before
    s = tri.rpc("getSignaturesForAddress", [bc, o], cache=False) or []; sg += s
    if len(s) < 1000: break
    before = s[-1]["signature"]
sg.sort(key=lambda x: x["slot"]); c0 = sg[0]["slot"]
rows = sorted((decode(t) for t in (tri.tx(x["signature"]) for x in sg if x["slot"] <= c0 + 12) if t), key=lambda r: (r["slot"], r["i"]))
cr = next(r for r in rows if r["cr"])
print("create", time.strftime("%H:%M:%S", time.gmtime(cr["bt"])), "signer", cr["signer"], "creator", cr["cr"]["creator"], "uri", cr["cr"]["uri"], "name", cr["cr"]["name"])
ins = {cr["signer"], cr["cr"]["creator"], cr["cr"]["user"]}
for r in rows:
    who = "US" if r["signer"] == US else "HIM" if r["signer"] == A else ("DEV" if r["signer"] in ins else r["signer"][:6])
    tr = [(("B" if x["b"] else "S"), round(x["sol"] / 1e9, 3), ("US" if x["u"] == US else "HIM" if x["u"] == A else x["u"][:6])) for x in r["tr"] if x["mint"] == m]
    print(f"  +{r['slot']-c0:2d} #{r['i']:4d} {who:6s} {'FAIL ' + str(r['errv'])[:40] if r['err'] else 'ok'} {'CREATE ' if r['cr'] else ''}{tr}")
