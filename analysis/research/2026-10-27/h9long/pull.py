# A wallet's landed transactions from <w8>_sigs.json -> h9long/<w8>.jsonl. Each row: slot, block index, time, fee,
# the wallet's SOL change (native) and token changes by mint (wSOL included, so a swap through wSOL nets), the pump
# curve trades (TradeEvent, every user), the programs called (top level and inner), tips. Any venue: a PumpSwap or
# router swap shows as token deltas and its program ids. Raw transactions are not kept.
#   python3 -I pull.py WALLET
import sys, os, json, time
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, common as C, pumpparse as PP
from rugsnipe_analysis import events as E
W = sys.argv[1]
OUT = f"{SP}/h9long/{W[:8]}.jsonl"
sigs = [x for x in json.load(open(f"{SP}/h9long/{W[:8]}_sigs.json")) if not x[3]]
done = set()
if os.path.exists(OUT):
    for l in open(OUT):
        try: done.add(json.loads(l)["sig"])
        except Exception: pass
todo = [s for s, *_ in sigs if s not in done]
print(time.strftime("%H:%M:%S"), len(sigs), "landed,", len(todo), "to fetch", flush=True)
def row(tx):
    ks = PP.keys_of(tx); a = PP.analyze(tx); meta = tx.get("meta") or {}; m = tx["transaction"]["message"]
    i = ks.index(W) if W in ks else None
    tr = []
    progs = set()
    for ix in m["instructions"]: progs.add(ks[ix["programIdIndex"]])
    for grp in meta.get("innerInstructions") or []:
        for ix in grp["instructions"]:
            p = ks[ix["programIdIndex"]]; progs.add(p)
            if p != C.PUMP: continue
            raw = C.b58d(ix.get("data", ""))
            if raw[:8] == E.EVENT_IX_TAG and raw[8:16] == E.DISC_TRADE:
                try:
                    e = E.decode_trade(raw[8:])
                    tr.append([e.mint, e.user, int(e.is_buy), e.sol_amount, e.token_amount, e.virtual_sol_reserves,
                               e.virtual_token_reserves, e.real_sol_reserves, e.creator, e.fee or 0, e.creator_fee or 0, e.ix_name])
                except Exception: pass
    tok = {}
    for side, sgn in (("preTokenBalances", -1), ("postTokenBalances", 1)):
        for b in meta.get(side) or []:
            if b.get("owner") != W: continue
            mt = b["mint"]; amt = int(b["uiTokenAmount"]["amount"])
            t = tok.setdefault(mt, [0, b["uiTokenAmount"].get("decimals", 0), b.get("programId", "")])
            t[0] += sgn * amt
    tok = {k: v for k, v in tok.items() if v[0] != 0}
    return dict(sig=tx["transaction"]["signatures"][0], s=tx["slot"], i=tx.get("transactionIndex"), bt=tx.get("blockTime"),
                fee=meta.get("fee"), d=(meta["postBalances"][i] - meta["preBalances"][i]) if i is not None else 0,
                tok=tok, tr=tr, progs=sorted(progs - {"11111111111111111111111111111111", "ComputeBudget111111111111111111111111111111"}),
                tip=a["tip_total"], cup=a["cu_price"], signer=ks[0] == W)
def one(s):
    for k in range(3):
        try:
            tx = tri.rpc("getTransaction", [s, {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}], cache=False)
            return json.dumps(row(tx), separators=(",", ":")) if tx else None
        except Exception as e:
            err = e
    return None
n = 0
with ThreadPoolExecutor(10) as ex, open(OUT, "a") as f:
    for line in ex.map(one, todo):
        n += 1
        if line: f.write(line + "\n")
        if n % 2000 == 0: print(time.strftime("%H:%M:%S"), n, flush=True); f.flush()
print(time.strftime("%H:%M:%S"), "done", flush=True)
