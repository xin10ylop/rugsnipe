# A copy as a reaction measured in block positions: we see his buy and our transaction lands D transaction indexes
# after his in the same block (the leader packs it D positions later; past the block's last trade of the mint it fills
# at the end of his slot). Entry = the mint's state after every trade at index <= his + D; bound at his price + 10%;
# sold at the operator table hold H - 1 (H = his hold for the signer; else 10), or H. All his landed buys at create+0/1.
import json, sys, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/dec/copy/copyval.py").read().split("def val(")[0]
sys.argv = ["x", "0.0", "10"]
exec(src)
def entry_at(r, D):
    b = his_buy[r["m"]]
    xs = [x for x in r["seq"] if (x[0], x[1]) <= (b["slot"], b["idx"] + D) and x[0] <= b["slot"]]
    return (xs[-1][2], xs[-1][3]) if xs else None
def val2(r, en, h):
    if not en: return None
    if (en[0] / en[1]) / r["hisp"] - 1 > PREM: return -FAIL
    ex = exit_at(r["seq"], r["cs"], h, r["s"])
    return pnl(en, ex) if ex else None
for lab, sel in (("Oct 23+, coins the newest bot does not fire", [r for r in rows if r["bt"] >= T0 and not r["fired"]]),
                 ("all 5 days, all his landed buys", rows)):
    for mn in (0.0, 0.7):
        s2 = [r for r in sel if his_buy[r["m"]]["fill"][0] / 1e9 >= mn]
        print(f"\n== {lab}, his buy >= {mn} SOL: {len(s2)} coins, his net {sum(r['his'] for r in s2):+.2f} SOL")
        for D in (1, 5, 20, 50, 100, 200, 400, 10**6):
            for hl, hf in (("H-1", lambda r: r["H"] - 1),):
                xs = [v for v in (val2(r, entry_at(r, D), hf(r)) for r in s2) if v is not None]
                byd = collections.defaultdict(float)
                for r in s2:
                    v = val2(r, entry_at(r, D), hf(r))
                    if v is not None: byd[r["day"]] += v
                summ(f"D = {D if D < 10**6 else 'end of slot':>11} positions behind, sell {hl}", xs)
                print("        by day: " + "  ".join(f"{k} {v:+.2f}" for k, v in sorted(byd.items())))
