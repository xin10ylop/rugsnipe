# Holders (hold/log/<w>.jsonl: their landed transactions since Oct 24 00:00, with the wallet's token change per mint).
# Per wallet and coin: buys = transactions where its balance of the mint rises, sells = where it falls; the coin's
# chain-true result = the SOL balance change of those transactions (fees and tips in, any venue); hold = first buy to
# last sell. Only coins first bought in the window and fully sold (>= 95% of the tokens) count; still held = apart.
# The copy follows them: 0.5 SOL bought right after their first buy (the curve price after it, + IN slippage for the
# others ahead of us), and after each of their sells we sell the same share of our tokens, at the curve price after
# their sell when it was on the curve, else (after migration) at their own average sell price - OUT. 1.25% pump fee
# each way on the curve (pool fees taken as the same), 3M + 0.5M tips per round.   python3 -I hold/holders.py [IN OUT]
import json, glob, os, collections, statistics as st, sys, datetime
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
K = 30_000_000_000 * 1_073_000_000_000_000
FEE = 0.0125; STAKE = 0.5; COST = 0.0035 + 0.00002
IN = float(sys.argv[1]) if len(sys.argv) > 1 else 0.02; OUT = float(sys.argv[2]) if len(sys.argv) > 2 else 0.03
WSOL = "So11111111111111111111111111111111111111112"
def day(t): return datetime.datetime.fromtimestamp(t, datetime.timezone.utc).strftime("%m-%d")
def coins_of(w, rows):
    C = collections.defaultdict(list)
    for r in rows:
        tok = {m: v for m, v in (r.get("tok") or {}).items() if m != WSOL}
        if len(tok) != 1: continue          # one coin per transaction, else the SOL change cannot be split
        (m, dt), = tok.items()
        curve = [t for t in r.get("tr") or [] if t[1] == w and t[0] == m]
        st_ = None
        pre = None
        if curve:
            t = curve[-1]
            if t[6] > 0:
                st_ = (t[5] if t[5] > 0 else K // t[6], t[6])
                # the curve just before this trade: a sell took sol out and put tokens in; a buy the reverse
                pre = (st_[0] + t[3], st_[1] - t[4]) if t[2] == 0 else (st_[0] - t[3], st_[1] + t[4])
        C[m].append(dict(bt=r.get("bt") or 0, s=r["s"], dt=dt, d=r["d"], st=st_, pre=pre))
    return C
def copy_coin(ev):
    """the copy's net on one coin, or None"""
    b0 = ev[0]
    if not b0["st"]: return None                   # their first buy must be on the curve (a launch-time buy)
    v, t = b0["st"]; p_in = v / t * (1 + IN)
    v0 = (p_in * K) ** 0.5; t0 = K / v0             # the curve where it costs p_in
    s_in = STAKE * 1e9 / (1 + FEE); q = t0 - K / (v0 + s_in)
    held = sum(e["dt"] for e in ev if e["dt"] > 0)
    out = 0.0; left = 1.0
    for e in ev:
        if e["dt"] >= 0: continue
        frac = min(left, -e["dt"] / held); left -= frac
        if e["st"]:
            vr, tr = e["st"]; qq = q * frac
            out += vr * qq / (tr + qq) * (1 - FEE) / 1e9
        else:
            sol_in = (e["d"] / 1e9) / max(-e["dt"], 1)          # their SOL per raw token on this sell
            out += sol_in * (q * frac) * (1 - OUT)
        if left <= 1e-9: break
    return out - STAKE - COST
def parts(ev):
    """their entry impact, the move between their buy and their first sell, their exit impact (curve trades only)"""
    b0 = ev[0]; s0 = next((e for e in ev if e["dt"] < 0), None)
    if not (b0["st"] and b0["pre"] and s0 and s0["st"] and s0["pre"]): return None
    pb_pre = b0["pre"][0] / b0["pre"][1]; pb = b0["st"][0] / b0["st"][1]
    ps_pre = s0["pre"][0] / s0["pre"][1]; ps = s0["st"][0] / s0["st"][1]
    return (pb / pb_pre - 1, ps_pre / pb - 1, 1 - ps / ps_pre, s0["pre"])
def ceiling(ev):
    """the copy sold just before their first sell (all of it), entered as copy_coin"""
    p = parts(ev)
    if not p: return None
    v, t = ev[0]["st"]; p_in = v / t * (1 + IN)
    v0 = (p_in * K) ** 0.5; t0 = K / v0; s_in = STAKE * 1e9 / (1 + FEE); q = t0 - K / (v0 + s_in)
    vr, tr = p[3]
    return vr * q / (tr + q) * (1 - FEE) / 1e9 - STAKE - COST
res = []
for p in sorted(glob.glob(SP + "/hold/log/*.jsonl")):
    w8 = os.path.basename(p)[:8]
    rows = [json.loads(l) for l in open(p)]
    if not rows: continue
    w = next((t[1] for r in rows for t in r.get("tr") or [] if t[1].startswith(w8)), None) or w8
    C = coins_of(w, rows)
    per = []
    for m, ev in C.items():
        ev.sort(key=lambda e: (e["s"], e["bt"]))
        if ev[0]["dt"] <= 0: continue                      # first bought before the window
        bought = sum(e["dt"] for e in ev if e["dt"] > 0); sold = -sum(e["dt"] for e in ev if e["dt"] < 0)
        if sold < 0.95 * bought:
            per.append(dict(m=m, closed=False, day=day(ev[0]["bt"]))); continue
        sells = [e for e in ev if e["dt"] < 0]
        per.append(dict(m=m, closed=True, day=day(ev[0]["bt"]), own=sum(e["d"] for e in ev) / 1e9,
                        spent=-sum(e["d"] for e in ev if e["dt"] > 0) / 1e9, hold=sells[-1]["bt"] - ev[0]["bt"],
                        first=sells[0]["bt"] - ev[0]["bt"], n_sells=len(sells), off_curve=sum(1 for e in sells if not e["st"]),
                        copy=copy_coin(ev), ceil=ceiling(ev), parts=parts(ev)))
    cl = [x for x in per if x["closed"]]
    if len(cl) < 15: continue
    byd = collections.defaultdict(lambda: [0.0, 0.0, 0])
    for x in cl:
        byd[x["day"]][0] += x["own"]; byd[x["day"]][2] += 1
        if x["copy"] is not None: byd[x["day"]][1] += x["copy"]
    own = sum(x["own"] for x in cl); cp = [x["copy"] for x in cl if x["copy"] is not None]
    srt = sorted((x["own"] for x in cl), reverse=True)
    pp = [x["parts"] for x in cl if x["parts"]]
    med = lambda i: st.median(p_[i] for p_ in pp) if pp else float("nan")
    ce = [x["ceil"] for x in cl if x["ceil"] is not None]
    res.append(dict(w=w, imp_in=med(0), drift=med(1), imp_out=med(2), mdrift=st.mean(p_[1] for p_ in pp) if pp else float("nan"), ceil=sum(ce), nceil=len(ce), n=len(cl), open=len(per) - len(cl), own=own, roi=own / max(sum(x["spent"] for x in cl), 1e-9),
                    hold=st.median(x["hold"] for x in cl), first=st.median(x["first"] for x in cl), sells=st.mean(x["n_sells"] for x in cl),
                    off=sum(x["off_curve"] > 0 for x in cl) / len(cl), copy=sum(cp), ncopy=len(cp), top3=sum(srt[:3]) / own if own > 0 else None,
                    byd={d: (round(v[0], 2), round(v[1], 2), v[2]) for d, v in sorted(byd.items())}))
json.dump(res, open(SP + f"/hold/holders_{IN}_{OUT}.json", "w"))
res.sort(key=lambda o: -o["copy"])
print(f"copy: in +{IN:.0%} after their buy, out -{OUT:.0%} under their price off the curve")
print("their own price impact at entry and at exit, and the market's move between their buy and their first sell (medians;")
print("the move's mean in brackets): a copy keeps only the move, less our lag; following their sell also pays their exit impact")
print(f"{'wallet':9} {'coins':>5} {'open':>4} {'own':>7} {'roi':>5} {'hold':>6} {'1st sell':>8} {'sells':>5} {'off-curve':>9} {'top3':>5} | {'imp in':>6} {'move':>6} {'(mean)':>7} {'imp out':>7} | {'copy':>6} ({'n':>3}) {'ceiling':>7} | by day: own / copy (coins)")
for o in res:
    t3 = "-" if o["top3"] is None else f"{o['top3']:.0%}"
    print(f"{o['w'][:8]:9} {o['n']:5d} {o['open']:4d} {o['own']:+7.2f} {o['roi']:+5.0%} {o['hold']:5.0f}s {o['first']:7.0f}s {o['sells']:5.1f} {o['off']:9.0%} {t3:>5} | {o['imp_in']:+6.1%} {o['drift']:+6.1%} ({o['mdrift']:+6.1%}) {o['imp_out']:+7.1%} | {o['copy']:+6.2f} ({o['ncopy']:3d}) {o['ceil']:+7.2f} | " +
          "  ".join(f"{d[3:]}: {v[0]:+.2f}/{v[1]:+.2f} ({v[2]})" for d, v in o["byd"].items()))
