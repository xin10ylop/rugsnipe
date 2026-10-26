# The screen's wallets (screen2.json) under filters the bot could apply to each wallet, as it does to him: their buy
# size, their own repeat on the operator (>= 2 of their launch buys on the create signer in the previous 24 h, counted
# live from their earlier rows), both; copy sold at their learned modal hold - 1 (vm) or + 9 (v9). By day; the coins he
# did not buy separately (the volume a second wallet adds).
import json, collections, bisect, sys
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
best = []
for w, rows in S.items():
    if w == A: continue
    rep(rows)
    for fn, f in F.items():
        for k in ("vm", "v9"):
            sel = [r for r in rows if f(r) and r[k] is not None]
            if len(sel) < 15: continue
            byd = collections.defaultdict(float)
            for r in sel: byd[r["day"]] += r[k]
            nh = [r for r in sel if not r["his"]]
            best.append(dict(w=w, f=fn, k=k, n=len(sel), tot=sum(byd.values()), pos=sum(1 for d in DAYS if byd[d] > 0), byd=byd,
                             nh=len(nh), nhv=sum(r[k] for r in nh), mn=min(byd[d] for d in DAYS)))
# a wallet's best filter must hold on every day (no peeking at the total alone)
best.sort(key=lambda o: (-(o["pos"] == 3), -o["tot"]))
print(f"{'wallet':9} {'filter':16} {'exit':4} {'copies':>6} {'total':>6} {'d+':>3} {'worst day':>9} {'not his':>7} {'value':>6}  by day")
seen = collections.Counter()
for o in best:
    if seen[o["w"]] >= 2: continue
    if o["tot"] <= 0: continue
    seen[o["w"]] += 1
    print(f"{o['w'][:8]:9} {o['f']:16} {o['k']:4} {o['n']:6d} {o['tot']:+6.2f} {o['pos']:2d}  {o['mn']:+9.2f} {o['nh']:7d} {o['nhv']:+6.2f}  " + " ".join(f"{o['byd'][d]:+.2f}" for d in DAYS))
    if sum(seen.values()) >= 40: break
