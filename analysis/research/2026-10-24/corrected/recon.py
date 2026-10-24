# Every coin we or the copied wallet tried since T0, from chain: the create (tool, dev buy, post-bundle SOL, signer),
# the tweet (age, rank among all coins on it since T0 - 1 h), who took the first spot and what the first outside buyer
# made at +10 slots (the backtest's value), our result, his result. Writes recon.json and prints a table.
import sys, json, re, time, urllib.request, collections
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP, common as C
from rugsnipe_analysis import events as E
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
MA = "TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM"
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
FEE = 0.0125
T0, T1 = int(sys.argv[1]), int(sys.argv[2])
def log(*a): print(time.strftime("%H:%M:%S"), *a, file=sys.stderr, flush=True)
def sigs_since(addr, since, tries=1):
    seen = {}; before = None
    for page in range(60):
        o = {"limit": 1000, "commitment": "confirmed"}
        if before: o["before"] = before
        got = [tri.rpc("getSignaturesForAddress", [addr, o], cache=False) or [] for _ in range(tries if not before else 1)]
        s = max(got, key=lambda p: p[0]["slot"] if p else -1)
        for x in s:
            if since <= (x.get("blockTime") or 0) < T1: seen[x["signature"]] = x
        if len(s) < 1000 or (s[-1].get("blockTime") or 0) < since: break
        before = s[-1]["signature"]
    return seen
def decode(t):
    ks = PP.keys_of(t); meta = t.get("meta") or {}; tr = []; cr = None; mint_ix = None
    for prog, accs, data, parsed, oi, inner in PP._ixs(t, ks):
        if prog != C.PUMP or not data: continue
        if data[:8] == E.EVENT_IX_TAG:
            try:
                if data[8:16] == E.DISC_TRADE:
                    e = E.decode_trade(data[8:])
                    tr.append(dict(mint=e.mint, u=e.user, b=e.is_buy, sol=e.sol_amount, tok=e.token_amount, vs=e.virtual_sol_reserves,
                                   vt=e.virtual_token_reserves, f=(e.fee or 0) + (e.creator_fee or 0)))
                elif data[8:16] == E.DISC_CREATE:
                    e = E.decode_create(data[8:]); cr = dict(mint=e.mint, creator=e.creator, user=e.user, uri=e.uri, name=e.name, sym=e.symbol)
            except Exception: pass
        elif mint_ix is None and len(accs) >= 3 and not inner: mint_ix = accs[2]
    a = PP.analyze(t)
    return dict(slot=t["slot"], i=t.get("transactionIndex") or 0, bt=t.get("blockTime"), signer=ks[0], err=meta.get("err") is not None,
                errv=meta.get("err"), tr=tr, cr=cr, mint_ix=mint_ix, fee=meta.get("fee", 0), tip=a["tip_total"], cup=a["cu_price"])
# 1. both wallets since T0 (failed attempts included)
W = {}
for w in (US, A):
    s = sigs_since(w, T0, tries=6)
    with ThreadPoolExecutor(6) as ex: txs = [d for d in ex.map(lambda sg: (lambda t: decode(t) if t else None)(tri.tx(sg)), list(s)) if d]
    W[w] = txs; log(w[:6], len(s), "signatures")
def mint_of(d, w):
    for x in d["tr"]:
        if x["u"] == w: return x["mint"]
    return d["mint_ix"]
by_mint = collections.defaultdict(lambda: {US: [], A: []})
for w, txs in W.items():
    for d in txs:
        m = mint_of(d, w)
        if m and m.endswith("pump") or (m and len(m) >= 32): by_mint[m][w].append(d)
