# Same copy speed, different coins: how far the price has moved by the time a copy lands (D positions after his buy),
# and what it does after, on his listed vs unlisted signers (his 0.7 SOL+ landed buys at create + 0/1, 5 days).
import json, sys, statistics as st
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/dec/copy/exitopt.py").read().split("for sl, sel in")[0]
exec(src)
def px(s): return s[0] / s[1]
for lab, sel in (("listed", [r for r in rows if r["listed"] and r["sol"] >= 0.7]), ("unlisted", [r for r in rows if not r["listed"] and r["sol"] >= 0.7])):
    up = {D: [] for D in (10, 40, 90, 200)}; later = []; bought_after = []
    for r in sel:
        b = his_buy[r["m"]]
        for D in up:
            e = entry_at(r, D)
            if e: up[D].append(px(e) / r["hisp"] - 1)
        e = entry_at(r, 90)
        h = (r["H"] - 1) if r["listed"] else 5
        ex = exit_at(r["seq"], r["cs"], h, r["s"])
        if e and ex: later.append(px(ex) / px(e) - 1)
        bought_after.append(sum(x[6] / 1e9 for x in r["seq"] if x[0] == b["slot"] and b["idx"] < x[1] <= b["idx"] + 90 and x[5] == 1 and x[4] != A))
    print(f"{lab:8} {len(sel)} coins: price over his fill when a copy lands, median at D=10 {100*st.median(up[10]):+.1f}%  D=40 {100*st.median(up[40]):+.1f}%  "
          f"D=90 {100*st.median(up[90]):+.1f}% (mean {100*st.mean(up[90]):+.1f}%)  D=200 {100*st.median(up[200]):+.1f}%")
    print(f"{'':8} SOL others buy in the 90 positions after him: median {st.median(bought_after):.2f}, mean {st.mean(bought_after):.2f}; "
          f"price from a D=90 entry to the exit ({'H-1' if lab == 'listed' else '+5'}): median {100*st.median(later):+.1f}%, mean {100*st.mean(later):+.1f}%")
