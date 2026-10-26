# Which refused copies (at 5%) are worth filling? The 5-day copy of him (tip >= 2M), per coin: the value at 5% and with no
# limit, the difference split by his tip, his size and how far the price ran before our landing (the premium we'd pay);
# and the value of landing closer behind him (a fixed distance D) at 5%.
import os, collections, io, contextlib
src = open("scratch.py").read().split("for sl, sel in")[0].replace('PREM = float(__import__("os").environ.get("PREM", "0.10"))', "PREM = 0.05")
with contextlib.redirect_stdout(io.StringIO()):
    exec(src)
sel = [r for r in rows if r["tip"] >= 2e6]
def val(r, P, Ds=None):
    global PREM
    PREM = P
    vs = [one(r, D, "anticipate", 0.5) for D in (Ds or dec)]
    vs = [v for v in vs if v is not None]
    return sum(vs) / len(vs) if vs else None
def prem_at(r):
    ps = []
    for D in dec:
        en = entry_at(r, D)
        if en: ps.append((en[0] / en[1]) / r["hisp"] - 1)
    return sum(ps) / len(ps) if ps else None
agg = collections.defaultdict(lambda: [0, 0.0, collections.defaultdict(float)])
for r in sel:
    a, b = val(r, 0.05), val(r, 9.0)
    if a is None or b is None: continue
    d = b - a
    if abs(d) < 1e-9: continue
    p = prem_at(r) or 0
    for key in (("tip", "2-4M" if r["tip"] < 4e6 else "4-8M" if r["tip"] < 8e6 else "8-12M" if r["tip"] < 12e6 else ">=12M"),
                ("size", "<0.7" if r["sol"] < 0.7 else "0.7-1.1" if r["sol"] < 1.1 else ">=1.1"),
                ("premium", "5-10%" if p < 0.10 else "10-20%" if p < 0.20 else ">=20%")):
        x = agg[key]; x[0] += 1; x[1] += d; x[2][r["day"]] += d
print("what filling the refused copies adds (no limit minus 5%), by group:")
for k in sorted(agg):
    n, d, byd = agg[k]
    print(f"   {k[0]:8} {k[1]:8} coins {n:3d}  {d:+6.2f} SOL  by day " + " ".join(f"{dd[3:]} {v:+.2f}" for dd, v in sorted(byd.items())))
print("\nlanding closer behind him (a fixed distance D for every copy), 5% limit, his tip >= 2M:")
for D in (0, 1, 3, 10, 20, 40, 60, 90, 150, 240):
    vs = [val(r, 0.05, [D]) for r in sel]; vs = [v for v in vs if v is not None]
    print(f"   D = {D:3}: {sum(vs):+6.2f} SOL over {len(vs)} copies")
print("   (our measured distances, deciles:", dec, ")")
