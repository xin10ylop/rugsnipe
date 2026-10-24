# Per landed trade today (ours and his): the launch, our fill, his attempt relative to ours, other snipers, insiders'
# sells while held, chain-true P&L, and the curve state at every slot (for counterfactual exits). -> today/trades.json
import json, collections, time
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"; A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"
FEE = 0.0125
LU = json.load(open(SP + "/today/legs_us.json")); LH = json.load(open(SP + "/today/legs_him.json"))
def by_mint(L):
    d = collections.defaultdict(list)
    for x in L:
        if x["mint"] and x["kind"] in ("buy", "sell"): d[x["mint"]].append(x)
    return d
BU, BH = by_mint(LU), by_mint(LH)
def trip(legs):
    """chain-true round trip of one wallet on one mint: landed buy cost, sells, failed copies"""
    buys = [x for x in legs if x["kind"] == "buy" and not x["err"]]
    if not buys: return None
    b = buys[0]
    cost = -sum(x["delta"] for x in buys) / 1e9
    sells = [x for x in legs if x["kind"] == "sell"]
    proceeds = sum(x["delta"] for x in sells) / 1e9
    fails = [x for x in legs if x["kind"] == "buy" and x["err"]]
    ok_sells = [x for x in sells if not x["err"]]
    sol_in = sum(e["sol"] for x in buys for e in x["ev"] if e.get("is_buy")) / 1e9 if buys[0]["ev"] else None
    return dict(buy_slot=b["slot"], buy_idx=b["idx"], bt=b["bt"], cost=cost, proceeds=proceeds, net=proceeds - cost,
                closed=bool(ok_sells), sell_slot=(ok_sells[-1]["slot"] if ok_sells else None), n_sells=len(ok_sells),
                failed_sells=sum(1 for x in sells if x["err"]), fail_copies=len(fails), fail_cost=-sum(x["delta"] for x in fails) / 1e9,
                tip=b["tip"], prio=(b["cup"] or 0) * (b["cul"] or 0) / 1e12, fee=b["fee"], sol_in=sol_in,
                tok=sum(e["tok"] for x in buys for e in x["ev"] if e.get("is_buy")) if buys[0]["ev"] else None)
out = []
for l in open(SP + "/today/tapes.jsonl"):
    o = json.loads(l); m = o["mint"]
    rows = sorted(o["txs"], key=lambda t: (t["s"], t["i"] if t["i"] is not None else 10**6))
    ctx = next((t for t in rows if any(x.startswith("create") for x in t["ix"]) or any(e[0] == "C" for e in t["ev"])), None)
    if ctx is None: continue
    cs, ci = ctx["s"], ctx["i"]
    cev = next((e for e in ctx["ev"] if e[0] == "C"), None)
    trades = []   # every trade event, in block order: dict(s,i,u,b,sol,tok,vs,vt,sig,signer)
    for t in rows:
        for e in t["ev"]:
            if e[0] != "T" or len(e) > 12: continue
            trades.append(dict(s=t["s"], i=t["i"], u=e[1], b=bool(e[2]), sol=e[3], tok=e[4], vs=e[5], vt=e[6], fee=e[9], cfee=e[10], cre=t is ctx, signer=t["u"]))
    cset = set(ctx["signers"]) | ({cev[1], cev[2]} if cev else set()) | {x["u"] for x in trades if x["cre"]}
    cset -= {W, A}
    # the dev bundle: the creator set's create-slot buys, plus buys chained <= 2 apart within create+5 (as the dry run)
    bund = {k for k, x in enumerate(trades) if x["s"] == cs and x["b"] and (x["cre"] or x["u"] in cset)}
    prev = ci
    for k, x in enumerate(trades):
        if x["s"] != cs or x["i"] is None or x["i"] <= ci: continue
        if x["u"] in (W, A): break
        if x["i"] - prev <= 2 and x["i"] - ci <= 5 and x["b"]: bund.add(k); prev = x["i"]
        elif x["i"] - prev > 2 or x["i"] - ci > 5: break
    bw = {trades[k]["u"] for k in bund} - cset
    ins = cset | bw
    pb = max(bund) if bund else None
    # every buy ATTEMPT (failed ones too) in the create slot and the next 2, in block order
    attempts = [dict(s=t["s"], i=t["i"], u=t["u"], err=t["err"], tip=t["tip"], prio=(t["cup"] or 0) * (t["cul"] or 0) / 1e12)
                for t in rows if t["s"] <= cs + 3 and any(x.startswith("buy") for x in t["ix"]) and t is not ctx]
    # curve state at the end of every slot (after all its trades), from the create on
    end_state = {}
    for x in trades: end_state[x["s"]] = (x["vs"], x["vt"])
    rec = dict(mint=m, cs=cs, ci=ci, creator=(cev[2] if cev else ctx["u"]), dev=ctx["u"],
               dev_sol=sum(x["sol"] for x in trades if x["cre"] and x["b"]) / 1e9,
               bundle_sol=sum(trades[k]["sol"] for k in bund) / 1e9, n_bw=len(bw), post_bundle=((trades[pb]["vs"], trades[pb]["vt"]) if pb is not None else None),
               trades=[[x["s"], x["i"], x["u"], int(x["b"]), x["sol"], x["tok"], x["vs"], x["vt"], int(x["u"] in ins)] for x in trades],
               attempts=attempts, end_state={str(k): v for k, v in end_state.items()}, hi=o["hi"], trunc=o["trunc"])
    for who, B in (("us", BU), ("him", BH)):
        t = trip(B.get(m, []))
        rec[who] = t
        if t:
            # rank: outside buys landed between the bundle and this wallet's buy
            k = next((k for k, x in enumerate(trades) if x["u"] == (W if who == "us" else A) and x["b"]), None)
            rec[who]["rank"] = (sum(1 for x in trades[(pb + 1 if pb is not None else 0):k] if x["b"] and x["u"] not in ins) + 1) if k is not None else None
            rec[who]["fill_state"] = (trades[k]["vs"], trades[k]["vt"]) if k is not None else None
    # his buy attempts on this mint (failed or landed), and ours
    for who, B in (("him_try", BH), ("us_try", BU)):
        rec[who] = [dict(s=x["slot"], i=x["idx"], err=x["err"], errv=str(x["errv"])[:60] if x["err"] else None, tip=x["tip"] / 1e6,
                         prio=(x["cup"] or 0) * (x["cul"] or 0) / 1e12) for x in B.get(m, []) if x["kind"] == "buy"]
    out.append(rec)
json.dump(out, open(SP + "/today/trades.json", "w"))
us = [r for r in out if r["us"]]; him = [r for r in out if r["him"]]
print(f"launches {len(out)}; our landed trades {len(us)} (closed {sum(1 for r in us if r['us']['closed'])}); his {len(him)} (closed {sum(1 for r in him if r['him']['closed'])})")
print("our net SOL (landed trades) %+.4f; failed copies on these mints %.4f" % (sum(r["us"]["net"] for r in us if r["us"]["closed"]), sum(r["us"]["fail_cost"] for r in us)))
print("his net SOL (landed trades) %+.4f" % sum(r["him"]["net"] for r in him if r["him"]["closed"]))
