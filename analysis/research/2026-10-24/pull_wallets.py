# Today's (UTC) transactions of our wallet and the copied wallet, full, ascending -> today/tx_{us,him}.jsonl; legs -> today/legs_{us,him}.json
import sys, json, time, calendar, collections, os
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/rivalpnl")
import common as C, pumpparse as PP
C.MIN_GAP = 0.26
T0 = calendar.timegm((2026, 10, 24, 0, 0, 0, 0, 0, 0))
WAL = {"us": C.OUR, "him": "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"}
for who, addr in WAL.items():
    out = f"{SP}/today/tx_{who}.jsonl"
    if not os.path.exists(out):
        f = open(out + ".part", "w"); tok = None; n = 0
        while True:
            opt = {"transactionDetails": "full", "sortOrder": "asc", "limit": 100, "encoding": "json",
                   "maxSupportedTransactionVersion": 1, "filters": {"blockTime": {"gte": T0}}}
            if tok: opt["paginationToken"] = tok
            try:
                r = C.rpc("getTransactionsForAddress", [addr, opt], cache=False) or {}
            except Exception as e:
                print("rpc error", C._redact(e)); time.sleep(2); continue
            d = r.get("data") or []
            for tx in d: f.write(json.dumps(tx, separators=(",", ":")) + "\n")
            n += len(d); tok = r.get("paginationToken")
            if n % 1000 < 100: print(who, time.strftime("%H:%M:%S"), n, flush=True)
            if not tok or len(d) < 100: break
        f.close(); os.rename(out + ".part", out); print(who, "txs", n, flush=True)
    legs = []
    for l in open(out):
        tx = json.loads(l); ks = PP.keys_of(tx); a = PP.analyze(tx); meta = tx["meta"]
        i = ks.index(addr) if addr in ks else None
        d = meta["postBalances"][i] - meta["preBalances"][i] if i is not None else 0
        pix = a["pump_ix"]
        kind = "buy" if any(x.startswith("buy") for x in pix) else ("sell" if any(x.startswith("sell") for x in pix) else "other")
        tr = [t for t in a["trades"] if t["user"] == addr]
        mints = [k for k in ks if k.endswith("pump")]
        legs.append(dict(sig=tx["transaction"]["signatures"][0], slot=tx["slot"], bt=tx.get("blockTime"), idx=tx.get("transactionIndex"),
                         kind=kind, err=meta["err"] is not None, errv=meta["err"], delta=d, fee=meta["fee"], tip=a["tip_total"],
                         tips=dict(a["tips"]), cup=a["cu_price"], cul=a["cu_limit"], pix=pix,
                         mint=(tr[0]["mint"] if tr else (mints[0] if mints else None)), ev=tr, signer=ks[0],
                         nsig=tx["transaction"]["message"]["header"]["numRequiredSignatures"]))
    legs.sort(key=lambda x: (x["slot"], x["idx"] or 0))
    json.dump(legs, open(f"{SP}/today/legs_{who}.json", "w"))
    c = collections.Counter((x["kind"], x["err"]) for x in legs)
    print(who, "legs", len(legs), dict(c), "first", time.strftime("%H:%M", time.gmtime(legs[0]["bt"])) if legs else "-",
          "last", time.strftime("%H:%M", time.gmtime(legs[-1]["bt"])) if legs else "-", flush=True)
