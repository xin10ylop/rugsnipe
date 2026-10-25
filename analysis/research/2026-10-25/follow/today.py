# Today (Oct 25 09:13 UTC -> now), live, through the keyed follow started from data/follow_seed.json: every coin the
# followed records' wallets bundled and every coin he attempted, resolved from chain (create, bundle wallets, fee routing,
# 12-slot tape); ex ante in slot order (seed, then each coin's run and his attempt update the counts as the bot would).
import sys, json, os, time, collections, threading
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP, common as C
from rugsnipe_analysis import events as E
tri.MIN_GAP = 0.015
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
T0 = 1792919600; T1 = int(sys.argv[1]) if len(sys.argv) > 1 else int(time.time()) - 120
OUT = SP + "/dec/today"
def log(*a): print(time.strftime("%H:%M:%S"), *a, flush=True)
def sigs_range(addr, lo, hi, max_pages=40):
    out = []; before = None
    for page in range(max_pages):
        o = {"limit": 1000}
        if before: o["before"] = before
        s = tri.rpc("getSignaturesForAddress", [addr, o], cache=False) or []
        out += [x for x in s if lo <= (x.get("blockTime") or 0) < hi]
        if len(s) < 1000 or (s[-1].get("blockTime") or 0) < lo: break
        before = s[-1]["signature"]
    return out
def trades(t, who=None, mint=None):
    out = []
    ks = PP.keys_of(t)
    for prog, accs, data, parsed, oi, inner in PP._ixs(t, ks):
        if prog == C.PUMP and data and data[:8] == E.EVENT_IX_TAG and data[8:16] == E.DISC_TRADE:
            try:
                e = E.decode_trade(data[8:])
                if (who is None or e.user == who) and (mint is None or e.mint == mint): out.append(e)
            except Exception: pass
    return out
# 1. his transactions in the window: attempts (buys, failed too) and his P&L per mint
P = OUT + "/him.json"
if os.path.exists(P) and json.load(open(P)).get("T1") == T1: him = json.load(open(P))["him"]
else:
    ss = sigs_range(A, T0, T1, 100); log("his signatures", len(ss))
    him = {}; lock = threading.Lock()
    def one(x):
        t = tri.tx(x["signature"])
        if not t: return
        ks = PP.keys_of(t)
        if ks[0] != A: return
        fail = (t.get("meta") or {}).get("err") is not None
        m = None; buy = False
        for prog, accs, data, parsed, oi, inner in PP._ixs(t, ks):
            if prog == C.PUMP and not inner and data[:8] in PP.IXR and PP.IXR[data[:8]].startswith("buy"):
                buy = True; m = accs[2] if len(accs) > 2 else m
        evs = trades(t, A)
        if not m and evs: m = evs[0].mint
        if not m: return
        with lock:
            h = him.setdefault(m, dict(att=0, slot=None, spent=0, got=0))
            if buy:
                h["att"] += 1; h["slot"] = min(h["slot"] or t["slot"], t["slot"])
            for e in evs:
                if e.is_buy: h["spent"] += e.sol_amount + (e.fee or 0) + (e.creator_fee or 0)
                else: h["got"] += e.sol_amount - (e.fee or 0) - (e.creator_fee or 0)
    with ThreadPoolExecutor(8) as ex: list(ex.map(one, ss))
    json.dump(dict(T1=T1, him=him), open(P, "w"))
att = {m: h for m, h in him.items() if h["att"]}
log("his attempted mints", len(att))
# 2. the seed's followed wallets: their buys in the window -> candidate launches
seed = json.load(open("/home/user/rugsnipe/data/follow_seed.json"))
fw = sorted({k.split(":")[0] for k, v in seed.items() if v[1] >= 2 and v[1] >= 0.3 * v[0]})
P = OUT + "/wbuys.json"
wb = json.load(open(P)) if os.path.exists(P) else {}
if wb.get("_T1") != T1:
    wb = {"_T1": T1}; lock = threading.Lock()
    def onew(w):
        first = {}
        for x in sigs_range(w, T0, T1, 10):
            if x.get("err"): continue
            t = tri.tx(x["signature"])
            if not t: continue
            for e in trades(t, w):
                if e.is_buy and (e.mint not in first or t["slot"] < first[e.mint][0]):
                    first[e.mint] = (t["slot"], (e.virtual_sol_reserves - e.sol_amount) / 1e9)
        with lock: wb[w] = sorted([v[0], m, round(v[1], 2)] for m, v in first.items())
    def safew(w):
        try: onew(w)
        except Exception as e: log("error", w[:8], tri.red(e)[:80])
    with ThreadPoolExecutor(8) as ex: list(ex.map(safew, fw))
    json.dump(wb, open(P, "w"))
