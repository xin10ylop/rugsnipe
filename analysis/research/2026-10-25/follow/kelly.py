# Flat 0.5 SOL against 0.5 on hot followed coins and less on the rest, for a 0.55 SOL bankroll: expected profit and
# compounded growth (mean log of the bankroll per trade), from the out-of-sample per-coin values (live policy, net 1.2%)
# of the followed coins (K 2, R 0.3), split by the cell's last 3 coins at +10 (>= +10% = hot).
import os, json, math, random
__file__ = os.path.abspath("final.py")
exec(compile(open("cell/evval.py").read().split('for T in (0.05, 0.10, 0.20):')[0], "evval", "exec"))
fol = run_rules(T=0.10)
hot, cold = [], []
for m, (a, v) in fol.items():
    r = D[m]
    if not a or r["per"] == "23a" or cls(r) == "low" or (BW[m]["pb"] or 99) > 40: continue
    x = pol(m)
    if x is None: continue
    (hot if v else cold).append(x - 0.012)
print(f"hot {len(hot)} mean {100*sum(hot)/len(hot):+.2f}% | cold {len(cold)} mean {100*sum(cold)/len(cold):+.2f}%")
B = 0.55
seq = [(1, x) for x in hot] + [(0, x) for x in cold]
for h_st, c_st in ((0.5, 0.5), (0.5, 0.4), (0.5, 0.33), (0.5, 0.25), (0.4, 0.27), (0.33, 0.33)):
    ev = sum((h_st if h else c_st) * x for h, x in seq)
    g = sum(math.log(max(1e-9, 1 + (h_st if h else c_st) / B * x)) for h, x in seq) / len(seq)
    # a path: random order, the bankroll compounding with stakes capped at their SOL size and at the bankroll
    random.seed(1); ends = []
    for _ in range(400):
        s = seq[:]; random.shuffle(s); b = B
        for h, x in s:
            st = min(h_st if h else c_st, b) * min(1.0, b / B)
            b += st * x
            if b < 0.05: break
        ends.append(b)
    ends.sort()
    print(f"   hot {h_st:.2f} cold {c_st:.2f}: profit {ev:+.2f} SOL over {len(seq)} trades | growth {1e4*g:+.1f} bp a trade | "
          f"bankroll after them (stakes scale with it) median {ends[200]:.2f} p10 {ends[40]:.2f} ruin {sum(1 for e in ends if e < 0.05)}")
