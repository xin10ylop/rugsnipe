"""Stage 4: funding chain (<=3 hops) of each sampled create's signer.

Per wallet W (the create signer) and time T (the create):
  sigs = getSignaturesForAddress(W, before=create_sig)  (1 page)
  - 1000 sigs  -> 'serial' (long-lived wallet; operator key = the wallet itself)
  - else scan the 4 oldest successful txs: first incoming SOL (funder) and the largest
    incoming SOL (main). Follow `main` upward with the same rule (before = funding sig),
    stopping at a labelled terminal (CEX/bridge/swap...), a pump.fun curve (sell proceeds),
    or a wallet with >=1000 earlier sigs ('busy').
Output: data/cache/gap1/funding.jsonl  {sig, signer, n_pre, chain:[{addr,amount,kind,label,n_pre,dt}]}
"""
import json
import sys
import threading
from concurrent.futures import ThreadPoolExecutor

from common import OUT, SIGS, log, rpc
from rugsnipe_analysis import labels as L
from rugsnipe_analysis.funding import incoming_transfers, signers, program_ids

c = rpc(9.0)
PUMP = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
PUMPSWAP = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
rows = [json.loads(l) for l in open(OUT / "creates.jsonl")]
rows = [r for r in rows if r.get("kind") == "create"]
rows.sort(key=lambda r: r["rank"])
lim = int(sys.argv[1]) if len(sys.argv) > 1 else None
if lim:
    rows = rows[:lim]
out_path = OUT / "funding.jsonl"
done = set()
if out_path.exists():
    for l in open(out_path):
        done.add(json.loads(l)["sig"])
lock = threading.Lock()
fo = open(out_path, "a")
memo = {}
mlock = threading.Lock()


def funding_of(wallet, before_sig, before_time):
    """-> dict(n_pre, first_time, funder, amount, kind, fsig, ftime, via) (memoized)."""
    key = (wallet, before_sig)
    with mlock:
        if key in memo:
            return memo[key]
    page = c.call("getSignaturesForAddress", [wallet, {"limit": 1000, "before": before_sig}],
                  cache_key=f"sigs:{wallet}:{before_sig}:None:1000",
                  endpoints=SIGS) or []
    res = {"n_pre": len(page)}
    if len(page) >= 1000:
        res["status"] = "busy"
    elif not page:
        res["status"] = "no_history"
    else:
        res["first_time"] = page[-1].get("blockTime")
        chron = [s for s in reversed(page) if s.get("err") is None][:3]
        recs = []
        for s in chron:
            tx = c.get_transaction(s["signature"])
            if not tx:
                continue
            ins = incoming_transfers(tx, wallet)
            if not ins:
                continue
            t = max(ins, key=lambda x: x.lamports)
            progs = program_ids(tx)
            via = "pump_curve" if (PUMP in progs or PUMPSWAP in progs) and t.source not in signers(tx) else ""
            recs.append({"funder": t.source, "amount": round(t.lamports / 1e9, 6), "kind": t.kind,
                         "fsig": s["signature"], "ftime": s.get("blockTime"), "via": via})
            if t.lamports >= 50_000_000:
                break
        if not recs:
            res["status"] = "no_incoming"
        else:
            res["status"] = "ok"
            res["first"] = recs[0]
            res["main"] = max(recs, key=lambda x: x["amount"])
    with mlock:
        memo[key] = res
    return res


def trace(r):
    if r["sig"] in done:
        return
    w = r["user"]
    chain = []
    f = funding_of(w, r["sig"], r["bt"])
    out = {"sig": r["sig"], "signer": w, "n_pre": f["n_pre"], "status": f["status"],
           "first_time": f.get("first_time"), "first": f.get("first"), "chain": chain}
    cur = f
    depth = 0
    while cur.get("status") == "ok" and depth < 3:
        m = cur["main"]
        lab = L.get_label(m["funder"])
        hop = {"addr": m["funder"], "amount": m["amount"], "ftime": m["ftime"], "via": m["via"],
               "label": (lab.name if lab else None), "cat": (lab.category if lab else None)}
        chain.append(hop)
        depth += 1
        if lab and lab.category in L.TERMINAL_CATEGORIES or m["via"]:
            break
        if depth >= 3:
            break
        nxt = funding_of(m["funder"], m["fsig"], m["ftime"])
        hop["n_pre"] = nxt["n_pre"]
        hop["status"] = nxt["status"]
        cur = nxt
    with lock:
        fo.write(json.dumps(out, separators=(",", ":")) + "\n"); fo.flush()
        done.add(r["sig"])
        if len(done) % 500 == 0:
            log(f"{len(done)} traced; rpc {c.stats}")


import time
while True:
    log(f"{len(rows)} creates, {len(done)} done")
    with ThreadPoolExecutor(14) as ex:
        for fu in [ex.submit(trace, r) for r in rows if r["sig"] not in done]:
            try:
                fu.result()
            except Exception as e:  # noqa: BLE001
                log("fail", type(e).__name__, str(e)[:160])
    if "--follow" not in sys.argv:
        break
    time.sleep(30)
    rows = sorted((json.loads(l) for l in open(OUT / "creates.jsonl")), key=lambda r: r["rank"])
    rows = [r for r in rows if r.get("kind") == "create" and r["sig"] not in done]
    if not rows:
        break
log("done", len(done), c.stats)