cand = {m for w, v in wb.items() if w != "_T1" for s, m, vb in v if vb < 45}
log("followed wallets", len(fw), "candidate launches", len(cand))
# 3. coins: create, create-slot run, 12-slot tape
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
    for page in range(60):
        o = {"limit": 1000}
        if before: o["before"] = before
        s = tri.rpc("getSignaturesForAddress", [bc, o], cache=False) or []
        allsig += s
        if len(s) < 1000: reached = True; break
        before = s[-1]["signature"]
    if not allsig or not reached: json.dump(dict(mint=m, reached=False), open(p, "w")); return "unreached"
    cs = min(x["slot"] for x in allsig)
    rows = [decode(t, m) for t in (tri.tx(x["signature"]) for x in allsig if x["slot"] <= cs + 12 and x.get("err") is None) if t]
    rows.sort(key=lambda r: (r[0], r[1]))
    cr = next((r for r in rows if r[4]), None)
    c = None
    if cr:
        evs = cr[3]; c = dict(slot=cr[0], idx=cr[1], signer=cr[2], user=evs[0][0] if evs else cr[2], creator=evs[0][7] if evs else cr[2])
    json.dump(dict(mint=m, cs=cs, reached=True, create=c, rows=[r[:4] for r in rows]), open(p + ".tmp", "w")); os.replace(p + ".tmp", p)
    return "ok"
mints = sorted(cand | set(att)); cnt = collections.Counter()
def safec(m):
    try: cnt[coin(m)] += 1
    except Exception as e: cnt["error"] += 1
with ThreadPoolExecutor(8) as ex: list(ex.map(safec, mints))
log("coins", dict(cnt))
# 4. ex ante, in slot order
def run_of(m):
    p = f"{OUT}/coins/{m}.json"
    if not os.path.exists(p): return None
    t = json.load(open(p)); c = t.get("create")
    if not c: return None
    cs, ci = c["slot"], c["idx"]; cset = {c["creator"], c["user"], c["signer"]}; prev = ci; bw = []; last = None
    for s, i, sg, evs in t["rows"]:
        for e in evs:
            u, b = e[0], e[1]
            if (s, i) <= (cs, ci): continue
            if s == cs and u in cset and (i - prev) <= 2: prev = max(prev, i); last = e; continue
            if s == cs and b and u not in (A, US) and (i - prev) <= 2 and (i - ci) <= 5:
                prev = i; last = e
                if u not in bw: bw.append(u)
    ev = [(s, i, e) for s, i, sg, evs in t["rows"] for e in evs if e[4] > 0 and e[5] > 0]
    val = {}
    if last and ev:
        p0 = last[4] / last[5]
        for k in (1, 3, 6, 10):
            xs = [e for s, i, e in ev if s <= cs + k]
            val[k] = xs[-1][4] / xs[-1][5] / p0 - 1 if xs else None
    return dict(cs=cs, dev=c["creator"] in (c["signer"], c["user"]), bw=bw, pb=(last[4] / 1e9 if last else None), val=val)
runs = {m: r for m in mints for r in [run_of(m)] if r and r["bw"]}
rec = {k: list(v[:2]) for k, v in seed.items()}
def followed(r, K=2, R=0.3):
    f = "dev" if r["dev"] else "third"
    return any((x := rec.get(f"{w}:{f}")) and x[1] >= K and x[1] >= R * x[0] for w in r["bw"])
res = collections.defaultdict(lambda: dict(n=0, he=0, pnl=0.0, v=[]))
for m, r in sorted(runs.items(), key=lambda kv: kv[1]["cs"]):
    if r["cs"] < 461201998: continue
    fol = followed(r); he = m in att
    g = res[("followed" if fol else "not followed", "pb<=40" if (r["pb"] or 99) <= 40 else "pb>40")]
    g["n"] += 1; g["he"] += he
    if he and att[m]["got"]: g["pnl"] += (att[m]["got"] - att[m]["spent"]) / 1e9
    if r["val"].get(3) is not None: g["v"].append(r["val"][3])
    f = "dev" if r["dev"] else "third"
    for w in r["bw"]:
        x = rec.setdefault(f"{w}:{f}", [0, 0]); x[0] += 1; x[1] += he
tot_att = len(att); in_runs = sum(1 for m in att if m in runs); fol_att = 0
print(f"\nwindow {time.strftime('%H:%M', time.gmtime(T0))}-{time.strftime('%H:%M', time.gmtime(T1))} UTC: his attempted coins {tot_att}, bundled {in_runs}")
for k in sorted(res):
    g = res[k]; v = g["v"]
    print(f"   {k[0]:13s} {k[1]:7s} coins {g['n']:4d} he attempted {g['he']:4d} ({100*g['he']/max(1,g['n']):4.1f}%) his P&L {g['pnl']:+.3f} SOL  value at +3 (behind the bundle, gross) {100*sum(v)/max(1,len(v)):+5.1f}% n {len(v)}")
hp = sum((h["got"] - h["spent"]) / 1e9 for h in att.values() if h["got"])
print(f"   his P&L in the window on coins he sold: {hp:+.3f} SOL")
