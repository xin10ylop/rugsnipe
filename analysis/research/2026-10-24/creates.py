# Every pump.fun create between T0 and T1 (UTC unix), decoded: mint, creator, create signer, name/symbol/uri, dev buy,
# create ix. -> univ/creates_<T0>.json (resumable through the tri cache)
import sys, json, time, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP, common as C
from rugsnipe_analysis import events as E
MA = "TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM"
T0, T1 = int(sys.argv[1]), int(sys.argv[2])
sigs = []; before = None
while True:
    s = tri.sigs(MA, before=before)
    if not s: break
    before = s[-1]["signature"]
    sigs += [x for x in s if T0 <= (x.get("blockTime") or 0) < T1 and not x.get("err")]
    if (s[-1].get("blockTime") or 0) < T0: break
print("create signatures in the window:", len(sigs), flush=True)
out = []
for k, x in enumerate(sigs):
    t = tri.tx(x["signature"])
    if not t: continue
    ks = PP.keys_of(t); meta = t.get("meta") or {}
    cr = None; trades = []
    for grp in meta.get("innerInstructions") or []:
        for ix in grp["instructions"]:
            if ks[ix["programIdIndex"]] != C.PUMP: continue
            raw = C.b58d(ix.get("data", ""))
            if raw[:8] != E.EVENT_IX_TAG: continue
            try:
                if raw[8:16] == E.DISC_CREATE:
                    e = E.decode_create(raw[8:]); cr = dict(mint=e.mint, creator=e.creator, user=e.user, name=e.name, symbol=e.symbol, uri=e.uri, mayhem=getattr(e, "is_mayhem_mode", getattr(e, "mayhem", None)))
                elif raw[8:16] == E.DISC_TRADE:
                    e = E.decode_trade(raw[8:]); trades.append((e.user, e.is_buy, e.sol_amount))
            except Exception: pass
    if not cr: continue
    a = PP.analyze(t)
    cr.update(sig=x["signature"], slot=t["slot"], idx=t.get("transactionIndex"), bt=t.get("blockTime"), signer=ks[0], nsig=t["transaction"]["message"]["header"]["numRequiredSignatures"],
              ix=a["pump_ix"], dev_sol=sum(s for u, b, s in trades if b) / 1e9, n_buys_in_tx=sum(1 for u, b, s in trades if b), tip=a["tip_total"])
    out.append(cr)
    if k % 200 == 0: print(time.strftime("%H:%M:%S"), k, "of", len(sigs), flush=True)
json.dump(out, open(f"{SP}/univ/creates_{T0}.json", "w"))
print("decoded creates:", len(out), flush=True)
