# The full screen of the candidate copy targets (wallets/cand1.json, plus every other wallet with >= 30 launch buys on the
# complete 13-slot tapes of wide/coin): Oct 23 00:00 - Oct 25 18:00, every in-class launch they bought at create + 0/1 outside the dev's bundle run,
# Per wallet:
#   own   their realized net inside the tape (sells - buys, fees in; what they still hold marked at create + 12; no tips)
#   copy  our 0.5 SOL buy D positions after theirs (our deciles, capped at the end of their slot), bound +5% over their
#         post-trade price, 3M / 0.5M tips, 1.25% fees; sold at (m) their modal first-sell offset on earlier coins - 1
#         (else +9; learned live, ex ante), (5) / (9) flat, or (o) one slot before their own first sell (a ceiling).
#         A hold that reaches their sell sells right after it.
import json, glob, collections, statistics as st, datetime, bisect, os, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
K = 30_000_000_000 * 1_073_000_000_000_000
FEE = 0.0125; STAKE = 0.5; TIP = 0.003; SELL_TIP = 0.0005; BASE = 0.00001; PREM = 0.05
DEC = [32, 45, 58, 71, 84, 101, 125, 158, 200, 322]
T_LO, T_HI = 1792713600, 1792713600 + 66 * 3600
def pnl(entry, ex):
    v0, t0 = entry; k = v0 * t0; s_in = STAKE * 1e9 / (1 + FEE); q = t0 - k / (v0 + s_in)
    vr, tr = ex
    if tr <= q: return None
    return vr * q / (tr - q) * (1 - FEE) / 1e9 - STAKE - TIP - SELL_TIP - 2 * BASE
PAIRS = sorted({(r["slot"], r["bt"]) for w in ("AHwJSn", "4g49xG") for r in json.load(open(f"{SP}/dec/five/walk_{w}.json"))["rows"] if r.get("bt")})
PS = [p[0] for p in PAIRS]
def bt_of(slot):
    i = min(max(bisect.bisect_left(PS, slot), 1), len(PS) - 1)
    (s0, t0), (s1, t1) = PAIRS[i - 1], PAIRS[i]
    return int(t0 + (slot - s0) * (t1 - t0) / max(1, s1 - s0))
def day(t): return datetime.datetime.fromtimestamp(t, datetime.timezone.utc).strftime("%m-%d")
UNIV = set()
for l in open(SP + "/ext/replay/events.jsonl"):
    if l.startswith('{"t": "create"'): UNIV.add(json.loads(l)["mint"])
his = set()
for r in json.load(open(SP + "/dec/five/walk_AHwJSn.json"))["rows"]:
    if r["kind"] == "buy" and r["ok"]: his.add(r["m"])
paths = {}
for d_ in (SP + "/wide/coin",):   # complete tapes only (dec/five samples one state per slot, up to his last sell)
    for p in glob.glob(d_ + "/*.json"): paths[os.path.basename(p)[:-5]] = p
COINS = {}; trades = collections.defaultdict(list)
for m, p in paths.items():
    if m not in UNIV: continue
    d = json.load(open(p))
    if not d.get("create") or not d.get("rows"): continue
    c = d["create"]; cs = d["cs"]; ci = c["idx"]; bt = bt_of(cs)
    if not (T_LO <= bt < T_HI): continue
    seq = []
    for r in sorted(d["rows"], key=lambda r: (r[0], r[1])):
        vs, vt = r[6], r[7]
        if vt <= 0: continue
        if vs <= 0: vs = K // vt
        seq.append((r[0], r[1], vs, vt, r[2], r[3], r[4], r[5], r[8]))
    if not seq: continue
    bund = {c["signer"], c["creator"]}; prev = ci
    for x in seq:
        if x[0] != cs or x[1] < ci: continue
        if x[1] == ci or (x[1] - prev <= 2 and x[1] - ci <= 5): bund.add(x[4]); prev = x[1]
        elif x[1] - prev > 2: break
    COINS[m] = dict(seq=seq, cs=cs, bt=bt, signer=c["signer"])
    fb = {}; fs = {}; mine = collections.defaultdict(list)
    for x in seq:
        mine[x[4]].append(x)
        if x[5] == 1 and x[4] not in fb: fb[x[4]] = x
        if x[5] == 0 and x[4] not in fs: fs[x[4]] = x
    for w, x in fb.items():
        if w in bund or w == US or x[0] > cs + 1: continue
        # their own net inside the tape
        sol = 0; tok = 0
        for y in mine[w]:
            if y[5] == 1: sol -= y[6] + y[8]; tok += y[7]
            else: sol += y[6] - y[8]; tok -= y[7]
        last = seq[-1]
        mark = last[2] * tok / (last[3] + tok) * (1 - FEE) if tok > 0 else 0
        trades[w].append(dict(m=m, x=x, sell=fs.get(w), own=(sol + mark) / 1e9, size=x[6] / 1e9, his=m in his))
