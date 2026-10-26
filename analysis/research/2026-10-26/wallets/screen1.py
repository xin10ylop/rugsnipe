# Who else is worth copying? Every wallet's launch buys (create + 0/1, not part of the dev's bundle run, not the dev) on
# the 9,302 coin tapes of Oct 21-25 (the first 13 slots of every coin he or we touched), copied the way we copy him:
# our buy lands D positions after theirs (our measured deciles, capped at the end of their slot), bound at their price +
# 5%, 0.5 SOL, 3M / 0.5M tips, 1.25% fees. Exits: (9) create + 9 flat; (m) their modal first-sell offset on earlier coins
# - 1, else + 9 (learned live, ex ante); (o) one slot before their own first sell (a ceiling no bot can know). A hold that
# reaches their sell sells right after it. Per wallet: coins, days, copy by day, their overlap with him.
import json, glob, collections, statistics as st, datetime, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
K = 30_000_000_000 * 1_073_000_000_000_000
FEE = 0.0125; STAKE = 0.5; TIP = 0.003; SELL_TIP = 0.0005; BASE = 0.00001; PREM = 0.05
DEC = [32, 45, 58, 71, 84, 101, 125, 158, 200, 322]
def pnl(entry, ex):
    v0, t0 = entry; k = v0 * t0; s_in = STAKE * 1e9 / (1 + FEE); q = t0 - k / (v0 + s_in)
    vr, tr = ex
    if tr <= q: return None
    return vr * q / (tr - q) * (1 - FEE) / 1e9 - STAKE - TIP - SELL_TIP - 2 * BASE
def day(bt): return datetime.datetime.fromtimestamp(bt, datetime.timezone.utc).strftime("%m-%d")
his = set()
for r in json.load(open(SP + "/dec/five/walk_AHwJSn.json"))["rows"]:
    if r["kind"] == "buy" and r["ok"]: his.add(r["m"])
import bisect
PAIRS = sorted({(r["slot"], r["bt"]) for w in ("AHwJSn", "4g49xG") for r in json.load(open(f"{SP}/dec/five/walk_{w}.json"))["rows"] if r.get("bt")})
PS = [p[0] for p in PAIRS]
def bt_of(slot):
    i = min(max(bisect.bisect_left(PS, slot), 1), len(PS) - 1)
    (s0, t0), (s1, t1) = PAIRS[i - 1], PAIRS[i]
    return int(t0 + (slot - s0) * (t1 - t0) / max(1, s1 - s0))
trades = collections.defaultdict(list)    # wallet -> [(bt, coin record)]
COINS = {}
for p in glob.glob(SP + "/dec/five/coin/*.json"):
    d = json.load(open(p))
    if not d.get("create") or not d.get("rows"): continue
    c = d["create"]; cs = d["cs"]; ci = c["idx"]
    rows = sorted(d["rows"], key=lambda r: (r[0], r[1]))
    seq = []
    for r in rows:
        vs, vt = r[6], r[7]
        if vt <= 0: continue
        if vs <= 0: vs = K // vt
        seq.append((r[0], r[1], vs, vt, r[9], r[3]))
    if not seq: continue
    bt = bt_of(cs)
    # the dev's bundle run: the create tx and the buys chained right behind it
    bund = {c["signer"], c["creator"]}; prev = ci
    for x in seq:
        if x[0] != cs or x[1] < ci: continue
        if x[1] == ci or (x[1] - prev <= 2 and x[1] - ci <= 5): bund.add(x[4]); prev = x[1]
        elif x[1] - prev > 2: break
    first_buy = {}; first_sell = {}
    for x in seq:
        if x[5] == 1 and x[4] not in first_buy: first_buy[x[4]] = x
        if x[5] == 0 and x[4] not in first_sell: first_sell[x[4]] = x
    COINS[d["mint"]] = dict(seq=seq, cs=cs, bt=bt)
    for w, x in first_buy.items():
        if w in bund or w in (A, US) or x[0] > cs + 1: continue
        trades[w].append((x[0], d["mint"], x, first_sell.get(w)))
def state_end(seq, slot):
    v = None
    for x in seq:
        if x[0] > slot: break
        v = x
    return (v[2], v[3]) if v else None
def after_row(x): return (x[2], x[3])
def copy_val(m, x, sell, hold):
    co = COINS[m]; seq = co["seq"]; cs = co["cs"]
    wp = x[2] / x[3]; vs = []
    for D in DEC:
        en = None
        for y in seq:
            if (y[0], y[1]) > (x[0], x[1] + D) or y[0] > x[0]: break
            en = (y[2], y[3])
        if (en[0] / en[1]) / wp - 1 > PREM: vs.append(-0.0001); continue
        h = cs + hold
        if sell is not None and sell[0] <= h: ex = after_row(sell)
        else: ex = state_end(seq, min(h, cs + 12))
        v = pnl(en, ex) if ex else None
        if v is not None: vs.append(v)
    return sum(vs) / len(vs) if vs else None
OUT = []
for w, ts in trades.items():
    if len(ts) < 20: continue
    ts.sort(key=lambda t: COINS[t[1]]["bt"])
    offs = []; byd = {k: collections.defaultdict(float) for k in ("9", "m", "o")}; n = 0; ov = 0; ov_v = 0.0
    for slot, m, x, sell in ts:
        cs = COINS[m]["cs"]; dd = day(COINS[m]["bt"])
        hm = (collections.Counter(offs).most_common(1)[0][0] - 1) if offs else 9
        v9 = copy_val(m, x, sell, 9); vm = copy_val(m, x, sell, max(hm, 1))
        vo = copy_val(m, x, None, max((sell[0] - cs - 1) if sell else 12, 1))
        if sell: offs.append(sell[0] - cs)
        if v9 is None: continue
        n += 1; byd["9"][dd] += v9; byd["m"][dd] += vm or 0; byd["o"][dd] += vo or 0
        if m in his: ov += 1; ov_v += vm or 0
    days = sorted(byd["9"])
    OUT.append(dict(w=w, n=n, days=len(days), t9=sum(byd["9"].values()), tm=sum(byd["m"].values()), to=sum(byd["o"].values()),
                    pos_m=sum(1 for d in days if byd["m"][d] > 0), byd={d: round(byd["m"][d], 3) for d in days}, ov=ov, ov_v=ov_v,
                    sells=len(offs), mode=collections.Counter(offs).most_common(1)[0][0] if offs else None))
json.dump(OUT, open(SP + "/wallets/screen1.json", "w"))
OUT.sort(key=lambda o: -o["tm"])
print(f"wallets with >= 20 launch buys on the tapes: {len(OUT)}")
print(f"{'wallet':10} {'buys':>5} {'days':>4} {'copy+9':>7} {'copy modal':>10} {'ceiling':>8} {'days+':>5} {'w/ him':>6} {'copy w/o him':>12} {'mode':>4}  by day (modal exit)")
for o in OUT[:30]:
    print(f"{o['w'][:8]:10} {o['n']:5d} {o['days']:4d} {o['t9']:+7.2f} {o['tm']:+10.2f} {o['to']:+8.2f} {o['pos_m']:3d}/{o['days']} {o['ov']:6d} {o['tm'] - o['ov_v']:+12.2f} {str(o['mode']):>4}  " + " ".join(f"{k[3:]}:{v:+.2f}" for k, v in o["byd"].items()))
print("... worst:"); 
for o in OUT[-3:]: print(f"{o['w'][:8]:10} {o['n']:5d} {o['tm']:+.2f}")