log("mints touched", len(by_mint))
# 2. every create since T0 - 1 h (for tweet ranks): mint authority signatures, then the create transactions
cs = sigs_since(MA, T0 - 3600, tries=4)
log("creates since T0-1h:", len(cs))
def dcreate(sg):
    t = tri.tx(sg)
    if not t: return None
    d = decode(t)
    if not d["cr"]: return None
    dev = sum(x["sol"] for x in d["tr"] if x["b"] and x["mint"] == d["cr"]["mint"]) / 1e9
    return dict(mint=d["cr"]["mint"], slot=d["slot"], i=d["i"], bt=d["bt"], signer=d["signer"], creator=d["cr"]["creator"], uri=d["cr"]["uri"], dev=dev)
with ThreadPoolExecutor(8) as ex: creates = [c for c in ex.map(dcreate, [k for k, v in cs.items() if not v.get("err")]) if c]
log("decoded creates", len(creates))
host = lambda u: re.sub(r"^https?://([^/]+).*", r"\1", u or "").lower()
def meta(c):
    try:
        with urllib.request.urlopen(urllib.request.Request(c["uri"], headers={"User-Agent": "Mozilla/5.0"}), timeout=20) as r: j = json.loads(r.read(200000))
        mm = re.search(r"/status/(\d+)", j.get("twitter") or ""); return int(mm.group(1)) if mm else 0
    except Exception: return None
asked = [c for c in creates if host(c["uri"]) in TOOLS and c["dev"] >= 0.95]
with ThreadPoolExecutor(12) as ex: sids = list(ex.map(meta, asked))
for c, s in zip(asked, sids): c["sid"] = s
by_sid = collections.defaultdict(list)
for c in asked:
    if c.get("sid"): by_sid[c["sid"]].append(c)
for v in by_sid.values(): v.sort(key=lambda c: (c["slot"], c["i"]))
cmap = {c["mint"]: c for c in creates}
log("tool creates with dev >= 0.95:", len(asked), "tweets", len(by_sid))
# 3. per touched mint: the tape from the create to +14 slots
def tape(m):
    bc = C.bonding_curve(m); sg = []; before = None
    for _ in range(30):
        o = {"limit": 1000, "commitment": "confirmed"}
        if before: o["before"] = before
        s = tri.rpc("getSignaturesForAddress", [bc, o], cache=False) or []
        sg += s
        if len(s) < 1000: break
        before = s[-1]["signature"]
    if not sg: return None
    sg.sort(key=lambda x: x["slot"]); c0 = sg[0]["slot"]
    rows = []
    for x in [x for x in sg if x["slot"] <= c0 + 14][:400]:
        t = tri.tx(x["signature"])
        if t: rows.append(decode(t))
    rows.sort(key=lambda r: (r["slot"], r["i"]))
    return rows
