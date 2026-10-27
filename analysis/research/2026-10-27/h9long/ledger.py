# The wallet's trading result in SOL over every venue and quote. Each transaction's legs: SOL (native + wSOL), USD
# (USDC / USDT / USD1) and tokens. Prices: SOL per USD and SOL per token from the transactions that swap one token
# against SOL or USD; a token-for-token swap (a coin sold for its quote token, e.g. a pump coin priced in another
# token) is valued at the received token's nearest price. Per token, average-cost P&L in SOL: realized on sells,
# the rest marked at its last price (open). -> <w8>_ledger.json   python3 -I ledger.py WALLET
import sys, json, bisect, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = sys.argv[1]
WSOL = "So11111111111111111111111111111111111111112"
USD = {"EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
       "USD1ttGY1N17NEEHLmELoaybftRBUSErhqYiQzvEmuB"}
rows = sorted((json.loads(l) for l in open(f"{SP}/h9long/{W[:8]}.jsonl")), key=lambda r: (r["s"], r["i"] or 0))
def legs(r):
    sol = r["d"] + r["tok"].get(WSOL, [0])[0]
    usd = sum(v[0] / 10 ** v[1] for m, v in r["tok"].items() if m in USD)
    tk = {m: v[0] / 10 ** v[1] for m, v in r["tok"].items() if m != WSOL and m not in USD}
    return sol / 1e9, usd, tk
# price series
def near(ser, t):
    if not ser: return None
    ts = [x[0] for x in ser]; k = bisect.bisect_left(ts, t)
    c = [ser[j] for j in (k - 1, k) if 0 <= j < len(ser)]
    return min(c, key=lambda x: abs(x[0] - t))[1]
usdp = []                     # (bt, SOL per USD)
usdq = []                     # (bt, mint, USD per token)
tokp = collections.defaultdict(list)   # mint -> [(bt, SOL per token)]
for r in rows:
    sol, usd, tk = legs(r)
    fee = (r["fee"] or 0) / 1e9 + (r["tip"] or 0) / 1e9 if r["signer"] else 0
    s = sol + fee
    if not tk and abs(usd) > 1 and s * usd < 0: usdp.append((r["bt"], -s / usd))
    if len(tk) == 1:
        (m, q), = tk.items()
        if q and abs(s) > 0.01 and s * q < 0 and abs(usd) < 0.01: tokp[m].append((r["bt"], -s / q))
        elif q and abs(usd) > 1 and usd * q < 0 and abs(s) < 0.02: usdq.append((r["bt"], m, -usd / q))
usdp.sort()
for t, m, pu in usdq:
    if near(usdp, t): tokp[m].append((t, pu * near(usdp, t)))
