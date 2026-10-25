# Every rule of the live copy, one at a time: the 5-day backtest (Oct 21-25, his landed launch buys with a tape, our
# measured landing distances behind him, 0.5 SOL, 3M buy tip, 0.5M sell tip, 1.25% fee each way) with the rule as live
# and with it moved or off. Live: his tip >= 4M or his size >= 2 SOL; his buys on the signer in the last 24 h >= 2;
# price at most 5% over his fill; sold one slot before his table hold (else his modal first sell - 1, else +9).
import json, sys, collections, statistics as st
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/dec/copy/recent.py").read().split("T = [r for r in rows if r[\"tip\"] >= 4e6]")[0]
exec(src)
def cnt(r, W): return sum(1 for t in r["prev_t"] if t >= his_buy[r["m"]]["bt"] - W * 3600)
KEY = lambda r: r["tip"] >= 4e6 or r["sol"] >= 2.0
HOT = lambda r: cnt(r, 24) >= 2
def H_live(r): return hold(r)
def val(r, prem=0.05, hold_fn=H_live, stake=0.5, D_list=None):
    vs = []
    for D in (D_list or dec):
        en = entry_at(r, D)
        if not en: continue
        if (en[0] / en[1]) / r["hisp"] - 1 > prem: vs.append(-0.0001); continue
        ex = exit_at(r["seq"], r["cs"], max(hold_fn(r), 1), r["s"])
        v = pnl_s(en, ex, stake) if ex else None
        if v is not None: vs.append(v)
    return sum(vs) / len(vs) if vs else None
def show(lab, sel, **kw):
    byd = collections.defaultdict(float); n = 0
    for r in sel:
        v = val(r, **kw)
        if v is None: continue
        byd[r["day"]] += v; n += 1
    tot = sum(byd.values()); pos = sum(1 for d in DAYS if byd[d] > 0)
    print(f"   {lab:50} n {n:4d} total {tot:+6.2f}  /copy {tot / max(n, 1):+.4f}  days+ {pos}/5  " + " ".join(f"{byd[d]:+5.2f}" for d in DAYS))
    return tot
live = [r for r in rows if KEY(r) and HOT(r)]
print("by day: " + " ".join(DAYS))
print("== 0. the live copy")
show("live (all rules)", live)
print("== 2. the tip / size key (operator rule kept)")
for t in (0, 1e6, 2e6, 3e6, 4e6, 6e6, 8e6):
    show(f"tip >= {t/1e6:.0f}M or size >= 2", [r for r in rows if HOT(r) and (r["tip"] >= t or r["sol"] >= 2.0)])
for s in (1.0, 1.5, 2.0, 3.0, 99):
    show(f"tip >= 4M or size >= {s}", [r for r in rows if HOT(r) and (r["tip"] >= 4e6 or r["sol"] >= s)])
print("== 3. the operator rule: his buys on the signer before this one (key kept)")
for K, W in ((0, 24), (1, 24), (2, 6), (2, 12), (2, 24), (2, 48), (3, 24), (4, 24)):
    show(f">= {K} in {W} h", [r for r in rows if KEY(r) and cnt(r, W) >= K])
print("== 4. the table (key and operator rule kept)")
show("listed in the backtest's 24 h tables only", [r for r in live if r["listed"]])
show("not listed there (hold: his modal first sell - 1)", [r for r in live if not r["listed"]])
show("all, hold +9 flat for the unlisted", live, hold_fn=lambda r: r["H"] - 1 if r["listed"] else 9)
print("== 5. the price limit")
for p in (0.02, 0.03, 0.05, 0.075, 0.10, 0.15, 9.0):
    show(f"price <= his + {p:.1%}" if p < 9 else "no limit", live, prem=p)
print("== 6. the exit")
for k in (0, 1, 2, 3):
    show(f"H - {k} (listed), modal - {k} (unlisted)", live, hold_fn=lambda r, k=k: (r["H"] - k) if r["listed"] else (hold(r) + 1 - k))
for h in (5, 7, 9, 10, 12):
    show(f"flat create + {h}", live, hold_fn=lambda r, h=h: h)
print("== 8. the stake")
for s in (0.25, 0.5, 0.75, 1.0):
    show(f"stake {s} SOL", live, stake=s)
# 1. launch buys only: his first buys after create + 1
late = sum(1 for m, b in his_buy.items() if m in coins and b["slot"] > coins[m]["cs"] + 1)
early = sum(1 for m, b in his_buy.items() if m in coins and b["slot"] <= coins[m]["cs"] + 1)
print(f"== 1. his first buy on a coin: at create + 0/1 {early}, later {late}")
json.dump([dict(m=r["m"], day=r["day"], bt=his_buy[r["m"]]["bt"], cs=r["cs"], hold=max(hold(r), 1), v=val(r)) for r in live], open(SP + "/dec/copy/live_copies.json", "w"))
