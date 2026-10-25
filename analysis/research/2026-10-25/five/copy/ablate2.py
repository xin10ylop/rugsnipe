import json, sys, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
exec(open(SP + "/dec/copy/ablate.py").read().split("live = [r for r in rows")[0])
K2 = lambda r: r["tip"] >= 2e6 or r["sol"] >= 2.0
print("by day: " + " ".join(DAYS))
print("== tip >= 2M or size >= 2: the operator rule again")
for K, W in ((1, 24), (2, 24), (2, 48), (3, 24)):
    show(f">= {K} in {W} h", [r for r in rows if K2(r) and cnt(r, W) >= K])
S2 = [r for r in rows if K2(r) and cnt(r, 24) >= 2]
print("== tip >= 2M or size >= 2: the price limit and the exit again")
for p in (0.03, 0.05, 0.075, 0.10):
    show(f"price <= his + {p:.1%}", S2, prem=p)
for k in (0, 1, 2):
    show(f"H - {k}", S2, hold_fn=lambda r, k=k: (r["H"] - k) if r["listed"] else (hold(r) + 1 - k))
show("listed in the 24 h tables only", [r for r in S2 if r["listed"]])
show("not listed there", [r for r in S2 if not r["listed"]])
print("== tip >= 2M or size >= 2, 2M-4M tips alone (what lowering the floor adds)")
show("2M <= tip < 4M, size < 2", [r for r in S2 if 2e6 <= r["tip"] < 4e6 and r["sol"] < 2.0])
show("tip in [1.5M, 2M), size < 2 (just under the floor)", [r for r in rows if cnt(r, 24) >= 2 and 1.5e6 <= r["tip"] < 2e6 and r["sol"] < 2.0])
show("tip < 1.5M, size < 2", [r for r in rows if cnt(r, 24) >= 2 and r["tip"] < 1.5e6 and r["sol"] < 2.0])
# risk limits on the 2M set: copies open at once (buy at his buy, sold at create + hold, 0.4 s a slot) and the
# worst running loss inside a UTC day
ev = []
for r in S2:
    v = val(r)
    if v is None: continue
    t0 = his_buy[r["m"]]["bt"]; t1 = t0 + (r["cs"] + max(hold(r), 1) - his_buy[r["m"]]["slot"]) * 0.4
    ev.append((t0, t1, v, r["day"]))
ev.sort()
mx = 0; blocked2 = 0; blocked1 = 0; open_ = []
for t0, t1, v, d in ev:
    open_ = [x for x in open_ if x > t0]
    if len(open_) >= 2: blocked2 += 1
    if len(open_) >= 1: blocked1 += 1
    open_.append(t1); mx = max(mx, len(open_))
print(f"== caps: most copies open at once {mx}; copies a cap of 2 open would block {blocked2}, a cap of 1 {blocked1}; copies a day {len(ev) / 5:.0f} (cap 300)")
worst = {}
for d in DAYS:
    run = 0; lo = 0
    for t0, t1, v, dd in sorted(ev, key=lambda x: x[1]):
        if dd != d: continue
        run += v; lo = min(lo, run)
    worst[d] = lo
print("   worst running loss inside each day (the 1 SOL stop): " + " ".join(f"{worst[d]:+.2f}" for d in DAYS))
w0 = 1792713600; w1 = 1792713600 + 2 * 86400 + 17 * 3600 + 58 * 60
print(f"== class filter: his launch buys with a tape Oct 23 00:00 - Oct 25 17:58: {sum(1 for r in rows if w0 <= his_buy[r['m']]['bt'] < w1)} (the replay's hot loop saw 871 past the class filter)")
