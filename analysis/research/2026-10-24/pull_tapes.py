# Every tx (failed ones too) on the bonding curve of each mint either wallet landed today, from the create to the last
# sell of either wallet + 30 slots (create + 120 at most). -> today/tapes.jsonl (resumable)
import sys, os, json, time, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis"); sys.path.insert(0, SP + "/ourtrades")
import common as C, pumpparse as PP
C.MIN_GAP = 0.26
src = open(SP + "/ourtrades/pull.py").read().split("f = open(OUT")[0]
src = src.replace('L = json.load(open(SP + "/ourtrades/legs.json"))', 'L = []')
ns = {}; exec(compile(src, "pull_defs", "exec"), ns)
compact = ns["compact"]
def fetch(bc, mint, hi, maxp=12):
    rows = []; token = None; pages = 0; trunc = False
    while True:
        opt = {"transactionDetails": "full", "sortOrder": "asc", "limit": 100, "encoding": "json", "maxSupportedTransactionVersion": 1,
               "filters": {"slot": {"lte": hi}}}
        if token: opt["paginationToken"] = token
        try:
            r = C.rpc("getTransactionsForAddress", [bc, opt], cache=False)
        except Exception as e:
            print("rpc error", C._redact(e)); time.sleep(2); continue
        pages += 1
        dd = (r or {}).get("data") or []
        rows += [compact(tx, mint) for tx in dd]
        token = (r or {}).get("paginationToken")
        if not token or len(dd) < 100: break
        if pages >= maxp: trunc = True; break
    return rows, pages, trunc
legs = json.load(open(SP + "/today/legs_us.json")) + json.load(open(SP + "/today/legs_him.json"))
landed = {x["mint"] for x in legs if x["kind"] == "buy" and not x["err"] and x["mint"]}
last = collections.defaultdict(int); first = {}
for x in legs:
    if x["mint"] in landed and x["kind"] in ("buy", "sell"):
        last[x["mint"]] = max(last[x["mint"]], x["slot"]); first[x["mint"]] = min(first.get(x["mint"], 10**12), x["slot"])
OUT = SP + "/today/tapes.jsonl"; done = set()
if os.path.exists(OUT):
    for l in open(OUT):
        try: done.add(json.loads(l)["mint"])
        except Exception: pass
f = open(OUT, "a"); k = 0
todo = [m for m in landed if m not in done]
print("mints", len(landed), "to pull", len(todo), flush=True)
for m in todo:
    hi = min(last[m] + 30, first[m] + 120)
    rows, pages, trunc = fetch(C.bonding_curve(m), m, hi)
    f.write(json.dumps(dict(mint=m, hi=hi, pages=pages, trunc=trunc, txs=rows), separators=(",", ":")) + "\n"); f.flush()
    k += 1
    if k % 25 == 0: print(time.strftime("%H:%M:%S"), k, "of", len(todo), flush=True)
print("done", k, flush=True)
