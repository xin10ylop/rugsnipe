# Last night (Oct 25 00:00-09:13 UTC) through the follow rule, ex ante. The wallet's attempts from Oct 24 12:00 (12 h of
# warm-up, as in the tests), every coin it attempted and every coin of the night's reconciliation resolved from chain
# (create, bundle wallets; the night's coins with a 12-slot tape), each bundle wallet's launches from its own history.
#   python3 -I night.py   -> dec/night/{att.json, coins/<mint>.json, wallets.json}
import sys, json, os, time, collections, threading
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP, common as C
from rugsnipe_analysis import events as E
tri.MIN_GAP = 0.015
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
TW, T0, T1 = 1792843200, 1792886400, 1792919600
OUT = SP + "/dec/night"; os.makedirs(OUT + "/coins", exist_ok=True)
def log(*a): print(time.strftime("%H:%M:%S"), *a, flush=True)
def sigs_range(addr, lo, hi, max_pages=40):
    out = []; before = None; capped = False
    for page in range(max_pages):
        o = {"limit": 1000}
        if before: o["before"] = before
        s = tri.rpc("getSignaturesForAddress", [addr, o], cache=False) or []
        out += [x for x in s if lo <= (x.get("blockTime") or 0) < hi]
        if len(s) < 1000 or (s[-1].get("blockTime") or 0) < lo: break
        before = s[-1]["signature"]
    else:
        capped = True
    return out, capped
# 1. his attempts
P = OUT + "/att.json"
if os.path.exists(P): att = json.load(open(P))
else:
    ss, _ = sigs_range(A, TW, T1, 100)
    log("his signatures in the window", len(ss))
    att = []
    def one_att(x):
        t = tri.tx(x["signature"])
        if not t: return None
        ks = PP.keys_of(t)
        if ks[0] != A: return None
        m = None; buy = False
        for prog, accs, data, parsed, oi, inner in PP._ixs(t, ks):
            if prog == C.PUMP and not inner and data[:8] in PP.IXR and PP.IXR[data[:8]].startswith("buy"):
                buy = True
                if len(accs) > 2: m = accs[2]
        if not buy: return None
        if not m or not m.endswith("pump"):
            mm = [k for k in ks if k.endswith("pump")]
            m = mm[0] if mm else m
        return dict(mint=m, slot=t["slot"], bt=t.get("blockTime"), failed=(t.get("meta") or {}).get("err") is not None)
    with ThreadPoolExecutor(8) as ex:
        for r in ex.map(one_att, ss):
            if r and r["mint"]: att.append(r)
    json.dump(att, open(P, "w"))
log("his buy transactions", len(att), "mints", len({a["mint"] for a in att}))
# 2. coins
R = [r for r in json.load(open(SP + "/live/recon.json"))["res"] if "note" not in r]
night = {r["mint"] for r in R}
mints = sorted({a["mint"] for a in att} | night)
def decode(t, m):
    ks = PP.keys_of(t); ev = []; cre = None
    for prog, accs, data, parsed, oi, inner in PP._ixs(t, ks):
        if prog != C.PUMP or not data: continue
        if data[:8] == E.EVENT_IX_TAG and data[8:16] == E.DISC_TRADE:
            try:
                e = E.decode_trade(data[8:])
                if e.mint == m: ev.append([e.user, int(e.is_buy), e.sol_amount, e.token_amount, e.virtual_sol_reserves, e.virtual_token_reserves, (e.fee or 0) + (e.creator_fee or 0), e.creator])
            except Exception: pass
        elif not inner and data[:8] in PP.IXR and PP.IXR[data[:8]].startswith("create") and accs and accs[0] == m:
            cre = True
    return [t["slot"], t.get("transactionIndex") or 0, ks[0], ev, bool(cre)]
def coin(m):
    p = f"{OUT}/coins/{m}.json"
    if os.path.exists(p): return "have"
    bc = C.bonding_curve(m); allsig = []; before = None; reached = False
    for page in range(300):
        o = {"limit": 1000}
        if before: o["before"] = before
        s = tri.rpc("getSignaturesForAddress", [bc, o], cache=False) or []
        allsig += s
        if len(s) < 1000: reached = True; break
        before = s[-1]["signature"]
    if not allsig: return "empty"
    cs = min(x["slot"] for x in allsig)
    span = 12 if m in night else 0
    keep = [x for x in allsig if x["slot"] <= cs + span and x.get("err") is None]
    rows = []
    for x in keep:
        t = tri.tx(x["signature"])
        if t: rows.append(decode(t, m))
    rows.sort(key=lambda r: (r[0], r[1]))
    cr = next((r for r in rows if r[4]), None)
    c = None
    if cr:
        evs = cr[3]
        c = dict(slot=cr[0], idx=cr[1], signer=cr[2], user=evs[0][0] if evs else cr[2], creator=evs[0][7] if evs else cr[2])
    json.dump(dict(mint=m, cs=cs, reached=reached, create=c, rows=[r[:4] for r in rows]), open(p + ".tmp", "w")); os.replace(p + ".tmp", p)
    return "ok" if c else "no create"
cnt = collections.Counter()
def safe_coin(m):
    try: cnt[coin(m)] += 1
    except Exception as e: cnt["error"] += 1; log("error", m[:8], tri.red(e)[:100])
    s = sum(cnt.values())
    if s % 200 == 0: log("coins", s, "of", len(mints), dict(cnt))
with ThreadPoolExecutor(8) as ex: list(ex.map(safe_coin, mints))
log("coins done", dict(cnt))
# 3. bundle wallets
def bundle(m):
    p = f"{OUT}/coins/{m}.json"
    if not os.path.exists(p): return None, None
    t = json.load(open(p)); c = t["create"]
    if not c: return None, None
    cs, ci = c["slot"], c["idx"]; cset = {c["creator"], c["user"], c["signer"]}; prev = ci; bw = []
    for s, i, sg, evs in t["rows"]:
        for (u, b, sol, tok, vs, vt, f, *_) in evs:
            if (s, i) <= (cs, ci): continue
            if s == cs and u in cset and (i - prev) <= 2: prev = max(prev, i); continue
            if s == cs and b and u not in (A, US) and (i - prev) <= 2 and (i - ci) <= 5: prev = i; bw.append(u)
    return c, sorted(set(bw))
BWS = {}
for m in mints:
    c, bw = bundle(m)
    if c: BWS[m] = dict(cs=c["slot"], bw=bw)
json.dump(BWS, open(OUT + "/bundles.json", "w"))
# 4. each night coin's bundle wallets: their launches (clusters of their own transactions) in the window
W = sorted({w for m in night if m in BWS for w in BWS[m]["bw"]})
log("night bundle wallets", len(W))
WP = OUT + "/wallets.json"
WL = json.load(open(WP)) if os.path.exists(WP) else {}
lock = threading.Lock()
def wal(w):
    if w in WL: return
    s, capped = sigs_range(w, TW, T1, 20)
    slots = sorted({x["slot"] for x in s}); launches = []
    for sl in slots:
        if not launches or sl - launches[-1][-1] > 40: launches.append([sl])
        else: launches[-1].append(sl)
    with lock: WL[w] = dict(capped=capped, n_sigs=len(s), launches=[l[0] for l in launches])
with ThreadPoolExecutor(8) as ex: list(ex.map(wal, W))
json.dump(WL, open(WP, "w"))
log("done; wallets", len(WL), "capped", sum(1 for v in WL.values() if v["capped"]))
