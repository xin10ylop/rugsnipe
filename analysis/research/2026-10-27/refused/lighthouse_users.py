# Whose token accounts do pump bots assert with Lighthouse (AssertTokenAccount on another wallet's account)?
# Is any of them a wallet we copy? And what do they assert (amount >= / <=)?
import sys, json, collections, base58
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month")
import tri
LH = "L2TExMFKdjpN9kozasaurPirfHy9P8sbXoAN1qA3S95"; PUMP = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
OURS = {"AHwJSnFN", "3yrJE1A8", "JBokzLFR", "H9kuqGKN", "CxyKYmQo", "D5BDDkBt", "4g49xGbg"}
own = collections.Counter(); pairs = collections.Counter(); ops = collections.Counter(); before = None; n = 0; errs = collections.Counter()
for _ in range(6):
    o = {"limit": 1000}
    if before: o["before"] = before
    sigs = tri.rpc("getSignaturesForAddress", [LH, o], cache=False); before = sigs[-1]["signature"]
    for s in sigs[::3]:
        tx = tri.rpc("getTransaction", [s["signature"], {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}], cache=False)
        if not tx: continue
        m = tx["transaction"]["message"]; la = tx["meta"].get("loadedAddresses") or {}
        keys = m["accountKeys"] + la.get("writable", []) + la.get("readonly", [])
        if PUMP not in keys: continue
        n += 1; signer = keys[0]
        toks = {b["accountIndex"]: (b.get("owner"), b["mint"], b["uiTokenAmount"]["amount"]) for b in (tx["meta"].get("preTokenBalances") or [])}
        for ix in m["instructions"]:
            if keys[ix["programIdIndex"]] != LH: continue
            d = base58.b58decode(ix["data"])
            if d[0] != 9: continue
            for a in ix["accounts"]:
                t = toks.get(a)
                if t and t[0] != signer:
                    own[t[0][:8]] += 1; pairs[(signer[:8], t[0][:8])] += 1; ops[(d[2], d[-1])] += 1
                    errs[tx["meta"].get("err") is None] += 1
print("pump txs sampled", n)
print("owners of the asserted token accounts (not the signer):", own.most_common(15))
print("ours among them:", {k: v for k, v in own.items() if k in OURS})
print("(signer, owner) pairs:", pairs.most_common(10))
print("assertion (variant, operator):", ops.most_common(6), " tx succeeded:", dict(errs))
