# scratch.py's copy of him (5 days, our measured landing distances in his slot, sold one slot before the table hold or
# create + 9) at wider price limits: does the 5% limit cut his winners?
import os, collections
src = open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "scratch.py")).read().split("for sl, sel in")[0]
src = src.replace('PREM = float(__import__("os").environ.get("PREM", "0.10"))', "PREM = 0.05")
import io, contextlib
with contextlib.redirect_stdout(io.StringIO()):
    exec(src)
for lab, sel in (("his tip >= 2M (the live key)", [r for r in rows if r["tip"] >= 2e6]), ("all his launch buys", rows)):
    print(f"== {lab}: {len(sel)} coins")
    for P in (0.05, 0.10, 0.15, 0.25, 9.0):
        PREM = P
        tot = 0.0; n = 0; byd = collections.defaultdict(float); refused = 0
        for r in sel:
            vs = [one(r, D, "anticipate", 0.5) for D in dec]
            vs = [v for v in vs if v is not None]
            if not vs: continue
            refused += sum(1 for v in vs if v == -0.0001) / len(vs)
            v = sum(vs) / len(vs); tot += v; n += 1; byd[r["day"]] += v
        print(f"   limit {'none' if P > 1 else f'{P:.0%}':>5}: n {n} refused {refused:5.1f}  total {tot:+6.2f}  by day " + " ".join(f"{k} {v:+.2f}" for k, v in sorted(byd.items())))
