# Races we won since 17:29: our bid, and the bids of the buys that failed right behind ours in the same slot or the next
# (they lost to us): would a 1.5M bid still have won? Bid = priority fee (cu price x cu limit) + tips.
import sys, json, collections, time
exec(open("recon.py").read().split("# 1. both wallets since T0")[0].replace("T0, T1 = int(sys.argv[1]), int(sys.argv[2])", "T0, T1 = 1792862944, int(time.time())"))
J = json.load(open(SP + "/live/recon.json"))
won = [r for r in J["res"] if "note" not in r and r["us"]["landed"]]
print("races we won (landed buys):", len(won))
def bid_of(d):
    cul = 200000
    t = d.get("_t")
    return (d.get("cup") or 0) * 0 + (d["tip"] or 0)
rows = []
for r in won:
    m = r["mint"]; bc = C.bonding_curve(m)
    sg = []; before = None
    for _ in range(30):
        o = {"limit": 1000, "commitment": "confirmed"}
        if before: o["before"] = before
        s = tri.rpc("getSignaturesForAddress", [bc, o], cache=False) or []; sg += s
        if len(s) < 1000: break
        before = s[-1]["signature"]
    sg.sort(key=lambda x: x["slot"]); c0 = sg[0]["slot"]
    ds = []
    for x in [x for x in sg if x["slot"] <= c0 + 3]:
        t = tri.tx(x["signature"])
        if not t: continue
        d = decode(t); a = PP.analyze(t)
        d["prio"] = (a["cu_price"] or 0) * (a["cu_limit"] or 200000) // 1_000_000
        d["bid"] = d["prio"] + (a["tip_total"] or 0); ds.append(d)
    ds.sort(key=lambda d: (d["slot"], d["i"]))
    ours = next((d for d in ds if d["signer"] == US and not d["err"] and any(x["u"] == US and x["b"] for x in d["tr"])), None)
    if not ours: continue
    behind = [d for d in ds if d["err"] and d["signer"] not in (US,) and (d["slot"], d["i"]) > (ours["slot"], ours["i"]) and d["slot"] <= ours["slot"] + 1]
    rows.append(dict(m=m[:8], our_bid=ours["bid"], our_slot=ours["slot"] - c0, our_pos=ours["i"], n_behind=len(behind),
                     behind_bids=sorted([d["bid"] for d in behind], reverse=True)[:6], him=[d["bid"] for d in behind if d["signer"] == A]))
for x in rows: print(json.dumps(x))
allb = [b for x in rows for b in x["behind_bids"]]
print(f"failed buys behind our winning buys: {len(allb)}; bid > 3M: {sum(b > 3_000_000 for b in allb)}, 1.5-3M: {sum(1_500_000 < b <= 3_000_000 for b in allb)}, <= 1.5M: {sum(b <= 1_500_000 for b in allb)}")
print(f"races where the best failed bid behind us was 1.5-3M (a 1.5M bid could have lost): {sum(1 for x in rows if x['behind_bids'] and 1_500_000 < x['behind_bids'][0] <= 3_000_000)} of {len(rows)}")
