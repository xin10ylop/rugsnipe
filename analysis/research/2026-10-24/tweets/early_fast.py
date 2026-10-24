# Early trades (create .. +12 slots, failed txs too) with the window anchored on a later create, for a launch list.
#   RS_DAY=24 python3 -I univ/early_fast.py MODE N      MODE: others (random universe launches he skipped) | r5b | all
import sys, json, os, random, glob, time, collections, calendar, re
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP, common as C
from rugsnipe_analysis import events as E
from concurrent.futures import ThreadPoolExecutor
DAYN = int(os.environ.get("RS_DAY", "24")); MODE, N = sys.argv[1], int(sys.argv[2])
R = json.load(open(SP + ("/univ/dataset.json" if DAYN == 24 else f"/univ/dataset_{DAYN}.json")))
creates = []
for f in glob.glob(SP + "/univ/creates_*.json"): creates += json.load(open(f))
creates.sort(key=lambda c: (c["slot"], c["idx"] or 0)); bym = {c["mint"]: c for c in creates}; slots = [c["slot"] for c in creates]
import bisect
U = [r for r in R if r["sid"] and r["dev"] >= 0.95 and r["delay"] is not None and r["delay"] < 3600]
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
r5b = lambda r: TEMPLATE(r["dev"]) and r["host"] in TOOLS and r["tweet_rank"] == 1
pool = [r for r in U if (MODE == "all") or (MODE == "others" and not r["he"]) or (MODE == "r5b" and r5b(r))]
random.seed(23); todo = random.sample(pool, min(N, len(pool)))
if MODE == "tt":  # the template+tool universe: every launch he attempted, plus N of the rest at random
    TT = [r for r in R if r["dev"] >= 0.95 and TEMPLATE(r["dev"]) and r["host"] in TOOLS]
    rest = [r for r in TT if not r["he"]]; random.seed(23)
    todo = [r for r in TT if r["he"]] + random.sample(rest, min(N, len(rest)))
SKIP_ERR = os.environ.get("RS_SKIP_ERR") == "1"  # failed txs carry no trade: skip fetching them
def early(r):
    c = bym[r["mint"]]; p = f"{SP}/univ/early/{c['mint']}.json"
    if os.path.exists(p):
        o = json.load(open(p))
        if not o.get("trunc"): return "have"
    k = bisect.bisect_left(slots, c["slot"] + 13)
    if k >= len(creates): return "no anchor"
    anchor = creates[k]["sig"]
    bc = C.bonding_curve(c["mint"])
    sigs = tri.rpc("getSignaturesForAddress", [bc, {"limit": 1000, "before": anchor, "until": c["sig"]}], cache=False) or []
    ear = sorted([x for x in sigs if x["slot"] <= c["slot"] + 12], key=lambda x: x["slot"])[:160]
    rows = []
    for x in ear:
        if SKIP_ERR and x.get("err") is not None:
            rows.append(dict(s=x["slot"], i=None, u=None, err=True, ev=[], tip=None, cup=None, cul=None)); continue
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
    json.dump(dict(mint=c["mint"], n_sigs=len(sigs), pages=1, trunc=len(sigs) >= 1000, rows=rows, anchored=True), open(p, "w"))
    return "ok"
t0 = time.time()
with ThreadPoolExecutor(6) as ex: res = list(ex.map(early, todo))
print(f"day {DAYN} mode {MODE}: {len(todo)} launches in {time.time()-t0:.0f}s", collections.Counter(res), flush=True)
