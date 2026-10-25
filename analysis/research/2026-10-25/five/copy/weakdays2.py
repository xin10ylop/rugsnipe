# Combinations: his tip x the copy's own price bound (copy_max_premium, applied at every landing distance), listed
# signers (and unlisted, high tips), sold at H-1. By day.
import json, sys, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/dec/copy/weakdays.py").read().split("L = [r for r in rows if r[\"listed\"]]")[0]
exec(src)
DAYS = ["10-21", "10-22", "10-23", "10-24", "10-25"]
def show(lab, sel, prem, hold=lambda r: r["H"] - 1):
    global PREM
    PREM = prem
    byd = collections.defaultdict(float); n = 0
    for r in sel:
        vs = []
        for D in dec:
            en = entry_at(r, D)
            if not en: continue
            if (en[0] / en[1]) / r["hisp"] - 1 > PREM: vs.append(-0.0001); continue
            v = value_at(r, en, hold(r))
            if v is not None: vs.append(v)
        if vs: byd[r["day"]] += sum(vs) / len(vs); n += 1
    pos = sum(1 for d in DAYS if byd[d] > 0)
    print(f"   {lab:52} n {n:4} total {sum(byd.values()):+6.2f}  days+ {pos}/5  " + " ".join(f"{byd[d]:+.2f}" for d in DAYS))
L = [r for r in rows if r["listed"]]; U = [r for r in rows if not r["listed"]]
for tl, tf in (("tip >= 4M", lambda r: r["tip"] >= 4e6), ("tip >= 8M", lambda r: r["tip"] >= 8e6), ("buy >= 0.7 SOL", lambda r: r["sol"] >= 0.7),
               ("tip >= 4M or buy >= 0.7 SOL", lambda r: r["tip"] >= 4e6 or r["sol"] >= 0.7)):
    print(f"listed, {tl}:")
    for prem in (0.10, 0.05, 0.03, 0.02):
        show(f"bound +{prem:.0%}", [r for r in L if tf(r)], prem)
print("unlisted, high tips (sold at +5):")
for t in (8e6, 12e6, 20e6):
    show(f"tip >= {t/1e6:.0f}M, bound +10%", [r for r in U if r["tip"] >= t], 0.10, lambda r: 5)
