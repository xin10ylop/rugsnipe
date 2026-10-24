# For candidate creates (launch-tool host or attempted by him), the create slot's trades on the curve: dev bundle wallets,
# SOL, the first outside buyer (who, when), and every trade up to create + 12 slots (for counterfactual value).
import sys, json, time, collections, calendar
from urllib.parse import urlparse
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP, common as C
from rugsnipe_analysis import events as E
T0 = int(sys.argv[1])
U = json.load(open(f"{SP}/univ/creates_{T0}.json"))
LH = json.load(open(SP + "/today/legs_him.json")); h_att = {x["mint"] for x in LH if x["kind"] == "buy" and x["mint"]}
TOOLS = {"metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io"}
cand = [c for c in U if (urlparse(c["uri"]).netloc in TOOLS and c["dev_sol"] >= 0.95) or c["mint"] in h_att]
print("candidates", len(cand), flush=True)
out = []
for k, c in enumerate(cand):
    bc = C.bonding_curve(c["mint"])
    s = tri.rpc("getSignaturesForAddress", [bc, {"limit": 1000, "until": c["sig"]}], cache=True) or []
    early = sorted([x for x in s if x["slot"] <= c["slot"] + 12], key=lambda x: x["slot"])
    trades = []
    for x in early[:60]:
        t = tri.tx(x["signature"])
        if not t: continue
        ks = PP.keys_of(t)
        for grp in (t.get("meta") or {}).get("innerInstructions") or []:
            for ix in grp["instructions"]:
                if ks[ix["programIdIndex"]] != C.PUMP: continue
                raw = C.b58d(ix.get("data", ""))
                if raw[:8] == E.EVENT_IX_TAG and raw[8:16] == E.DISC_TRADE:
                    try:
                        e = E.decode_trade(raw[8:])
                        if e.mint == c["mint"]: trades.append([t["slot"], t.get("transactionIndex"), e.user, int(e.is_buy), e.sol_amount, e.token_amount, e.virtual_sol_reserves, e.virtual_token_reserves, ks[0], x.get("err") is not None])
                    except Exception: pass
    trades.sort(key=lambda r: (r[0], r[1] or 0))
    c2 = dict(c); c2.update(n_sigs=len(s), early=len(early), trades=trades, he=c["mint"] in h_att)
    out.append(c2)
    if k % 20 == 0: print(time.strftime("%H:%M:%S"), k, "of", len(cand), flush=True)
json.dump(out, open(f"{SP}/univ/bundles_{T0}.json", "w"))
print("done", len(out), flush=True)
