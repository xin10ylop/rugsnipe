# The full pre-buy state of every bundled launch of last night's operators (Oct 24 12:00 - Oct 25 09:13), taken or
# skipped: the create transaction (compute price / limit, tip and route, programs, instruction count, version, lookup
# tables, name / symbol / uri), the bundle transactions (compute price, tips, signers), the dev wallet (age, earlier
# transactions, funder, funding amount and slots before the create), the slot leader, and his own context (his attempts
# in the 50 slots before). -> cell/prebuy.json {mint: {...}}
import sys, json, os, time, collections, threading, struct
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP, common as C
tri.MIN_GAP = 0.015
N = SP + "/dec/night"; OUT = SP + "/dec/cell/prebuy.json"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"
TW, T1 = 1792843200, 1792919600
def log(*a): print(time.strftime("%H:%M:%S"), *a, flush=True)
def bt_of(slot): return 1792919568 + (slot - 461201998) * 0.2121
def sigs_range(addr, lo, hi, max_pages=6):
    out = []; before = None; capped = True
    for page in range(max_pages):
        o = {"limit": 1000}
        if before: o["before"] = before
        s = tri.rpc("getSignaturesForAddress", [addr, o], cache=False) or []
        out += [x for x in s if lo <= (x.get("blockTime") or 0) < hi]
        if len(s) < 1000 or (s[-1].get("blockTime") or 0) < lo: capped = False; break
        before = s[-1]["signature"]
    return out, capped
L = json.load(open(SP + "/dec/cell/launch.json")); ops = json.load(open(N + "/ops.json"))
L = [r for r in L if r.get("cs")]
# his attempt slots (context)
att = json.load(open(N + "/att.json")); his_slots = sorted(a["slot"] for a in att)
import bisect
# operator wallets' transactions in the window, by slot
W = sorted({w for r in L for w in ops[r["op"]][r["mint"]]["wallets"]})
WS = {}; lock = threading.Lock()
def wsigs(w):
    s, cap = sigs_range(w, TW, T1, 12)
    by = collections.defaultdict(list)
    for x in s:
        if x.get("err") is None: by[x["slot"]].append(x["signature"])
    with lock: WS[w] = by
with ThreadPoolExecutor(8) as ex: list(ex.map(wsigs, W))
log("operator wallets", len(W))
def rstr(b, o):
    n = struct.unpack_from("<I", b, o)[0]; return b[o + 4:o + 4 + n].decode("utf-8", "replace"), o + 4 + n
def create_info(t, mint):
    ks = PP.keys_of(t); out = dict(progs=sorted({p for p, *_ in PP._ixs(t, ks) if True})[:12])
    msg = t["transaction"]["message"]
    out["n_ix"] = len(msg.get("instructions", [])); out["n_sig"] = len(t["transaction"]["signatures"])
    out["alts"] = len(msg.get("addressTableLookups") or []); out["v0"] = t.get("version") == 0
    for prog, accs, data, parsed, oi, inner in PP._ixs(t, ks):
        if prog == C.PUMP and not inner and data[:8] in PP.IXR and PP.IXR[data[:8]].startswith("create") and accs and accs[0] == mint:
            out["ix"] = PP.IXR[data[:8]]
            try:
                nm, o = rstr(data, 8); sy, o = rstr(data, o); uri, o = rstr(data, o)
                out.update(name=nm, sym=sy, uri=uri)
            except Exception: pass
    a = PP.analyze(t); out.update(cu_price=a["cu_price"], cu_limit=a["cu_limit"], tips=a["tips"], tip=a["tip_total"], fee=a["fee"], pump_ix=a["pump_ix"])
    return out
S = json.load(open(OUT)) if os.path.exists(OUT) else {}
def one(r):
    m = r["mint"]
    if m in S: return
    t = json.load(open(f"{N}/coins/{m}.json")); c = t["create"]; cs = t["cs"]
    d = dict(op=r["op"], dev_fees=r.get("dev_fees"), he=r["he"], cs=cs, idx=c["idx"], signer=c["signer"], user=c["user"], creator=c["creator"])
    # create tx and the dev wallet's history from its own signatures
    s, cap = sigs_range(c["signer"], 0, 10**10, 3)
    d["dev_n_sigs"] = len(s); d["dev_capped"] = cap
    cre = [x for x in s if x["slot"] == cs]
    prior = [x for x in s if x["slot"] < cs]
    d["dev_prior_txs"] = len(prior) if not cap else None
    if prior and not cap: d["dev_age_slots"] = cs - min(x["slot"] for x in prior)
    for x in cre:
        tx = tri.tx(x["signature"])
        if tx and PP.analyze(tx).get("create_mint") == m:
            d["create"] = create_info(tx, m); d["create_sig"] = x["signature"]; break
    # the funding: SOL into the signer in its last 3 transactions before the create
    fund = []
    for x in sorted(prior, key=lambda x: -x["slot"])[:3]:
        tx = tri.tx(x["signature"])
        if not tx: continue
        for src, dst, lam, inner in PP.analyze(tx)["sysxfers"]:
            if dst == c["signer"] and lam > 0: fund.append([src, lam / 1e9, cs - x["slot"]])
    d["fund"] = fund
    # bundle transactions: the operator wallets' transactions in the create slot
    bt = []
    for w in ops[r["op"]][m]["wallets"]:
        for sg in WS.get(w, {}).get(cs, []):
            tx = tri.tx(sg)
            if not tx: continue
            a = PP.analyze(tx)
            if not any(e.get("mint") == m for e in a["trades"]): continue
            bt.append(dict(w=w, idx=tx.get("transactionIndex"), cu_price=a["cu_price"], cu_limit=a["cu_limit"], tip=a["tip_total"], tips=a["tips"],
                           n_sig=len(tx["transaction"]["signatures"]), signer=PP.keys_of(tx)[0], same_as_create=sg == d.get("create_sig")))
    d["bundle_txs"] = bt
    i = bisect.bisect_left(his_slots, cs - 50); j = bisect.bisect_left(his_slots, cs)
    d["his_prev50"] = j - i
    with lock: S[m] = d
n = [0]
def safe(r):
    try: one(r)
    except Exception as e: log("error", r["mint"][:8], tri.red(e)[:100])
    n[0] += 1
    if n[0] % 200 == 0:
        log("launches", n[0], "of", len(L))
        with lock: json.dump(S, open(OUT + ".tmp", "w")); os.replace(OUT + ".tmp", OUT)
with ThreadPoolExecutor(8) as ex: list(ex.map(safe, L))
json.dump(S, open(OUT, "w")); log("prebuy done", len(S))
