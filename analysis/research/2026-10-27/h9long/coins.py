# Per coin from <w8>.jsonl: venue, its buys and sells (slot, time, SOL, tokens, the curve's real SOL at the trade), its
# net in SOL (the wallet's SOL change on the coin's transactions, fees and tips in; a swap through wSOL nets), the
# quote (SOL / a stablecoin). -> <w8>_coins.json        python3 -I coins.py WALLET
import sys, json, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = sys.argv[1]
WSOL = "So11111111111111111111111111111111111111112"
STABLE = {"EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v": "USDC", "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB": "USDT",
          "USD1ttGY1N17NEEHLmELoaybftRBUSErhqYiQzvEmuB": "USD1"}
PUMP, PAMM = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P", "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
VEN = {PUMP: "pump", PAMM: "pumpswap", "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4": "jupiter",
       "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo": "meteora", "cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG": "meteora",
       "Eo7WjKq67rjJQSZxS6z3YkapzY3eMj6Xy8X5EQVn5UaB": "meteora", "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc": "orca",
       "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK": "raydium", "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8": "raydium",
       "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C": "raydium", "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj": "raydium"}
rows = sorted((json.loads(l) for l in open(f"{SP}/h9long/{W[:8]}.jsonl")), key=lambda r: (r["s"], r["i"] or 0))
coins = {}; other = collections.Counter(); xfer = []
for r in rows:
    own = [t for t in r["tr"] if t[1] == W]
    toks = {m: v for m, v in r["tok"].items() if m != WSOL and m not in STABLE}
    q = {STABLE[m]: v[0] / 10 ** v[1] for m, v in r["tok"].items() if m in STABLE}
    sol = r["d"] + (r["tok"].get(WSOL, [0])[0])
    if own:
        ms = {t[0] for t in own}
    elif toks:
        ms = set(toks)
    else:
        (xfer if abs(r["d"]) > 3_000_000 else other.__setitem__("small", other["small"] + r["d"])) and None
        if abs(r["d"]) > 3_000_000: xfer.append([r["bt"], r["d"], r["signer"]])
        continue
    if len(ms) != 1:
        other["multi"] += sol; continue
    m = ms.pop(); c = coins.setdefault(m, dict(m=m, tr=[], net=0, q=collections.Counter(), ven=set(), prog=""))
    c["net"] += sol
    for k, v in q.items(): c["q"][k] += v
    c["ven"] |= {VEN.get(p, p[:6]) for p in r["progs"] if p in VEN} or {"?" + ",".join(p[:4] for p in r["progs"])}
    if m in r["tok"]: c["prog"] = r["tok"][m][2][:5]
    if own:
        for t in own:
            c["tr"].append(dict(s=r["s"], i=r["i"], bt=r["bt"], buy=t[2], sol=t[3], tok=t[4], vs=t[5], vt=t[6], rs=t[7], tip=r["tip"], v="pump"))
    else:
        dt = r["tok"][m][0]
        c["tr"].append(dict(s=r["s"], i=r["i"], bt=r["bt"], buy=int(dt > 0), sol=abs(sol), tok=abs(dt), tip=r["tip"],
                            v="+".join(sorted(c["ven"])), q=q))
out = []
for c in coins.values():
    c["ven"] = sorted(c["ven"]); c["q"] = dict(c["q"]); out.append(c)
json.dump(dict(coins=out, xfer=xfer, other=dict(other)), open(f"{SP}/h9long/{W[:8]}_coins.json", "w"))
print(len(out), "coins; net", round(sum(c["net"] for c in out) / 1e9, 2), "SOL; transfers", len(xfer), round(sum(x[1] for x in xfer) / 1e9, 2), "other", {k: round(v / 1e9, 3) for k, v in other.items()})
