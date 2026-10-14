"""Gap 4 phase 1: profile hot wallets + sample outgoing SOL transfers."""
import json, random, sys, time
from concurrent.futures import ThreadPoolExecutor
sys.path.insert(0, "/home/user/rugsnipe/analysis")
from rugsnipe_analysis.rpc import RpcClient
from rugsnipe_analysis.funding import sol_transfers, account_keys, program_ids, signers

OUT = "data/cache/gap4/"  # run from repo root (mkdir -p data/cache/gap4)
W = {
 "5tzFkiKscXHK5ZXCGbXZxdw7gTjjD1mBwuoFbhUvuAi9": "Binance(label)",
 "BmFdpraQhkiDQE6SnfG5omcA1VwzqfXrwtNYBwWTymy6": "KuCoin(label)",
 "A77HErqtfN1hLLpvZ9pCtu66FEtM8BveoaKbbMoZ4RiR": "Bitget(label)",
 "is6MTRHEgyFLNTfYcuV4QBWLjrZBfmhVNYR6ccgr8KV": "OKX(label)",
 "GJRs4FwHtemZ5ZE9x3FNvJ8TMwitKTh21yxdRPqn7npE": "Coinbase3(label)",
 "2AQdpHJ2JpcEgPiATUXjQxA8QmafFegfQwSLWSprPicm": "Coinbase2(label)",
 "F7p3dFrjRTbtRp8FRF6qHLomXbKRBzpvBLjtQcfcgmNe": "Relay/Mayan?",
 "FncazAs6omJJjtLVzquzT9KoyXn6tFixr9kGjr42ktLj": "unknown",
 "iGdFcQoyR2MwbXMHQskhmNsqddZ6rinsipHc4TNSdwu": "Bybit?",
 "5F1seMKUqSNhv45f6FhB2cFmgJbk8U1avJw7M6TexUq1": "MoonPay?",
}
NOW = int(time.time())
T_LO, T_HI = NOW - 6 * 3600, NOW - 75 * 60
N_SAMPLE = int(sys.argv[1]) if len(sys.argv) > 1 else 300
rpc = RpcClient(max_rps=10)

def collect(addr):
    sigs, before, pages = [], None, 0
    while pages < 32:
        page = rpc.get_signatures(addr, before=before, cache=False)
        pages += 1
        if not page: break
        sigs += page
        before = page[-1]["signature"]
        if (page[-1].get("blockTime") or 0) < T_LO or len(page) < 1000: break
    return sigs, pages

def decode(addr, s):
    tx = None
    for k in range(6):
        try:
            tx = rpc.get_transaction(s["signature"]); break
        except Exception as e:
            time.sleep(5 * (k + 1))
    if not tx: return None
    keys = account_keys(tx); meta = tx.get("meta") or {}
    pre = meta.get("preBalances") or []
    out, inc = [], []
    for t in sol_transfers(tx):
        if t.source == addr and t.destination != addr:
            i = keys.index(t.destination) if t.destination in keys else None
            out.append({"dst": t.destination, "lam": t.lamports, "kind": t.kind, "inner": t.inner,
                        "dst_pre": pre[i] if i is not None and i < len(pre) else None})
        elif t.destination == addr and t.source != addr:
            inc.append({"src": t.source, "lam": t.lamports, "inner": t.inner})
    tok = []
    for side in ("preTokenBalances", "postTokenBalances"):
        for b in meta.get(side) or []:
            if b.get("owner") == addr: tok.append(b.get("mint"))
    logs = meta.get("logMessages") or []
    memo = [l for l in logs if "Memo" in l][:2]
    return {"sig": s["signature"], "bt": s.get("blockTime"), "slot": s.get("slot"),
            "fee_payer": keys[0] if keys else None, "signers": signers(tx),
            "programs": program_ids(tx), "out": out, "inc": inc, "tok_mints": sorted(set(tok)),
            "n_ix": len(((tx.get("transaction") or {}).get("message") or {}).get("instructions") or []),
            "memo": memo, "fee": meta.get("fee")}

import os
prev = json.load(open(OUT + "p1.json")) if os.path.exists(OUT + "p1.json") else None
res = prev["wallets"] if prev else {}
if prev: NOW, T_LO, T_HI = prev["now"], prev["T_LO"], prev["T_HI"]
for addr, lab in W.items():
    if addr in res: continue
    t0 = time.time()
    sigs, pages = collect(addr)
    win = [s for s in sigs if s.get("blockTime") and T_LO <= s["blockTime"] <= T_HI]
    ok = [s for s in win if s.get("err") is None]
    span_all = (sigs[0]["blockTime"] - sigs[-1]["blockTime"]) if len(sigs) > 1 else None
    # if wallet history didn't reach T_LO, the window is the covered part
    cov_lo = max(T_LO, sigs[-1]["blockTime"]) if sigs else T_LO
    win_hours = (T_HI - cov_lo) / 3600 if cov_lo < T_HI else 0
    rng = random.Random(42)
    samp = rng.sample(ok, min(N_SAMPLE, len(ok)))
    with ThreadPoolExecutor(8) as ex:
        dec = [d for d in ex.map(lambda s: decode(addr, s), samp) if d]
    res[addr] = {"label_in": lab, "pages": pages, "n_sigs_fetched": len(sigs),
                 "tx_per_h_fetched": round(len(sigs) / (span_all / 3600), 1) if span_all else None,
                 "window": [cov_lo, T_HI], "win_hours": round(win_hours, 3),
                 "win_sigs": len(win), "win_ok": len(ok), "sampled": len(samp), "decoded": dec}
    print(f"{lab:18s} {addr[:6]} pages={pages} fetched={len(sigs)} win_ok={len(ok)} wh={win_hours:.2f} dec={len(dec)} {time.time()-t0:.0f}s stats={rpc.stats}", flush=True)
    json.dump({"now": NOW, "T_LO": T_LO, "T_HI": T_HI, "wallets": res}, open(OUT + "p1.json", "w"))
print("done")
