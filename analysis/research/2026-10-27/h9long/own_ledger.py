# Our own wallet, every lamport: each transaction since T0 (landed and failed) from <w8>_sigs.json, classified by what
# it did, and the sum against the wallet's balance change.   python3 -I own_ledger.py WALLET [fetch]
import sys, os, json, time, collections, datetime
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month")
import tri
W = sys.argv[1]
OUT = f"{SP}/h9long/{W[:8]}_own.jsonl"
SYS, CB = "11111111111111111111111111111111", "ComputeBudget111111111111111111111111111111"
PUMP, AMM = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P", "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
TOKEN, T22 = "TokenkegQfeZyiNwAJbNbGqPFXCWuBvf9Ss623VQ5DA", "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
ATA = "ATokenGPvbdGVxr1b2hSZbsiW5xqK1qxNzGyLRCDM6M"
def row(tx, err):
    meta = tx.get("meta") or {}; m = tx["transaction"]["message"]
    ks = list(m["accountKeys"]) + list((meta.get("loadedAddresses") or {}).get("writable", [])) + \
        list((meta.get("loadedAddresses") or {}).get("readonly", []))
    i = ks.index(W) if W in ks else None
    progs = set(ks[ix["programIdIndex"]] for ix in m["instructions"])
    for g in meta.get("innerInstructions") or []:
        for ix in g["instructions"]: progs.add(ks[ix["programIdIndex"]])
    tok = collections.Counter()
    for side, sgn in (("preTokenBalances", -1), ("postTokenBalances", 1)):
        for b in meta.get(side) or []:
            if b.get("owner") == W: tok[b["mint"]] += sgn * int(b["uiTokenAmount"]["amount"])
    logs = meta.get("logMessages") or []
    return dict(sig=tx["transaction"]["signatures"][0], s=tx["slot"], bt=tx.get("blockTime"), fee=meta.get("fee"),
                err=err, signer=ks[0] == W, d=(meta["postBalances"][i] - meta["preBalances"][i]) if i is not None else 0,
                tok={k: v for k, v in tok.items() if v}, progs=sorted(progs - {SYS, CB}),
                buy=any("Instruction: Buy" in l for l in logs), sell=any("Instruction: Sell" in l for l in logs),
                nlogs=len(logs))
if "fetch" in sys.argv:
    sigs = json.load(open(f"{SP}/h9long/{W[:8]}_sigs.json"))
    done = set()
    if os.path.exists(OUT):
        for l in open(OUT): done.add(json.loads(l)["sig"])
    todo = [x for x in sigs if x[0] not in done]
    print(len(sigs), "signatures,", len(todo), "to fetch", flush=True)
    def one(x):
        for _ in range(4):
            try:
                tx = tri.rpc("getTransaction", [x[0], {"encoding": "json", "maxSupportedTransactionVersion": 0,
                                                       "commitment": "confirmed"}], cache=False)
                if tx: return json.dumps(row(tx, x[3]), separators=(",", ":"))
            except Exception:
                time.sleep(0.5)
        return None
    with ThreadPoolExecutor(10) as ex, open(OUT, "a") as f:
        for line in ex.map(one, todo):
            if line: f.write(line + "\n")
R = [json.loads(l) for l in open(OUT)]
R.sort(key=lambda r: (r["s"], r["sig"]))
def kind(r):
    if r["err"]: return "failed tx (fee only)"
    if not r["signer"]: return "incoming (not signed by us)"
    if r["buy"] and r["sell"]: return "buy+sell in one tx"
    if r["buy"]: return "buy"
    if r["sell"]: return "sell"
    if not r["progs"]: return "SOL transfer (system only)"
    if r["progs"] in ([TOKEN], [T22], [TOKEN, T22]): return "token-account close/transfer"
    if ATA in r["progs"] and len(r["progs"]) <= 3: return "token-account create"
    return "other: " + ",".join(p[:6] for p in r["progs"])
g = collections.defaultdict(lambda: [0, 0])
for r in R:
    k = kind(r); g[k][0] += 1; g[k][1] += r["d"]
tot = sum(r["d"] for r in R)
print(f"{len(R)} transactions, {datetime.datetime.fromtimestamp(R[0]['bt'], datetime.timezone.utc):%m-%d %H:%M} to "
      f"{datetime.datetime.fromtimestamp(R[-1]['bt'], datetime.timezone.utc):%m-%d %H:%M} UTC; our SOL change {tot / 1e9:+.4f}")
for k, (n, d) in sorted(g.items(), key=lambda x: x[1][1]):
    print(f"   {k:34s} {n:5d}  {d / 1e9:+9.4f}")
if "-v" in sys.argv:
    for r in R:
        if kind(r) in ("SOL transfer (system only)", "incoming (not signed by us)") or kind(r).startswith("other"):
            print(f"   {datetime.datetime.fromtimestamp(r['bt'], datetime.timezone.utc):%m-%d %H:%M:%S} {kind(r):30s} {r['d'] / 1e9:+.4f} {r['sig'][:20]} {r['progs']}")
