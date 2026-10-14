"""Gap 4 phase 2: classify wallets, freshness verification, create lookups, fee inference."""
import json, sys, time, collections, threading
from concurrent.futures import ThreadPoolExecutor
import requests
sys.path.insert(0, "/home/user/rugsnipe/analysis")
from rugsnipe_analysis.rpc import RpcClient, RateLimiter
D = "data/cache/gap4/"  # run from repo root
p1 = json.load(open(D + "p1.json"))
rpc = RpcClient(max_rps=8)
plim = RateLimiter(4)
NOISE = {"11111111111111111111111111111111", "ComputeBudget111111111111111111111111111111",
         "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", "ATokenGPvbdGVxr1b2hYZbnyiAhy7mXkKZgRpkb7mt",
         "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr", "Memo1UhkJRfHyvLMcVucJwxXeuD728EqVDDwQDxFMNo",
         "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"}
LAM = 1e9
def oldest_sig(addr):
    page = rpc.get_signatures(addr, cache=False)
    return page, len(page) < 1000
def coins_of(addr):
    plim.acquire()
    for _ in range(5):
        try:
            r = requests.get("https://frontend-api-v3.pump.fun/coins", params={"creator": addr, "limit": 50, "offset": 0,
                             "includeNsfw": "true", "sort": "created_timestamp", "order": "ASC"}, timeout=20)
            if r.status_code == 200: return r.json()
            time.sleep(2)
        except Exception: time.sleep(2)
    return None
out = {}
fresh_all = []
for addr, w in p1["wallets"].items():
    dec = w["decoded"]; n = len(dec)
    signer = sum(1 for d in dec if addr in d["signers"])
    feepayer = sum(1 for d in dec if d["fee_payer"] == addr)
    has_out = [d for d in dec if d["out"]]
    has_in = [d for d in dec if d["inc"]]
    tok = sum(1 for d in dec if d["tok_mints"])
    progs = collections.Counter(p for d in dec for p in d["programs"] if p not in NOISE)
    payers = collections.Counter(d["fee_payer"] for d in dec)
    outs = [(d, o) for d in dec for o in d["out"] if o["kind"] == "transfer"]
    nout_per_tx = collections.Counter(len(d["out"]) for d in has_out)
    fresh = [(d, o) for d, o in outs if o["dst_pre"] == 0]
    inc_src = collections.Counter(i["src"] for d in dec for i in d["inc"])
    memo = sum(1 for d in dec if d["memo"])
    rate_ok = w["win_ok"] / w["win_hours"] if w["win_hours"] else None
    scale = (rate_ok * 24 / n) if (n and rate_ok) else None
    out[addr] = {"label_in": w["label_in"], "tx_per_h_ok": round(rate_ok, 1) if rate_ok else None,
                 "decoded": n, "wallet_signs": signer, "wallet_fee_payer": feepayer,
                 "tx_with_outgoing_sol": len(has_out), "tx_with_incoming_sol": len(has_in), "tx_with_token": tok,
                 "tx_with_memo": memo, "outgoing_sol_transfers": len(outs), "outs_per_tx": dict(nout_per_tx),
                 "outgoing_to_prebalance0": len(fresh), "top_programs": progs.most_common(8),
                 "top_fee_payers": payers.most_common(3), "top_incoming_src": inc_src.most_common(5),
                 "scale_per_day": scale,
                 "out_amounts": [o["lam"] for _, o in outs]}
    for d, o in fresh:
        fresh_all.append({"funder": addr, "dst": o["dst"], "lam": o["lam"], "sig": d["sig"], "bt": d["bt"]})
    print(addr[:6], w["label_in"], {k: out[addr][k] for k in ["tx_per_h_ok", "decoded", "wallet_signs", "tx_with_outgoing_sol", "tx_with_incoming_sol", "tx_with_token", "outgoing_sol_transfers", "outgoing_to_prebalance0"]}, flush=True)

def check(f):
    page, complete = oldest_sig(f["dst"])
    f["n_sigs"] = len(page); f["complete"] = complete
    f["first_is_funding"] = bool(page) and complete and page[-1]["signature"] == f["sig"]
    f["sigs_before_funding"] = None
    if complete and page:
        ts = [s for s in page if s.get("blockTime") and s["blockTime"] < f["bt"]]
        f["sigs_before_funding"] = len(ts)
    # outgoing activity times after funding (for quick behaviour stats)
    later = sorted(s["blockTime"] for s in page if s.get("blockTime") and s["blockTime"] >= f["bt"])
    f["n_after"] = len(later)
    cs = coins_of(f["dst"])
    f["coins_api_ok"] = cs is not None
    cl = [(c["created_timestamp"] // 1000 - f["bt"]) for c in (cs or [])]
    f["create_lags"] = cl
    return f
with ThreadPoolExecutor(6) as ex:
    fresh_all = list(ex.map(check, fresh_all))
json.dump({"wallets": out, "fresh": fresh_all, "now": p1["now"]}, open(D + "p2.json", "w"))
print("fresh recipients", len(fresh_all), "stats", rpc.stats)