print("coins on tapes in the window:", len(COINS), file=sys.stderr)
def state_end(seq, slot):
    v = None
    for x in seq:
        if x[0] > slot: break
        v = x
    return (v[2], v[3]) if v else None
def copy_val(m, x, sell, hold):
    co = COINS[m]; seq = co["seq"]; cs = co["cs"]; wp = x[2] / x[3]; vs = []
    for D in DEC:
        en = None
        for y in seq:
            if (y[0], y[1]) > (x[0], x[1] + D) or y[0] > x[0]: break
            en = (y[2], y[3])
        if (en[0] / en[1]) / wp - 1 > PREM: vs.append(-0.0001); continue
        h = cs + hold
        ex = (sell[2], sell[3]) if (sell is not None and sell[0] <= h) else state_end(seq, min(h, cs + 12))
        v = pnl(en, ex) if ex else None
        if v is not None: vs.append(v)
    return sum(vs) / len(vs) if vs else None
OUT = {}
for w, ts in trades.items():
    if len(ts) < 30: continue
    ts.sort(key=lambda t: COINS[t["m"]]["bt"])
    offs = []; rows = []
    for t in ts:
        m = t["m"]; cs = COINS[m]["cs"]; sell = t["sell"]
        hm = (collections.Counter(offs).most_common(1)[0][0] - 1) if offs else 9
        r = dict(m=m, day=day(COINS[m]["bt"]), bt=COINS[m]["bt"], signer=COINS[m]["signer"], own=t["own"], size=t["size"], his=t["his"],
                 vm=copy_val(m, t["x"], sell, max(hm, 1)), v5=copy_val(m, t["x"], sell, 5), v9=copy_val(m, t["x"], sell, 9),
                 vo=copy_val(m, t["x"], None, max((sell[0] - cs - 1) if sell else 12, 1)), soff=(sell[0] - cs) if sell else None)
        if sell: offs.append(sell[0] - cs)
        rows.append(r)
    OUT[w] = rows
json.dump(OUT, open(SP + "/wallets/screen2.json", "w"))
DAYS = ["10-23", "10-24", "10-25"]
def agg(rows, k):
    byd = collections.defaultdict(float)
    for r in rows:
        if r[k] is not None: byd[r["day"]] += r[k]
    return sum(byd.values()), byd
res = []
for w, rows in OUT.items():
    om, ob = agg(rows, "own"); cm, cb = agg(rows, "vm"); c9, _ = agg(rows, "v9"); c5, _ = agg(rows, "v5"); co, _ = agg(rows, "vo")
    nh = [r for r in rows if not r["his"]]; cmn, _ = agg(nh, "vm")
    so = [r["soff"] for r in rows if r["soff"] is not None]
    res.append(dict(w=w, n=len(rows), size=st.median(r["size"] for r in rows), own=om, ownpos=sum(1 for d in DAYS if ob[d] > 0), copy=cm,
                    copypos=sum(1 for d in DAYS if cb[d] > 0), c5=c5, c9=c9, co=co, nh=len(nh), cmn=cmn, hold=st.median(so) if so else None,
                    sold=len(so) / len(rows), cb={d: cb[d] for d in DAYS}, ob={d: ob[d] for d in DAYS}))
res.sort(key=lambda o: -o["copy"])
print(f"wallets with >= 30 launch buys in the window: {len(res)}")
hdr = f"{'wallet':9} {'buys':>4} {'size':>5} {'hold':>4} {'own':>7} {'d+':>3} {'copy m':>7} {'d+':>3} {'+5':>6} {'+9':>6} {'ceil':>6} {'not his':>7} {'copy':>6}  copy by day / own by day"
print(hdr)
def show(o):
    print(f"{o['w'][:8]:9} {o['n']:4d} {o['size']:5.2f} {str(o['hold']):>4} {o['own']:+7.2f} {o['ownpos']:2d}  {o['copy']:+7.2f} {o['copypos']:2d}  {o['c5']:+6.2f} {o['c9']:+6.2f} {o['co']:+6.2f} {o['nh']:7d} {o['cmn']:+6.2f}  " +
          " ".join(f"{o['cb'][d]:+.2f}" for d in DAYS) + "  / " + " ".join(f"{o['ob'][d]:+.2f}" for d in DAYS))
for o in res[:30]: show(o)
print("... his row and the worst:")
for o in res:
    if o["w"] == A: show(o)
for o in res[-4:]: show(o)
