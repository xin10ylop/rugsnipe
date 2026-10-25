# On the coins his keyed follow could not know (new wallets, never-taken wallets, no bundle), Oct 23 12-24 + Oct 24
# 00-12: which outside wallets bought in the create slot (before any +1 buy of his), on his picks vs the coins he skipped?
# A wallet he copies would show up far more often on his picks.
import os, json, collections
__file__ = os.path.abspath("final.py")
src = open("final.py").read().rsplit('if __name__ == "__main__":', 1)[0]
exec(compile(src, "final5", "exec"))
cat = json.load(open(SP + "/dec/cell/cat.json"))
UNK = ("new wallets", "known, he never took", "no bundle wallets", "wallet known, other fee routing")
pick = collections.Counter(); skip = collections.Counter(); npk = nsk = 0
for m, c in cat.items():
    r = D[m]
    if c not in UNK or r["per"] == "23a" or cls(r) == "low": continue
    p = None
    for d in ("full", "fp"):
        q = f"{SP}/{d}/{m}.json"
        if os.path.exists(q): p = q; break
    if not p: continue
    t = json.load(open(p)); cc = cr[m]; cs = cc["slot"]
    cset = {cc["creator"], cc["user"], cc["signer"]} | set(BW[m]["bw"])
    ws = set()
    for s, i, sg, evs in t["rows"]:
        if s != cs: continue
        for e in evs:
            if e[1] and e[0] not in cset and e[0] != A: ws.add(e[0])
    if r["he"]: npk += 1; pick.update(ws)
    else: nsk += 1; skip.update(ws)
print("his picks", npk, "skipped", nsk)
rows = []
for w, k in pick.items():
    if k < 5: continue
    rows.append((k / npk, skip[w] / max(1, nsk), k, skip[w], w))
rows.sort(key=lambda x: -(x[0] / max(x[1], 1e-4)))
for a, b, k, ks, w in rows[:15]:
    print(f"   {w[:8]} in {100*a:5.1f}% of his picks ({k}) vs {100*b:5.2f}% of skips ({ks}) lift {a/max(b,1e-4):6.1f}")
