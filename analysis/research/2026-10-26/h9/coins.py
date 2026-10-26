# Wallets 4 / 6: every coin whose first trade of theirs is a buy (Oct 25 18:00 UTC -> now); its create (class as the bot
# reads it: create_v2, not mayhem, not cashback, priced in SOL) and, for the in-class ones bought at create + 0/1, every
# landed trade from the create to the wallet's last sell + 3 slots (at least create + 40, at most create + 160).
#   python3 -I coins.py WALLET  ->  h9/coin/<mint>.json, h9/<w8>_coins.json
import sys, json, os, struct, time
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP, common as C
from rugsnipe_analysis import events as E
tri.MIN_GAP = 0.012
W = sys.argv[1]; OUT = SP + "/h9"
TX = {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}
def log(*a): print(time.strftime("%H:%M:%S"), *a, flush=True)
# the wallet's trades per coin
own = {}
for l in open(f"{OUT}/{W[:8]}.jsonl"):
    r = json.loads(l)
    for t in r["tr"]:
        if t[1] != W: continue
        own.setdefault(t[0], []).append(dict(s=r["s"], i=r["i"], bt=r["bt"], buy=t[2], sol=t[3], tok=t[4], vs=t[5], vt=t[6], d=r["d"], tip=r["tip"]))
coins = {}
for m, ts in own.items():
    ts.sort(key=lambda x: (x["s"], x["i"] or 0))
    if ts[0]["buy"]: coins[m] = ts
log(W[:8], len(own), "coins traded,", len(coins), "first trade a buy")
def rstr(b, o):
    n = struct.unpack_from("<I", b, o)[0]; return b[o + 4:o + 4 + n], o + 4 + n
def one(m):
    p = f"{OUT}/coin/{m}.json"
    if os.path.exists(p): return json.load(open(p))
    ts = coins[m]
    bc = C.bonding_curve(m); allsig = []; before = None
    for _ in range(80):
        o = {"limit": 1000}
        if before: o["before"] = before
        s = tri.rpc("getSignaturesForAddress", [bc, o], cache=False) or []
        allsig += s
        if len(s) < 1000: break
        before = s[-1]["signature"]
    out = dict(mint=m, w=W)
    if not allsig:
        out["why"] = "no bonding curve"; json.dump(out, open(p, "w")); return out
    cs = min(x["slot"] for x in allsig); out["cs"] = cs
    # the create slot also holds the dev's bundle buys (listed newest first): look through it for the create
    create = None
    for x in reversed([x for x in allsig if x["slot"] == cs and not x.get("err")]):
        ct = tri.rpc("getTransaction", [x["signature"], TX], cache=False)
        if not ct: continue
        ks = PP.keys_of(ct)
        for prog, accs, data, parsed, oi, inner in PP._ixs(ct, ks):
            if prog == C.PUMP and data and data[:8] in PP.IXR and PP.IXR[data[:8]].startswith("create") and accs and accs[0] == m:
                o = 8; name, o = rstr(data, o); sym, o = rstr(data, o); uri, o = rstr(data, o); tail = data[o + 32:]
                create = dict(ix=PP.IXR[data[:8]], tail=tail.hex(), idx=ct.get("transactionIndex") or 0, signer=ks[0])
                break
        if create: break
    out["create"] = create
    at = ts[0]["s"] - cs; out["at"] = at
    v2 = bool(create) and create["ix"] == "create_v2"
    tail = bytes.fromhex(create["tail"]) if create else b""
    mayhem = len(tail) >= 1 and tail[0] == 1; cashback = len(tail) >= 2 and tail[1] == 1
    sol_q = ts[0]["vs"] > 0
    out["cls"] = dict(v2=v2, mayhem=mayhem, cashback=cashback, sol=sol_q)
    out["in_class"] = v2 and not mayhem and not cashback and sol_q
    if not out["in_class"] or at > 1:
        json.dump(out, open(p, "w")); return out
    sells = [x["s"] for x in ts if not x["buy"]]
    end = min(cs + 160, max(cs + 40, (max(sells) + 3) if sells else cs + 40)); out["end"] = end
    rows = []
    want = [x for x in allsig if x["slot"] <= end and not x.get("err")]
    def tx(x):
        try: return tri.rpc("getTransaction", [x["signature"], TX], cache=False)
        except Exception: return None
    for t in map(tx, want):
        if not t: continue
        ks = PP.keys_of(t); idx = t.get("transactionIndex") or 0
        for prog, accs, data, parsed, oi, inner in PP._ixs(t, ks):
            if prog != C.PUMP or not data: continue
            if data[:8] == E.EVENT_IX_TAG and data[8:16] == E.DISC_TRADE:
                try: e = E.decode_trade(data[8:])
                except Exception: continue
                if e.mint == m:
                    rows.append([t["slot"], idx, e.user, int(e.is_buy), e.sol_amount, e.token_amount, e.virtual_sol_reserves, e.virtual_token_reserves])
    rows.sort(key=lambda r: (r[0], r[1]))
    out["rows"] = rows; out["complete"] = len(want) == sum(1 for _ in want)
    json.dump(out, open(p + ".tmp", "w")); os.replace(p + ".tmp", p)
    return out
res = {}
with ThreadPoolExecutor(10) as ex:
    for i, o in enumerate(ex.map(one, list(coins)), 1):
        res[o["mint"]] = {k: v for k, v in o.items() if k != "rows"}
        if i % 50 == 0: log(W[:8], i, "/", len(coins))
json.dump(dict(own=coins, coins=res), open(f"{OUT}/{W[:8]}_coins.json", "w"))
n_in = sum(1 for o in res.values() if o.get("in_class") and o.get("at", 9) <= 1)
log(W[:8], "done:", len(res), "coins,", n_in, "in class at create + 0/1")
