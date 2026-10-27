# Its other-quote coins by quote token (the token it spent on the buy or got on the sell; wSOL both ways = via SOL),
# its size and buy route, by period.   python3 -I byquote.py WALLET
import sys, json, collections, datetime
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = sys.argv[1]; WSOL = "So11111111111111111111111111111111111111112"
rows = sorted((json.loads(l) for l in open(f"{SP}/h9long/{W[:8]}.jsonl")), key=lambda r: (r["s"], r["i"] or 0))
T = {x["m"]: x for x in json.load(open(f"{SP}/h9long/{W[:8]}_trips.json"))}
NAMES = {"CARDSccU": "CARDS", "pumpCmXq": "PUMP", "3NZ9JMVB": "WBTC", "TTWofwAg": "TTWO", "WXMRyRZh": "WXMR", "A7bdiYdS": "A7bd",
         "SPCXxcqX": "SPCX", "98sMhvDw": "98sM", "Pren1FvF": "Pren", "DJTu7vi8": "DJTu", "XsoCS1Tf": "XsoC", "7vfCXTUX": "WETH"}
quote = {}
for r in rows:
    coin = [m for m in r["tok"] if m in T and T[m]["kind"] == "pump, other quote"]
    if len(coin) != 1: continue
    c = coin[0]
    for m, v in r["tok"].items():
        if m == c or m == WSOL: continue
        if (v[0] > 0) != (r["tok"][c][0] > 0):        # the opposite leg of the coin's
            quote.setdefault(c, NAMES.get(m[:8], m[:8]))
def per(t): return "P1" if datetime.datetime.fromtimestamp(t, datetime.timezone.utc).day <= 22 else "P2"
g = collections.defaultdict(lambda: collections.defaultdict(list))
for m, x in T.items():
    if x["kind"] != "pump, other quote" or x["open"] or not x["tr"]: continue
    g[quote.get(m, "via SOL only")][per(x["tr"][0]["bt"])].append(x["net"])
print("quote          P1 coins   net     P2 coins   net")
for q in sorted(g, key=lambda q: -sum(sum(v) for v in g[q].values())):
    a, b = g[q]["P1"], g[q]["P2"]
    print(f"  {q:12s} {len(a):6d} {sum(a):+8.2f}   {len(b):6d} {sum(b):+8.2f}")
json.dump(quote, open(f"{SP}/h9long/{W[:8]}_quote.json", "w"))
