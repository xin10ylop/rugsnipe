# Diversity of the copy set (screen2.json rows; a wallet's rows pass its chosen filter and exit, given on the command
# line as WALLET[:8]=FILTER:EXIT, filters as in variants.py):
#   per wallet: how much of its copy profit its top operator carries, the operators it needs, its coins shared with him
#   and with each other wallet of the set;
#   the set as one bot: one 0.5 SOL copy per coin (the first wallet to buy it), by day, against the sum of the wallets.
#   python3 -I wallets/diversity.py 3yrJE1A8=all:vm 9su3UPQW=size>=0.5:v9 ...
import json, collections, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"
S = json.load(open(SP + "/wallets/screen2.json"))
DAYS = ["10-23", "10-24", "10-25"]
def rep(rows):
    t = collections.defaultdict(list)
    for r in sorted(rows, key=lambda r: r["bt"]):
        v = t[r["signer"]]; r["rep"] = sum(1 for x in v if x >= r["bt"] - 86400); v.append(r["bt"])
F = {"all": lambda r: True, "size>=0.5": lambda r: r["size"] >= 0.5, "size>=1": lambda r: r["size"] >= 1.0,
     "op>=2": lambda r: r["rep"] >= 2, "op>=2,size>=0.5": lambda r: r["rep"] >= 2 and r["size"] >= 0.5}
full = {w[:8]: w for w in S}
sel = {}
for a in sys.argv[1:]:
    k, spec = a.split("=", 1); f, ex = spec.split(":")
    w = full[k]; rows = S[w]; rep(rows)
    sel[w] = [dict(r, v=r[ex], w=w) for r in rows if F[f](r) and r[ex] is not None]
his_rows = S.get(A, [])
his_coins = {r["m"] for r in his_rows}
print("== each wallet")
for w, rows in sel.items():
    by_op = collections.defaultdict(float)
    for r in rows: by_op[r["signer"]] += r["v"]
    tot = sum(by_op.values()); top = max(by_op.items(), key=lambda kv: kv[1]) if by_op else (None, 0)
    pos_ops = sorted((v for v in by_op.values() if v > 0), reverse=True)
    need = 0; acc = 0.0
    for v in pos_ops:
        acc += v; need += 1
        if acc >= 0.5 * sum(pos_ops): break
    sh = sum(1 for r in rows if r["m"] in his_coins)
    print(f"  {w[:8]} copies {len(rows):4d} total {tot:+6.2f}  operators {len(by_op):3d}, top one {top[1]:+5.2f} ({(top[1] / tot if tot > 0 else 0):.0%} of the total), "
          f"half the gains from {need} operator(s); shared with him {sh}")
ws = list(sel)
print("== shared coins between the wallets")
for i, a in enumerate(ws):
    ca = {r["m"] for r in sel[a]}
    print("  " + a[:8] + ": " + ", ".join(f"{b[:8]} {len(ca & {r['m'] for r in sel[b]})}" for b in ws if b != a))
print("== the set as one bot (one copy per coin, the first wallet to buy it)")
allr = sorted((r for rows in sel.values() for r in rows), key=lambda r: (r["bt"], r["m"]))
first = {}
for r in allr:
    first.setdefault(r["m"], r)
byd = collections.defaultdict(float); bys = collections.defaultdict(float)
for r in first.values(): byd[r["day"]] += r["v"]
for r in allr: bys[r["day"]] += r["v"]
print(f"  one per coin: {len(first)} copies, {sum(byd.values()):+.2f} SOL  " + " ".join(f"{d} {byd[d]:+.2f}" for d in DAYS))
print(f"  sum of the wallets: {len(allr)} copies, {sum(bys.values()):+.2f} SOL")
nh = [r for r in first.values() if r["m"] not in his_coins]
print(f"  of which coins he did not buy: {len(nh)} copies, {sum(r['v'] for r in nh):+.2f} SOL")
