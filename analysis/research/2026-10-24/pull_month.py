# One wallet's transactions, newest first, back DAYS days, as compact rows -> month/<wallet[:8]>.jsonl (resumable).
#   python3 -I month/pull_month.py WALLET DAYS
import sys, os, json, time
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import common as C, pumpparse as PP
from rugsnipe_analysis import events as E
C.MIN_GAP = 0.3
W, DAYS = sys.argv[1], float(sys.argv[2])
OUT = f"{SP}/month/{W[:8]}.jsonl"; TOKF = OUT + ".tok"; DONEF = OUT + ".done"
STD = {"11111111111111111111111111111111", "ComputeBudget111111111111111111111111111111", C.PUMP,
       "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb", "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"}
if os.path.exists(DONEF): print("already done"); sys.exit(0)
T1 = int(time.time()); T0 = int(T1 - DAYS * 86400)
if os.path.exists(TOKF):
    st = json.load(open(TOKF)); tok, T0, T1 = st["tok"], st["T0"], st["T1"]
else: tok = None
def compact(tx):
    ks = PP.keys_of(tx); a = PP.analyze(tx); meta = tx.get("meta") or {}; m = tx["transaction"]["message"]
    i = ks.index(W) if W in ks else None
    trades = []
    for grp in meta.get("innerInstructions") or []:
        for ix in grp["instructions"]:
            if ks[ix["programIdIndex"]] != C.PUMP: continue
            raw = C.b58d(ix.get("data", ""))
            if raw[:8] == E.EVENT_IX_TAG and raw[8:16] == E.DISC_TRADE:
                try:
                    e = E.decode_trade(raw[8:])
                    trades.append([e.mint, e.user, int(e.is_buy), e.sol_amount, e.token_amount, e.virtual_sol_reserves, e.virtual_token_reserves,
                                   e.real_sol_reserves, e.creator, e.fee or 0, e.creator_fee or 0, e.ix_name])
                except Exception: pass
    calls = []
    for ix in m["instructions"]:
        p = ks[ix["programIdIndex"]]
        if p not in STD: calls.append([p, C.b58d(ix.get("data", "")).hex()[:96], len(ix.get("accounts", []))])
    xf = [[s, d, l] for s, d, l, inner in a["sysxfers"] if (s == W or d == W) and d not in PP.TIPS]
    nsig = m["header"]["numRequiredSignatures"]
    return dict(sig=tx["transaction"]["signatures"][0], s=tx["slot"], i=tx.get("transactionIndex"), bt=tx.get("blockTime"),
                err=(json.dumps(meta["err"])[:80] if meta.get("err") is not None else None), fee=meta.get("fee"), signers=ks[:nsig],
                d=(meta["postBalances"][i] - meta["preBalances"][i]) if i is not None else 0, pix=a["pump_ix"], tr=trades,
                tip=a["tip_total"], tips=a["tips"], cup=a["cu_price"], cul=a["cu_limit"], calls=calls, xf=xf,
                mints=[k for k in ks if k.endswith("pump")][:3])
f = open(OUT, "a"); pages = 0; n = 0
while True:
    opt = {"transactionDetails": "full", "sortOrder": "desc", "limit": 100, "encoding": "json", "maxSupportedTransactionVersion": 1,
           "filters": {"blockTime": {"gte": T0, "lt": T1}}}
    if tok: opt["paginationToken"] = tok
    try:
        r = C.rpc("getTransactionsForAddress", [W, opt], cache=False) or {}
    except Exception as e:
        print("rpc error", C._redact(e), flush=True); time.sleep(5); continue
    d = r.get("data") or []
    for tx in d:
        try: f.write(json.dumps(compact(tx), separators=(",", ":")) + "\n")
        except Exception as e: print("parse error", str(e)[:100], flush=True)
    f.flush(); n += len(d); pages += 1
    tok = r.get("paginationToken")
    json.dump(dict(tok=tok, T0=T0, T1=T1), open(TOKF, "w"))
    if pages % 20 == 0 and d: print(time.strftime("%H:%M:%S"), W[:8], "pages", pages, "txs", n, "at", time.strftime("%m-%d %H:%M", time.gmtime(d[-1].get("blockTime") or 0)), flush=True)
    if not tok or len(d) < 100: break
open(DONEF, "w").write("done"); print(W[:8], "done", n, "txs", pages, "pages", flush=True)
