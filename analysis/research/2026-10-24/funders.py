# For every launch the copied wallet attempted today (tapes, failed included): the create signer (dev wallet) and who
# funded it (the first incoming SOL transfer in its history). -> month/funders.json (resumable cache in rivalpnl/cache)
import sys, json, time, collections, os
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/rivalpnl")
import common as C, pumpparse as PP
C.MIN_GAP = 0.3
tapes = {}
for f in ("tapes.jsonl", "tapes2.jsonl"):
    for l in open(f"{SP}/today/{f}"):
        o = json.loads(l); tapes[o["mint"]] = o
devs = {}
for m, o in tapes.items():
    ctx = next((t for t in sorted(o["txs"], key=lambda t: (t["s"], t["i"] or 0)) if any(x.startswith("create") for x in t["ix"]) or any(e[0] == "C" for e in t["ev"])), None)
    if ctx: devs[m] = ctx["u"]
print("launches", len(tapes), "with a create", len(devs), "distinct dev wallets", len(set(devs.values())), flush=True)
OUT = SP + "/month/funders.json"
F = json.load(open(OUT)) if os.path.exists(OUT) else {}
k = 0
for d in sorted(set(devs.values())):
    if d in F: continue
    opt = {"transactionDetails": "full", "sortOrder": "asc", "limit": 8, "encoding": "json", "maxSupportedTransactionVersion": 1}
    try: r = C.rpc("getTransactionsForAddress", [d, opt]) or {}
    except Exception as e: print("rpc error", C._redact(e)); continue
    fund = None; first_bt = None; n_creates_seen = 0
    for tx in r.get("data") or []:
        first_bt = first_bt or tx.get("blockTime")
        a = PP.analyze(tx)
        if "create_mint" in a: n_creates_seen += 1
        for s, dst, lam, inner in a["sysxfers"]:
            if dst == d and s != d and fund is None and lam >= 1_000_000: fund = [s, lam, tx.get("blockTime")]
    F[d] = dict(funder=fund, first_bt=first_bt, creates_in_first8=n_creates_seen)
    k += 1
    if k % 50 == 0: json.dump(F, open(OUT, "w")); print(time.strftime("%H:%M:%S"), k, flush=True)
json.dump(F, open(OUT, "w"))
json.dump(devs, open(SP + "/month/devs_today.json", "w"))
print("done", k, flush=True)
