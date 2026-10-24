# Who else signs transactions that call AHwJ's guard programs? (recent pages, newest first)
import sys, json, time, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/rivalpnl")
import common as C
C.MIN_GAP = 0.3
PROGS = {"slot-window guard": "A5c1eN7MzgYWAfBBbwogAkjjmoiRYP3CDGTV2Fa5x4hp", "curve-state guard": "8jzqTjEDKxxTppLGjyhUXUmDbHeUjJ5kkpGiWmC9jAez"}
out = {}
for name, p in PROGS.items():
    signers = collections.Counter(); first = last = None; n = 0; tok = None
    for page in range(int(sys.argv[1]) if len(sys.argv) > 1 else 5):
        opt = {"transactionDetails": "signatures", "sortOrder": "desc", "limit": 1000}
        if tok: opt["paginationToken"] = tok
        r = C.rpc("getTransactionsForAddress", [p, opt], cache=False) or {}
        d = r.get("data") or []
        for t in d:
            n += 1; bt = t.get("blockTime") or 0
            last = bt if last is None else max(last, bt); first = bt if first is None else min(first, bt)
        sig_list = [t["signature"] for t in d]
        # signer of each tx: needs the tx; sample up to 60 per page
        for s in sig_list[:: max(1, len(sig_list) // 60)]:
            tx = C.rpc("getTransaction", [s, {"encoding": "json", "maxSupportedTransactionVersion": 1}])
            if tx: signers[tx["transaction"]["message"]["accountKeys"][0]] += 1
        tok = r.get("paginationToken")
        if not tok or len(d) < 1000: break
    out[name] = dict(program=p, txs_seen=n, span_h=round(((last or 0) - (first or 0)) / 3600, 1), signers_sampled=signers.most_common(15))
    print(name, p[:8], "txs", n, "over", out[name]["span_h"], "h; sampled signers:", [(k[:8], v) for k, v in signers.most_common(10)], flush=True)
json.dump(out, open(SP + "/month/guard_users.json", "w"), indent=1)
