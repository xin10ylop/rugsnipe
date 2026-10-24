# A wallet's month: daily attempts / trades / P&L, stake, hold, entry curve level, creators, SOL transfers.
#   python3 -I month/profile.py WALLET
import sys, json, collections, statistics as St, time, math
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = sys.argv[1]
rows = [json.loads(l) for l in open(f"{SP}/month/{W[:8]}.jsonl")]
rows.sort(key=lambda r: (r["s"], r["i"] or 0))
by = collections.defaultdict(list); other = []
for r in rows:
    mine = [t for t in r["tr"] if t[1] == W]
    kind = None
    if mine: kind = "buy" if mine[0][2] else "sell"
    elif any(p.startswith("buy") for p in r["pix"]): kind = "buy"
    elif any(p.startswith("sell") for p in r["pix"]): kind = "sell"
    r["kind"] = kind; r["mine"] = mine
    m = (mine[0][0] if mine else (r["mints"][0] if r["mints"] else None))
    if kind and m: by[m].append(r)
    else: other.append(r)
trips = []
for m, xs in by.items():
    lb = [x for x in xs if x["kind"] == "buy" and not x["err"] and x["mine"]]
    fails = [x for x in xs if x["kind"] == "buy" and x["err"]]
    sells = [x for x in xs if x["kind"] == "sell" and not x["err"]]
    t = dict(mint=m, bt=min(x["bt"] for x in xs), att=1, landed=bool(lb), fail_cost=-sum(x["d"] for x in fails) / 1e9, n_fail=len(fails))
    if lb:
        b = lb[0]; e = b["mine"][0]
        t.update(stake=-sum(x["d"] for x in lb) / 1e9, sol_in=e[3] / 1e9, creator=e[8], v_after=e[5] / 1e9, v_before=(e[5] - e[3]) / 1e9,
                 buy_slot=b["s"], buy_idx=b["i"], tip=b["tip"], prio=(b["cup"] or 0) * (b["cul"] or 0) / 1e6, closed=bool(sells),
                 pnl=sum(x["d"] for x in lb + sells) / 1e9, hold_s=(sells[-1]["bt"] - b["bt"]) if sells else None,
                 hold_slots=(sells[-1]["s"] - b["s"]) if sells else None, n_sells=len(sells), ix=e[11])
    trips.append(t)
day = lambda bt: time.strftime("%m-%d", time.gmtime(bt))
D = collections.defaultdict(list)
for t in trips: D[day(t["bt"])].append(t)
print(f"== {W[:8]}: {len(rows)} txs, {len(trips)} launches attempted, {sum(1 for t in trips if t['landed'])} landed")
print(f"{'day':5s} {'att':>4s} {'land':>4s} {'land%':>5s} {'win%':>5s} {'stake':>5s} {'pnl':>8s} {'fails':>6s} {'net':>8s} {'pertrade':>8s} {'hold_s':>6s} {'v_in':>5s}")
tot = 0
for dd in sorted(D):
    G = D[dd]; L = [t for t in G if t["landed"] and t.get("closed")]
    pnl = sum(t["pnl"] for t in L); fc = sum(t["fail_cost"] for t in G); tot += pnl - fc
    print(f"{dd:5s} {len(G):4d} {len(L):4d} {100*len(L)/max(len(G),1):5.0f} {100*sum(1 for t in L if t['pnl']>0)/max(len(L),1):5.0f} "
          f"{St.median([t['stake'] for t in L]) if L else 0:5.2f} {pnl:+8.3f} {fc:6.3f} {pnl-fc:+8.3f} "
          f"{100*St.mean([t['pnl']/t['stake'] for t in L]) if L else 0:+7.2f}% {St.median([t['hold_s'] for t in L if t['hold_s'] is not None]) if L else 0:6.1f} "
          f"{St.median([t['v_before'] for t in L]) if L else 0:5.1f}")
print(f"month net {tot:+.3f} SOL")
json.dump(trips, open(f"{SP}/month/{W[:8]}_trips.json", "w"))
# SOL transfers in/out (non-trade)
cp = collections.defaultdict(lambda: [0, 0, 0, 0])   # in_n, in_sol, out_n, out_sol
for r in rows:
    for s, d, l in r["xf"]:
        if r["kind"]: continue
        if d == W: cp[s][0] += 1; cp[s][1] += l / 1e9
        elif s == W: cp[d][2] += 1; cp[d][3] += l / 1e9
print("\nSOL transfers (outside trades), top counterparties:")
for k, v in sorted(cp.items(), key=lambda kv: -(kv[1][1] + kv[1][3]))[:12]:
    print(f"  {k}  in {v[0]:3d} x {v[1]:9.3f} SOL   out {v[2]:3d} x {v[3]:9.3f} SOL")
print("other txs (no pump trade):", len(other), collections.Counter(tuple(sorted({c[0][:8] for c in r["calls"]})) for r in other).most_common(6))
