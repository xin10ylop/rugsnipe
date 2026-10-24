# Early trades (create .. +12 slots, failed txs too) for a random sample of universe launches he did NOT attempt, capped.
import sys, json, os, random, time
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP, common as C
from rugsnipe_analysis import events as E
from concurrent.futures import ThreadPoolExecutor
R = json.load(open(SP + "/univ/dataset.json"))
creates = {}
import glob
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): creates[c["mint"]] = c
U = [r for r in R if r["sid"] and r["dev"] >= 0.95 and r["delay"] is not None and r["delay"] < 3600 and not r["he"]]
random.seed(11); sample = random.sample(U, min(int(sys.argv[1]), len(U)))
def early(r):
    c = creates[r["mint"]]; p = f"{SP}/univ/early/{c['mint']}.json"
    if os.path.exists(p): return "have"
    bc = C.bonding_curve(c["mint"]); sigs = []; before = None; pages = 0
    while True:
        o = {"limit": 1000, "until": c["sig"]}
        if before: o["before"] = before
        s = tri.rpc("getSignaturesForAddress", [bc, o], cache=False) or []
        pages += 1; sigs += s
        if len(s) < 1000 or pages >= 6: break
        before = s[-1]["signature"]
    trunc = len(s) == 1000 and pages >= 6
    ear = sorted([x for x in sigs if x["slot"] <= c["slot"] + 12], key=lambda x: x["slot"])[:80]
    rows = []
    for x in ear:
        t = tri.tx(x["signature"])
        if not t: continue
        ks = PP.keys_of(t); ev = []
        for grp in (t.get("meta") or {}).get("innerInstructions") or []:
            for ix in grp["instructions"]:
                if ks[ix["programIdIndex"]] != C.PUMP: continue
                raw = C.b58d(ix.get("data", ""))
                if raw[:8] == E.EVENT_IX_TAG and raw[8:16] == E.DISC_TRADE:
                    try:
                        e = E.decode_trade(raw[8:])
                        if e.mint == c["mint"]: ev.append([e.user, int(e.is_buy), e.sol_amount, e.token_amount, e.virtual_sol_reserves, e.virtual_token_reserves, e.fee or 0, e.creator_fee or 0])
                    except Exception: pass
        a = PP.analyze(t)
        rows.append(dict(s=t["slot"], i=t.get("transactionIndex"), u=ks[0], err=x.get("err") is not None, ev=ev, tip=a["tip_total"], cup=a["cu_price"], cul=a["cu_limit"]))
    json.dump(dict(mint=c["mint"], n_sigs=len(sigs), pages=pages, trunc=trunc, rows=rows), open(p, "w"))
    return "trunc" if trunc else "ok"
with ThreadPoolExecutor(6) as ex: res = list(ex.map(early, sample))
import collections; print("skipped sample:", len(sample), collections.Counter(res))
