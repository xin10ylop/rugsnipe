# A wider limit (7.5% / 10%) only when a condition known before we send holds. The 5-day copy of him (tip >= 2M, our
# measured landing distances, sold one slot before the table hold), per copy: what 7.5% and 10% add over 5% (the refused
# landings that would fill). Conditions: his tip, his size, create + 0/1, his block index, the price level his buy left
# (virtual SOL), the launch site, listed signer. Then the honest test: pick the best condition on Oct 21-23, score it
# on Oct 24-25.
import collections, io, contextlib, json
src = open("scratch.py").read().split("for sl, sel in")[0].replace('PREM = float(__import__("os").environ.get("PREM", "0.10"))', "PREM = 0.05")
with contextlib.redirect_stdout(io.StringIO()):
    exec(src)
sel = [r for r in rows if r["tip"] >= 2e6]
def v(r, P):
    global PREM
    PREM = P
    vs = [one(r, D, "anticipate", 0.5) for D in dec]; vs = [x for x in vs if x is not None]
    return sum(vs) / len(vs) if vs else None
X = []
for r in sel:
    a = v(r, 0.05)
    if a is None: continue
    b = his_buy[r["m"]]; d = coins[r["m"]]
    f = b["fill"]; vsol = (f[2] if f[2] > 0 else K // f[3]) / 1e9
    X.append(dict(day=r["day"], d75=v(r, 0.075) - a, d10=v(r, 0.10) - a,
                  tip=r["tip"] / 1e6, sol=r["sol"], at=b["slot"] - r["cs"], idx=b["idx"], vsol=vsol,
                  host=(d["create"].get("host") or "?"), listed=r["listed"]))
json.dump(X, open(SP + "/dec/copy/condlimit.json", "w"))
CONDS = {
    "always": lambda x: True,
    "tip >= 8M": lambda x: x["tip"] >= 8, "tip >= 12M": lambda x: x["tip"] >= 12, "tip >= 20M": lambda x: x["tip"] >= 20,
    "tip < 8M": lambda x: x["tip"] < 8,
    "size >= 1.1": lambda x: x["sol"] >= 1.1, "size >= 1.5": lambda x: x["sol"] >= 1.5, "size < 0.7": lambda x: x["sol"] < 0.7,
    "create + 0": lambda x: x["at"] == 0, "create + 1": lambda x: x["at"] == 1,
    "his index < 200": lambda x: x["idx"] < 200, "his index >= 400": lambda x: x["idx"] >= 400,
    "vSOL < 36": lambda x: x["vsol"] < 36, "vSOL 36-45": lambda x: 36 <= x["vsol"] < 45, "vSOL >= 45": lambda x: x["vsol"] >= 45,
    "listed": lambda x: x["listed"], "not listed": lambda x: not x["listed"],
    "j7tracker host": lambda x: "j7tracker" in x["host"], "other host": lambda x: "j7tracker" not in x["host"],
    "tip >= 12M and size >= 1.1": lambda x: x["tip"] >= 12 and x["sol"] >= 1.1,
    "tip >= 8M and create + 0": lambda x: x["tip"] >= 8 and x["at"] == 0,
    "tip >= 8M and vSOL < 45": lambda x: x["tip"] >= 8 and x["vsol"] < 45,
}
days = sorted({x["day"] for x in X})
def score(xs, key):
    byd = collections.defaultdict(float)
    for x in xs: byd[x["day"]] += x[key]
    return sum(byd.values()), byd
print(f"copies {len(X)}; days {days}")
for key, lab in (("d75", "7.5%"), ("d10", "10%")):
    print(f"\n== the limit at {lab} when the condition holds (what it adds over 5%)")
    res = []
    for c, fn in CONDS.items():
        xs = [x for x in X if fn(x)]
        tot, byd = score(xs, key)
        pos = sum(1 for d in days if byd[d] > 0)
        res.append((tot, c, len(xs), byd, pos))
    for tot, c, n, byd, pos in sorted(res, reverse=True):
        print(f"   {c:28} copies {n:3d}  adds {tot:+.3f}  days positive {pos}/5  " + " ".join(f"{d[3:]} {byd[d]:+.3f}" for d in days))
# out of sample: pick on Oct 21-23, score on Oct 24-25
print("\n== picked on Oct 21-23, scored on Oct 24-25")
for key, lab in (("d75", "7.5%"), ("d10", "10%")):
    ins = [x for x in X if x["day"] <= "10-23"]; oos = [x for x in X if x["day"] >= "10-24"]
    best = max(CONDS, key=lambda c: score([x for x in ins if CONDS[c](x)], key)[0])
    a = score([x for x in ins if CONDS[best](x)], key)[0]; b = score([x for x in oos if CONDS[best](x)], key)[0]
    print(f"   {lab}: best on Oct 21-23 = '{best}' ({a:+.3f}); on Oct 24-25 it adds {b:+.3f}")
