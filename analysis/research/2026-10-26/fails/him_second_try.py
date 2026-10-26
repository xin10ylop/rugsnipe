# A second try after a refused copy: we learn the refusal one slot after our buy, so buy again at the end of his slot
# + k (k = 1, 2, 3) at whatever the price is then, under a ceiling of his price + C, and sell at the same exit
# (one slot before the table hold, else create + 9). The 5-day copy of him, tip >= 2M, our landing distances; only the
# copies the 5% limit refused get a second try.
import collections, io, contextlib
src = open("scratch.py").read().split("for sl, sel in")[0].replace('PREM = float(__import__("os").environ.get("PREM", "0.10"))', "PREM = 0.05")
with contextlib.redirect_stdout(io.StringIO()):
    exec(src)
sel = [r for r in rows if r["tip"] >= 2e6]
base = 0.0
refused = []
for r in sel:
    vs = [one(r, D, "anticipate", 0.5) for D in dec]; vs = [v for v in vs if v is not None]
    if not vs: continue
    base += sum(vs) / len(vs)
    share = sum(1 for v in vs if v == -0.0001) / len(vs)
    if share > 0: refused.append((r, share))
print(f"the copy now (5% limit): {base:+.2f} SOL; copies with a refusal: {len(refused)} (refused share {sum(s for _, s in refused):.1f})")
for k in (1, 2, 3):
    for C in (0.10, 0.25, 0.5, 9.0):
        add = 0.0; n = 0; byd = collections.defaultdict(float)
        for r, share in refused:
            b = his_buy[r["m"]]
            en = state_end(r["seq"], b["slot"] + k)
            if not en or (en[0] / en[1]) / r["hisp"] - 1 > C: continue
            ex = exit_at(r["seq"], r["cs"], r["H"] - 1, r["s"])
            if not ex or b["slot"] + k >= r["cs"] + r["H"] - 1: continue
            v = pnl_s(en, ex, 0.5)
            if v is None: continue
            add += share * v; n += 1; byd[r["day"]] += share * v
        print(f"   second try at his slot + {k}, ceiling {'none' if C > 1 else f'+{C:.0%}':>5}: tries {n:3d}  adds {add:+.2f} SOL  by day "
              + " ".join(f"{d[3:]} {v:+.2f}" for d, v in sorted(byd.items())))
