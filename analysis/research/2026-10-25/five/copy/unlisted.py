# Copies of his landed buys on UNLISTED signers (not in his operator table): his own result there, his sell timing, and
# exit rules for a copy: short flat holds, his modal first-sell offset per launch tool (uri host) or per dev-buy bucket
# learned from his earlier coins (ex ante) minus 1, and the entry distance's share of the loss (D = 0 / 1 vs ours).
import json, sys, collections, statistics as st
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/dec/copy/exitopt.py").read().split("for sl, sel in")[0]
exec(src)
un = [r for r in rows if not r["listed"]]; ls = [r for r in rows if r["listed"]]
print(f"his landed buys at create+0/1: listed {len(ls)} (his net {sum(r['his'] for r in ls):+.2f} SOL), unlisted {len(un)} (his net {sum(r['his'] for r in un):+.2f} SOL)")
for lab, sel in (("listed", ls), ("unlisted", un)):
    print(f"  {lab}: his first sell offset {collections.Counter(min(r['soff'] or 99, 99) for r in sel).most_common(8)}")
for r in rows:
    c = coins[r["m"]]["create"]
    r["host"] = c.get("host") or "-"
    dev = None
    for x in r["seq"]:
        if x[0] == r["cs"] and x[1] == c["idx"]: dev = x; break
    r["devb"] = "?" if not dev else ("<1" if 0 else None)
def dev_sol(r):
    c = coins[r["m"]]["create"]
    xs = [x for x in coins[r["m"]]["rows"] if x[0] == r["cs"] and x[1] == c["idx"] and x[3] == 1]
    return sum(x[4] for x in xs) / 1e9 if xs else 0.0
for r in rows:
    d = dev_sol(r); r["devb"] = "0" if d < 0.05 else "<1" if d < 0.95 else "1" if d < 1.5 else "2-3" if d < 3.5 else "4-6" if d < 6.5 else "7+"
# ex ante modal first-sell offset per key, from his earlier coins (any signer)
def learner(keyf):
    hist = collections.defaultdict(list); out = {}
    for r in sorted(rows, key=lambda r: his_buy[r["m"]]["bt"]):
        k = keyf(r); h = hist[k]
        out[r["m"]] = collections.Counter(h[-20:]).most_common(1)[0][0] if len(h) >= 3 else None
        if r["soff"] is not None: h.append(r["soff"])
    return out
by_host = learner(lambda r: r["host"]); by_dev = learner(lambda r: r["devb"]); by_hd = learner(lambda r: (r["host"], r["devb"]))
for sl, sel in (("unlisted, his buy >= 0.7 SOL", [r for r in un if r["sol"] >= 0.7]), ("unlisted, all his buys", un)):
    print(f"\n== {sl}: {len(sel)} copies, his net {sum(r['his'] for r in sel):+.2f} SOL")
    for h in (1, 2, 3, 4, 5):
        run(f"flat +{h}", sel, lambda r, h=h: h)
    run("his modal sell per launch host - 1 (else +5)", sel, lambda r: (by_host[r["m"]] - 1) if by_host.get(r["m"]) else 5)
    run("his modal sell per dev-buy bucket - 1 (else +5)", sel, lambda r: (by_dev[r["m"]] - 1) if by_dev.get(r["m"]) else 5)
    run("his modal sell per host x dev bucket - 1 (else +5)", sel, lambda r: (by_hd[r["m"]] - 1) if by_hd.get(r["m"]) else 5)
    run("oracle: one slot before his own first sell", [r for r in sel if r["soff"]], lambda r: r["soff"] - 1)
    # entry distance: D fixed at 0 / 1 (his own position) with the oracle exit and with +5
    for D in (0, 1):
        vs = []
        for r in sel:
            en = entry_at(r, D)
            if not en: continue
            ex = exit_at(r["seq"], r["cs"], 5, r["s"])
            v = pnl_s(en, ex, 0.5) if ex else None
            if v is not None: vs.append(v)
        print(f"   entry D = {D} (as close as he is), sell +5: {sum(vs):+.2f} SOL over {len(vs)}")
