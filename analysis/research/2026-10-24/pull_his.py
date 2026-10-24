# Tapes (failed txs included) for every launch the copied wallet ATTEMPTED today that has no tape yet, create -> his
# attempt + 45 slots; and, for every tape, the create's name / symbol / uri. -> today/tapes2.jsonl, today/meta.json
import sys, os, json, time, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis"); sys.path.insert(0, SP + "/ourtrades")
import common as C, pumpparse as PP
from rugsnipe_analysis import events as E
C.MIN_GAP = 0.26
src = open(SP + "/ourtrades/pull.py").read().split("f = open(OUT")[0].replace('L = json.load(open(SP + "/ourtrades/legs.json"))', 'L = []')
ns = {}; exec(compile(src, "pull_defs", "exec"), ns); compact = ns["compact"]
def fetch(bc, mint, hi, maxp=6):
    rows = []; token = None; pages = 0; trunc = False
    while True:
        opt = {"transactionDetails": "full", "sortOrder": "asc", "limit": 100, "encoding": "json", "maxSupportedTransactionVersion": 1, "filters": {"slot": {"lte": hi}}}
        if token: opt["paginationToken"] = token
        try: r = C.rpc("getTransactionsForAddress", [bc, opt], cache=False)
        except Exception as e: print("rpc error", C._redact(e)); time.sleep(2); continue
        pages += 1; dd = (r or {}).get("data") or []
        for tx in dd:
            row = compact(tx, mint)
            ks = PP.keys_of(tx)
            for grp in (tx.get("meta") or {}).get("innerInstructions") or []:
                for ix in grp["instructions"]:
                    if ks[ix["programIdIndex"]] != C.PUMP: continue
                    raw = C.b58d(ix.get("data", ""))
                    if raw[:8] == E.EVENT_IX_TAG and raw[8:16] == E.DISC_CREATE:
                        try:
                            e = E.decode_create(raw[8:]); row["meta"] = [e.name, e.symbol, e.uri]
                        except Exception: pass
            rows.append(row)
        token = (r or {}).get("paginationToken")
        if not token or len(dd) < 100: break
        if pages >= maxp: trunc = True; break
    return rows, pages, trunc
LH = json.load(open(SP + "/today/legs_him.json"))
att = {}
for x in LH:
    if x["kind"] == "buy" and x["mint"] and x["mint"] not in att: att[x["mint"]] = x["slot"]
have = set()
for f in ("tapes.jsonl", "tapes2.jsonl"):
    p = SP + "/today/" + f
    if os.path.exists(p):
        for l in open(p):
            try: have.add(json.loads(l)["mint"])
            except Exception: pass
todo = [m for m in att if m not in have]
print("attempted", len(att), "have tapes", len(have & set(att)), "to pull", len(todo), flush=True)
f = open(SP + "/today/tapes2.jsonl", "a"); k = 0
for m in todo:
    rows, pages, trunc = fetch(C.bonding_curve(m), m, att[m] + 45)
    f.write(json.dumps(dict(mint=m, hi=att[m] + 45, pages=pages, trunc=trunc, txs=rows), separators=(",", ":")) + "\n"); f.flush()
    k += 1
    if k % 50 == 0: print(time.strftime("%H:%M:%S"), k, "of", len(todo), flush=True)
print("done", k, flush=True)
