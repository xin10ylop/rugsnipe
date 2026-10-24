# Tweet-coin dataset for one UTC day window [H0, H1) hours: stage A creates, B metadata, C early trades.
#   python3 -I univ/pipeline.py 0 12
import sys, json, os, re, time, calendar, threading, urllib.request, collections
from concurrent.futures import ThreadPoolExecutor
from urllib.parse import urlparse
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP, common as C
from rugsnipe_analysis import events as E
H0, H1 = int(sys.argv[1]), int(sys.argv[2])
DAY = calendar.timegm((2026, 10, 24, 0, 0, 0, 0, 0, 0))
MA = "TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM"
LH = json.load(open(SP + "/today/legs_him.json")); H_ATT = {x["mint"] for x in LH if x["kind"] == "buy" and x["mint"]}
def log(*a): print(time.strftime("%H:%M:%S"), *a, flush=True)
def decode_create(sig):
    t = tri.tx(sig)
    if not t: return None
    ks = PP.keys_of(t); meta = t.get("meta") or {}; cr = None; trades = []
    for grp in meta.get("innerInstructions") or []:
        for ix in grp["instructions"]:
            if ks[ix["programIdIndex"]] != C.PUMP: continue
            raw = C.b58d(ix.get("data", ""))
            if raw[:8] != E.EVENT_IX_TAG: continue
            try:
                if raw[8:16] == E.DISC_CREATE:
                    e = E.decode_create(raw[8:]); cr = dict(mint=e.mint, creator=e.creator, user=e.user, name=e.name, symbol=e.symbol, uri=e.uri, mayhem=getattr(e, "is_mayhem_mode", None))
                elif raw[8:16] == E.DISC_TRADE:
                    e = E.decode_trade(raw[8:]); trades.append((e.user, e.is_buy, e.sol_amount, e.virtual_sol_reserves, e.virtual_token_reserves))
            except Exception: pass
    if not cr: return None
    a = PP.analyze(t)
    cr.update(sig=sig, slot=t["slot"], idx=t.get("transactionIndex"), bt=t.get("blockTime"), signer=ks[0], ix=a["pump_ix"],
              dev_sol=sum(s for u, b, s, v1, v2 in trades if b) / 1e9, post_dev=(trades[-1][3], trades[-1][4]) if trades else None,
              tip=a["tip_total"], cup=a["cu_price"])
    return cr
# ---- stage A: creates per hour
for h in range(H0, H1):
    T0 = DAY + h * 3600; out = f"{SP}/univ/creates_{T0}.json"
    if os.path.exists(out): continue
    sigs = []; before = None
    # page from the newest signature before T0+3600 (fast-forward with 'before' from the previous hour's newest if known)
    while True:
        s = tri.sigs(MA, before=before)
        if not s: break
        before = s[-1]["signature"]
        sigs += [x for x in s if T0 <= (x.get("blockTime") or 0) < T0 + 3600 and not x.get("err")]
        if (s[-1].get("blockTime") or 0) < T0: break
    with ThreadPoolExecutor(4) as ex: res = list(ex.map(lambda x: decode_create(x["signature"]), sigs))
    res = [r for r in res if r]
    json.dump(res, open(out, "w")); log("hour", h, "creates", len(res))
# ---- stage B: metadata for creates with dev >= 0.95 SOL or attempted by him
os.makedirs(SP + "/univ/meta", exist_ok=True)
def fetch_meta(c):
    p = f"{SP}/univ/meta/{c['mint']}.json"
    if os.path.exists(p): return
    try:
        req = urllib.request.Request(c["uri"], headers={"User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(req, timeout=20) as r: body = r.read(200_000)
        json.dump(dict(uri=c["uri"], body=body.decode("utf-8", "replace")), open(p, "w"))
    except Exception as e:
        json.dump(dict(uri=c["uri"], error=str(e)[:120]), open(p, "w"))
allc = []
for h in range(H0, H1): allc += json.load(open(f"{SP}/univ/creates_{DAY + h * 3600}.json"))
cand = [c for c in allc if c["dev_sol"] >= 0.95 or c["mint"] in H_ATT]
log("stage B: metadata for", len(cand), "of", len(allc), "creates")
with ThreadPoolExecutor(8) as ex: list(ex.map(fetch_meta, cand))
# ---- stage C: early trades (create .. create + 12 slots, failed txs included) for fresh tweet coins + his attempts
def tweet(c):
    p = f"{SP}/univ/meta/{c['mint']}.json"
    try: j = json.loads(json.load(open(p))["body"])
    except Exception: return None, None
    mm = re.search(r"/status/(\d+)", j.get("twitter") or "")
    if not mm: return j, None
    return j, ((int(mm.group(1)) >> 22) + 1288834974657) / 1000
sel = []
for c in cand:
    j, tt = tweet(c)
    if c["mint"] in H_ATT or (tt is not None and c["bt"] - tt < 3600 and c["dev_sol"] >= 0.95): sel.append(c)
log("stage C: early trades for", len(sel), "launches")
os.makedirs(SP + "/univ/early", exist_ok=True)
def early(c):
    p = f"{SP}/univ/early/{c['mint']}.json"
    if os.path.exists(p): return
    bc = C.bonding_curve(c["mint"]); sigs = []; before = None; pages = 0
    while True:
        o = {"limit": 1000, "until": c["sig"]}
        if before: o["before"] = before
        s = tri.rpc("getSignaturesForAddress", [bc, o], cache=False) or []
        pages += 1; sigs += s
        if len(s) < 1000 or pages >= 40: break
        before = s[-1]["signature"]
    ear = sorted([x for x in sigs if x["slot"] <= c["slot"] + 12], key=lambda x: x["slot"])[:150]
    rows = []
    for x in ear:
        t = tri.tx(x["signature"])
        if not t: continue
        ks = PP.keys_of(t); ev = []
        for grp in (t.get("meta") or {}).get("innerInstructions") or []:
            for ix in grp["instructions"]:
                if ks[ix["programIdIndex"]] != C.PUMP: continue
                raw = C.b58d(ix.get("data", ""))
                if raw[:8] == E.EVENT_IX_TAG and raw[8:16] == E.DISC_TRADE:
                    try:
                        e = E.decode_trade(raw[8:])
                        if e.mint == c["mint"]: ev.append([e.user, int(e.is_buy), e.sol_amount, e.token_amount, e.virtual_sol_reserves, e.virtual_token_reserves])
                    except Exception: pass
        a = PP.analyze(t)
        rows.append(dict(s=t["slot"], i=t.get("transactionIndex"), u=ks[0], err=x.get("err") is not None, ev=ev, tip=a["tip_total"], cup=a["cu_price"], cul=a["cu_limit"], pix=a["pump_ix"]))
    json.dump(dict(mint=c["mint"], n_sigs=len(sigs), pages=pages, trunc=pages >= 40, rows=rows), open(p, "w"))
done = [0]
def early_l(c):
    try: early(c)
    except Exception as e: log("early error", c["mint"][:8], tri.red(e)[:100])
    done[0] += 1
    if done[0] % 100 == 0: log("early", done[0], "of", len(sel))
with ThreadPoolExecutor(4) as ex: list(ex.map(early_l, sel))
log("pipeline done", H0, H1)
