# Routability of wallet 4's buys from its transaction rows (H9kuqGKN.jsonl): A multihop from SOL/wSOL, B multihop
# from WBTC, C direct curve buy of a WBTC-quoted coin, E the rest. Checked against routable.json (598 big buys).
import json, collections, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/h9long"
W = "H9kuqGKNYgPFYCA17uUTkaqXGum7MK8y1GwbS7cxsjDg"
WSOL, WBTC = "So11111111111111111111111111111111111111112", "3NZ9JMVBmGAqocybic2c7LQCJScmgsAZ6vQqTDzcqmJh"
PAMM, JUP = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA", "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4"
rows = [json.loads(l) for l in open(f"{SP}/H9kuqGKN.jsonl")]
by = {(t[0], r["s"], t[2]): r for r in rows for t in r["tr"] if t[1] == W}
def cls(x):
    m = x["m"]; b = x["tr"][0]; r = by.get((m, b["s"], 1))
    if not r: return "?"
    # the token the route starts from: the one besides the coin and wSOL the transaction touched (a Jupiter leg buys
    # it and the wallet keeps a tenth, so its balance can rise); none = SOL
    others = [k for k in r["tok"] if k not in (m, WSOL)]
    if PAMM in r["progs"]:
        return "B" if WBTC in others else ("E" if others else "A")
    return "C" if WBTC in others else "E"
if __name__ == "__main__":
    T = json.load(open(f"{SP}/H9kuqGKN_trips.json"))
    R = json.load(open(f"{SP}/routable.json"))
    c = collections.Counter()
    for x in T:
        if x["m"] in R: c[(R[x["m"]][0], cls(x))] += 1
    for k in sorted(c): print(k, c[k])
    ok = sum(n for (a, b), n in c.items() if (a in "ABC") == (b in "ABC"))
    print(f"routable or not agrees on {ok} of {sum(c.values())}")