def solusd(t): return near(usdp, t) or 0.0
for m in tokp: tokp[m].sort()
# ledger
pos = collections.defaultdict(lambda: dict(q=0.0, cost=0.0, real=0.0, pre=0.0, buys=0, sells=0, xfer=0, tr=[]))
tin = []
unpriced = collections.Counter(); xfer = []
for r in rows:
    sol, usd, tk = legs(r)
    fee = ((r["fee"] or 0) + (r["tip"] or 0)) / 1e9 if r["signer"] else 0
    vusd = usd * solusd(r["bt"])
    if not tk:
        if abs(usd) < 0.01 and abs(sol) > 0.003: xfer.append([r["bt"], sol, r["signer"]])
        continue
    if abs(sol + vusd) < 0.001 and len(tk) == 1:   # tokens moved without payment: a transfer, not a trade
        (m, q), = tk.items(); tin.append([r["bt"], m, q]); pos[m]["xfer"] += 1
        if q < 0 and pos[m]["q"] > 0:             # sent away: drop that much at cost (no P&L)
            out = min(-q, pos[m]["q"]); pos[m]["cost"] -= pos[m]["cost"] / pos[m]["q"] * out; pos[m]["q"] -= out
        continue
    # value each token leg in SOL: the SOL/USD legs pay for the tokens; with several token legs, price the others
    cash = sol + vusd + fee                        # SOL value moved against the tokens (fees counted after)
    vals = {}
    if len(tk) == 1:
        (m, q), = tk.items(); vals[m] = -cash
    else:
        known = {m: (near(tokp[m], r["bt"]) or 0) * q for m, q in tk.items()}
        # the leg we can't price absorbs the rest (a coin sold for a quote token takes the quote's value)
        unk = [m for m in tk if not tokp[m]]
        if len(unk) == 1:
            for m in tk:
                vals[m] = known[m] if m != unk[0] else -cash - sum(known[x] for x in tk if x != unk[0])
        else:
            for m in tk: vals[m] = known[m]
            if unk: unpriced[",".join(sorted(x[:6] for x in unk))] += 1
            tot = sum(vals.values()); 
            if tot and abs(-cash - tot) > 1e-9:  # spread the cash residual by value
                for m in tk: vals[m] += (-cash - tot) * (abs(vals[m]) / sum(abs(v) for v in vals.values()) if sum(abs(v) for v in vals.values()) else 1 / len(tk))
    share = {m: abs(vals[m]) for m in tk}; st = sum(share.values()) or 1
    for m, q in tk.items():
        p = pos[m]; v = vals[m]; f = fee * share[m] / st
        if q > 0:                                   # bought q tokens for v SOL of value (+ its share of the fee)
            p["q"] += q; p["cost"] += v + f; p["buys"] += 1
        else:                                       # sold -q tokens for -v
            avg = p["cost"] / p["q"] if p["q"] > 1e-12 else 0
            out = min(-q, p["q"]) if p["q"] > 0 else 0
            k = out / -q                              # the share of this sale that closes what we saw it buy
            p["real"] += (-v - f) * k - avg * out; p["pre"] += (-v - f) * (1 - k)
            p["cost"] -= avg * out; p["q"] -= out; p["sells"] += 1
        p["tr"].append([r["s"], r["bt"], int(q > 0), round(v, 6), q])
res = []
for m, p in pos.items():
    last = tokp[m][-1][1] if tokp[m] else None
    opn = (p["q"] * last - p["cost"]) if (last is not None and p["q"] > 1e-9) else (-p["cost"] if p["q"] > 1e-9 else 0)
    res.append(dict(m=m, real=p["real"], open=opn, pre=p["pre"], q=p["q"], buys=p["buys"], sells=p["sells"], xfer=p["xfer"], first=p["tr"][0][1] if p["tr"] else None, tr=p["tr"]))
res.sort(key=lambda x: -(x["real"] + x["open"]))
json.dump(dict(pos=res, xfer=xfer, tin=tin, usdp=usdp[-5:]), open(f"{SP}/h9long/{W[:8]}_ledger.json", "w"))
R = sum(x["real"] for x in res); O = sum(x["open"] for x in res)
print(f"tokens {len(res)}  realized {R:+.2f}  open {O:+.2f}  sold from before the start {sum(x['pre'] for x in res):+.2f}  token transfers {len(tin)}  SOL/USD last {usdp[-1][1] if usdp else None}  transfers {len(xfer)} net {sum(x[1] for x in xfer):+.2f}")
print("unpriced", unpriced.most_common(5))
for x in res[:15] + res[-6:]:
    print(f"  {x['m'][:8]}  real {x['real']:+8.3f}  open {x['open']:+7.3f}  buys {x['buys']:3d} sells {x['sells']:3d}")

# accounting check: realized + open + sold-from-before must equal the cash the trades moved (SOL + USD, fees in)
cash = 0.0
for r in rows:
    sol, usd, tk = legs(r)
    if tk: cash += sol + usd * solusd(r["bt"])
print(f"check: cash moved by trading {cash:+.2f}; open positions at cost {-sum(min(0, 0) for _ in []) + sum(p['cost'] for p in pos.values() if p['q'] > 1e-9):.2f}")
