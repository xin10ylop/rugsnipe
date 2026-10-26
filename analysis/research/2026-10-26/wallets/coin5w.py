# Per coin (every mint either wallet bought or tried to buy, Oct 21 -> now): the create (signer, creator argument, uri,
# name, ticker, ix, mode tail, dev buy in the create transaction) and every trade of the first 13 slots (create slot +
# 12) with reserves after and fees. -> five/coin/<mint>.json
import sys, json, os, struct, time, collections, threading
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP, common as C
from rugsnipe_analysis import events as E
tri.MIN_GAP = 0.012
OUT = SP + "/wide"
def log(*a): print(time.strftime("%H:%M:%S"), *a, flush=True)
def rstr(b, o):
    n = struct.unpack_from("<I", b, o)[0]; return b[o + 4:o + 4 + n], o + 4 + n
def coin(m):
    p = f"{OUT}/coin/{m}.json"
    if os.path.exists(p): return "have"
    bc = C.bonding_curve(m); allsig = []; before = None; reached = False
    for _ in range(40):
        o = {"limit": 1000}
        if before: o["before"] = before
        s = tri.rpc("getSignaturesForAddress", [bc, o], cache=False) or []
        allsig += s
        if len(s) < 1000: reached = True; break
        before = s[-1]["signature"]
    if not allsig:
        json.dump(dict(mint=m, reached=False), open(p, "w")); return "empty"
    cs = min(x["slot"] for x in allsig)
    out = dict(mint=m, reached=reached, cs=cs, n_sigs=len(allsig))
    rows = []; failed = []
    for x in allsig:
        if x["slot"] > cs + 12: continue
        if x.get("err"): continue   # landed trades only: the failed snipes are most of a busy launch
        t = tri.rpc("getTransaction", [x["signature"], {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}], cache=False)
        if not t: continue
        ks = PP.keys_of(t); idx = t.get("transactionIndex") or 0
        if x.get("err"):
            failed.append([t["slot"], idx, ks[0]]); continue
        for prog, accs, data, parsed, oi, inner in PP._ixs(t, ks):
            if prog != C.PUMP or not data: continue
            if data[:8] in PP.IXR and PP.IXR[data[:8]].startswith("create") and accs and accs[0] == m and "uri" not in out:
                try:
                    o = 8; name, o = rstr(data, o); sym, o = rstr(data, o); uri, o = rstr(data, o)
                    creator = C.b58e(data[o:o + 32]); tail = data[o + 32:]
                except Exception:
                    name = sym = uri = b""; creator = None; tail = b""
                u = uri.decode(errors="replace")
                host = u.split("//")[-1].split("/")[0].lower() if "//" in u else None
                out.update(create=dict(slot=t["slot"], idx=idx, signer=ks[0], signers=ks[:t["transaction"]["message"]["header"]["numRequiredSignatures"]],
                                       creator=creator, ix=PP.IXR[data[:8]], inner=bool(inner), uri=u[:200], host=host, name=name.decode(errors="replace")[:40],
                                       sym=sym.decode(errors="replace")[:20], tail=tail.hex(), progs=sorted({k for k in ks if not k.startswith("1111")})[:12],
                                       cu_price=None, fee=t["meta"].get("fee")))
            if data[:8] == E.EVENT_IX_TAG and data[8:16] == E.DISC_TRADE:
                try: e = E.decode_trade(data[8:])
                except Exception: continue
                if e.mint == m:
                    rows.append([t["slot"], idx, e.user, int(e.is_buy), e.sol_amount, e.token_amount, e.virtual_sol_reserves, e.virtual_token_reserves, (e.fee or 0) + (e.creator_fee or 0), ks[0]])
    rows.sort(key=lambda r: (r[0], r[1]))
    out["rows"] = rows; out["failed"] = sorted(failed)
    json.dump(out, open(p + ".tmp", "w")); os.replace(p + ".tmp", p)
    return "ok" if reached else "partial"
if __name__ == "__main__":
    mints = [l.strip() for l in open(sys.argv[1]) if l.strip()]
    log("coins", len(mints))
    cnt = collections.Counter(); lock = threading.Lock()
    def safe(m):
        try: r = coin(m)
        except Exception as e: r = "error"
        with lock:
            cnt[r] += 1
            if sum(cnt.values()) % 250 == 0: log(dict(cnt))
    with ThreadPoolExecutor(16) as ex: list(ex.map(safe, mints))
    log("done", dict(cnt))
