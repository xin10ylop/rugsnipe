# The curve after each of 6anjqMe's buys (candidate slices + a random baseline): the state at the end of each slot from
# his buy slot S to S + WIN. Signatures newest first (within a slot Agave lists them in reverse block order, so the first
# one of a slot is its last transaction), then getTransaction for the last transaction of each slot.
import json, sys, random, collections
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, "/home/user/rugsnipe/analysis/scripts/strategy"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, operators_refresh as orf
from rugsnipe_analysis import events as E
import common as C
W = "6anjqMeWigQFo5wpciARAunVQUXTqoat8w8fFWarCkpc"
TRIPS, MODE, OUTF, WIN = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
trips = json.load(open(SP + "/six/" + TRIPS))
C_ = [t for t in trips if t["closed"]]
random.seed(7)
if MODE == "size12":
    sel = {t["m"] for t in C_ if 1.0 <= t["stake"] < 2.0}
else:
    sel = {t["m"] for t in C_ if t["tip"] >= 8 or 1.0 <= t["stake"] < 2.0 or t["rep"] >= 1}
    sel |= {t["m"] for t in random.sample(C_, 200)}
# his buy slot per coin (from the rows)
rows = {}
for p in (SP + "/month/6anjqMeW.jsonl", SP + "/six/6anjqMeW_recent.jsonl"):
    for l in open(p):
        r = json.loads(l)
        if not r["err"]: rows[r["sig"]] = r
first = {}
for r in sorted(rows.values(), key=lambda r: r["s"]):
    for t in r["tr"]:
        if t[1] == W and t[2] and t[0] in sel and t[0] not in first:
            first[t[0]] = dict(slot=r["s"], sig=r["sig"], v=t[5], t=t[6])
print(len(sel), "coins,", len(first), "with his first buy", flush=True)
def state_of(tx, mint):
    meta = tx.get("meta") or {}; ks = None
    out = None
    for grp in meta.get("innerInstructions") or []:
        for ix in grp["instructions"]:
            raw = C.b58d(ix.get("data", ""))
            if raw[:8] == E.EVENT_IX_TAG and raw[8:16] == E.DISC_TRADE:
                try:
                    e = E.decode_trade(raw[8:])
                    if e.mint == mint: out = (e.virtual_sol_reserves, e.virtual_token_reserves)
                except Exception: pass
    return out
def one(m):
    b = first[m]; S = b["slot"]; curve = orf.bonding_curve(m)
    sigs, before = [], None
    for _ in range(12):
        o = {"limit": 1000, "until": b["sig"]}
        if before: o["before"] = before
        page = tri.rpc("getSignaturesForAddress", [curve, o]) or []
        sigs += page
        if len(page) < 1000 or page[-1]["slot"] <= S + WIN: break
        before = page[-1]["signature"]
    last = {}
    for x in sigs:   # newest first: the first one of a slot is its last transaction
        if S <= x["slot"] <= S + WIN and x.get("err") is None and x["slot"] not in last: last[x["slot"]] = x["signature"]
    st = {}
    for sl, sg in sorted(last.items()):
        tx = tri.tx(sg)
        s = state_of(tx, m) if tx else None
        if s: st[sl] = s
    return m, dict(S=S, his=(b["v"], b["t"]), states=st)
out = {}
with ThreadPoolExecutor(12) as ex:
    for k, (m, d) in enumerate(ex.map(one, list(first)), 1):
        out[m] = d
        if k % 50 == 0: print(k, "of", len(first), flush=True)
json.dump(out, open(SP + "/six/" + OUTF, "w"))
print("done", len(out), flush=True)