out = []
def one(m):
    rows = tape(m)
    if not rows: return None
    cr_row = next((r for r in rows if r["cr"] and r["cr"]["mint"] == m), None)
    if not cr_row: return dict(mint=m, note="create not found")
    cs_, ci = cr_row["slot"], cr_row["i"]; cr = cr_row["cr"]
    cset = {cr["creator"], cr["user"], cr_row["signer"]}
    seq = [(r["slot"], r["i"], x["u"], x["b"], x["sol"], x["tok"], x["vs"], x["vt"], x["f"]) for r in rows if not r["err"] for x in r["tr"] if x["mint"] == m]
    dev = sum(x[4] for x in seq if (x[0], x[1]) == (cs_, ci) and x[3]) / 1e9
    prev = ci; end = (cs_, ci); pb = None; bw = set()
    for s, i, u, b, sol, tok, vs, vt, f in seq:
        if (s, i) < (cs_, ci): continue
        if (s, i) == (cs_, ci) or (s == cs_ and u in cset and (i - prev) <= 2): prev = max(prev, i); end = max(end, (s, i)); pb = vs; continue
        if s == cs_ and b and u not in (A, US) and (i - prev) <= 2 and (i - ci) <= 5: prev = i; end = (s, i); bw.add(u); pb = vs
        elif (s, i) > end: break
    ins = cset | bw
    fo = next((x for x in seq if (x[0], x[1]) > end and x[3] and x[2] not in ins), None)
    def val(who, hz):
        spent = got = held = 0; st = None
        for s, i, u, b, sol, tok, vs, vt, f in seq:
            if s > hz: break
            st = (vs, vt)
            if u != who: continue
            if b: spent += sol + f; held += tok
            else: got += sol - f; held -= tok
        if not spent: return None
        mark = (st[0] - st[0] * st[1] / (st[1] + held)) * (1 - FEE) if held > 0 else 0
        return (got + mark) / spent - 1
    def realized(w):
        b = [x for d in by_mint[m][w] if not d["err"] for x in d["tr"] if x["mint"] == m and x["u"] == w]
        spent = sum(x["sol"] + x["f"] for x in b if x["b"]); got = sum(x["sol"] - x["f"] for x in b if not x["b"])
        txc = sum(d["fee"] + d["tip"] for d in by_mint[m][w] if not d["err"])
        fail = sum(d["fee"] + d["tip"] for d in by_mint[m][w] if d["err"])
        bs = [d for d in by_mint[m][w] if not d["err"] and any(x["b"] and x["u"] == w and x["mint"] == m for x in d["tr"])]
        ss = [d for d in by_mint[m][w] if not d["err"] and any((not x["b"]) and x["u"] == w and x["mint"] == m for x in d["tr"])]
        return dict(att=len([d for d in by_mint[m][w] if d["err"] or bs]), failed=sum(1 for d in by_mint[m][w] if d["err"]), landed=bool(bs),
                    buy_off=(bs[0]["slot"] - cs_) if bs else None, buy_pos=bs[0]["i"] if bs else None,
                    sell_off=(max(d["slot"] for d in ss) - cs_) if ss else None,
                    stake=spent / 1e9, curve_pct=(100 * (got / spent - 1)) if spent and ss else None,
                    net=(got - spent - txc - fail) / 1e9 if spent and ss else (-fail / 1e9 if not spent else None),
                    net_pct=(100 * (got - spent - txc) / spent) if spent and ss else None, open=bool(spent and not ss))
    c = cmap.get(m); sid = c.get("sid") if c else meta(dict(uri=cr["uri"]))
    rank = None
    if sid:
        lst = by_sid.get(sid, [])
        rank = 1 + sum(1 for o in lst if (o["slot"], o["i"]) < (cs_, ci)) if lst or c else None
    tw_age = (cr_row["bt"] - ((sid >> 22) + 1288834974657) / 1000) if sid else None
    us, him = realized(US), realized(A)
    return dict(mint=m, t=time.strftime("%H:%M:%S", time.gmtime(cr_row["bt"])), host=host(cr["uri"]), dev=round(dev, 3), template=TEMPLATE(dev),
                pb_sol=round(pb / 1e9, 1) if pb else None, signer=cr_row["signer"], creator=cr["creator"], sid=sid, tw_age=round(tw_age) if tw_age is not None else None,
                rank=rank, first_fresh=bool(sid and rank == 1 and tw_age is not None and tw_age < 3600),
                fo_who=("us" if fo and fo[2] == US else "him" if fo and fo[2] == A else (fo[2][:6] if fo else None)), fo_off=(fo[0] - cs_) if fo else None,
                fo_v10=val(fo[2], fo[0] + 10) if fo else None, fo_v1=val(fo[2], fo[0] + 1) if fo else None, fo_v3=val(fo[2], fo[0] + 3) if fo else None,
                us=us, him=him, n_rows=len(rows))
with ThreadPoolExecutor(4) as ex: res = [r for r in ex.map(one, list(by_mint)) if r]
res.sort(key=lambda r: r.get("t", ""))
json.dump(dict(res=res, n_creates=len(creates), n_asked=len(asked)), open(SP + "/live/recon.json", "w"))
log("done", len(res))
