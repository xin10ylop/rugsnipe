# His execution on the coins he bought, Oct 23 12-24 + Oct 24 00-12, by the cell's state (followed / hot): the buy tip,
# tip / stake, the entry slot (create + 0 or + 1) and the hold, from his trips (month/AHwJ_T.json).
import os, json, collections, statistics as stt
__file__ = os.path.abspath("final.py")
exec(compile(open("cell/evval.py").read().split('for T in (0.05, 0.10, 0.20):')[0], "evval", "exec"))
fol = run_rules(T=0.10)
T = {t["mint"]: t for t in M}
G = collections.defaultdict(list)
for m, (a, v) in fol.items():
    if m not in T or D[m]["per"] == "23a": continue
    t = T[m]; g = ("followed" if a else "not followed") + (", hot" if v else ", cold")
    G[g].append(t)
for g, ts in sorted(G.items()):
    tips = [t["tip"] for t in ts]; ratio = [t["tip"] / 1e9 / t["stake"] for t in ts if t["stake"] > 0]
    off = collections.Counter(t["slot_off"] for t in ts); hold = [t["hold"] for t in ts]
    print(f"{g:22s} trades {len(ts):4d} | stake median {stt.median(t['stake'] for t in ts):.3f} | tip median {stt.median(tips)/1e6:.2f}M "
          f"tip/stake median {100*stt.median(ratio):.2f}% | entry +0 {100*off[0]/len(ts):.0f}% +1 {100*off[1]/len(ts):.0f}% | hold median {stt.median(hold)} mean {stt.mean(hold):.1f}")
allt = [t for t in M if t["stake"] > 0.1]
r = collections.Counter(round(t["tip"] / 1e9 / t["stake"] * 1000, 1) for t in allt)
print("tip / stake (per mille) over his month:", r.most_common(8))
