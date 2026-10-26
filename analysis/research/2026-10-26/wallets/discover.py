# Every wallet that buys pump.fun launches early, across the 26,952 in-class launches of Oct 23 00:00 - Oct 25 18:00
# (create_v2, dev buy >= 0.25 SOL; the exact replay's events: every create-slot trade with its block index, every trade
# of create + 1..3). Launch buy = a buy in the create slot after the dev's bundle run, or in create + 1. Per wallet:
# launch buys per day, size, how often they are his coins, and a short first cut of the copy (our buy D positions after
# theirs, our deciles, bound +5%, sold at the end of create + 3) and of their own position marked at create + 3.
import json, collections, datetime, bisect, statistics as st
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
K = 30_000_000_000 * 1_073_000_000_000_000
FEE = 0.0125; STAKE = 0.5; TIP = 0.003; SELL_TIP = 0.0005; BASE = 0.00001
DEC = [32, 45, 58, 71, 84, 101, 125, 158, 200, 322]
def pnl(entry, ex, stake=STAKE, tips=True):
    v0, t0 = entry; k = v0 * t0; s_in = stake * 1e9 / (1 + FEE); q = t0 - k / (v0 + s_in)
    vr, tr = ex
    if tr <= q: return None
    return vr * q / (tr - q) * (1 - FEE) / 1e9 - stake - ((TIP + SELL_TIP + 2 * BASE) if tips else 0)
C = {}; T = collections.defaultdict(list)
for l in open(SP + "/ext/replay/events.jsonl"):
    if '"t": "create"' in l[:20]:
        x = json.loads(l); C[x["mint"]] = (x["slot"], x["index"])
    elif l.startswith('{"t": "trade"') or l.startswith('{"t": "ltrade"'):
        x = json.loads(l)
        T[x["mint"]].append((x["slot"], x["index"], x["user"], x["buy"], x["sol"], x["vs"], x["vt"], x.get("run_end", False), x.get("is_create", False)))
PAIRS = sorted({(r["slot"], r["bt"]) for w in ("AHwJSn", "4g49xG") for r in json.load(open(f"{SP}/dec/five/walk_{w}.json"))["rows"] if r.get("bt")})
PS = [p[0] for p in PAIRS]
def bt_of(slot):
    i = min(max(bisect.bisect_left(PS, slot), 1), len(PS) - 1)
    (s0, t0), (s1, t1) = PAIRS[i - 1], PAIRS[i]
    return int(t0 + (slot - s0) * (t1 - t0) / max(1, s1 - s0))
his = set()
for r in json.load(open(SP + "/dec/five/walk_AHwJSn.json"))["rows"]:
    if r["kind"] == "buy": his.add(r["m"])
W = collections.defaultdict(lambda: dict(n=0, sol=[], days=collections.Counter(), his=0, c3=collections.defaultdict(float), o3=collections.defaultdict(float), cnt=collections.defaultdict(int)))
for m, rows in T.items():
    if m not in C: continue
    cs, ci = C[m]
    rows.sort(key=lambda r: (r[0], r[1]))
    # the bundle run: up to the trade flagged run_end in the create slot (else the create tx alone)
    ends = [r[1] for r in rows if r[0] == cs and r[7]]
    run_end = max(ends) if ends else ci
    seen = set(r[2] for r in rows if r[0] == cs and r[1] <= run_end)
    end3 = None
    for r in rows:
        if r[0] <= cs + 3: end3 = (r[5], r[6])
    dd = datetime.datetime.fromtimestamp(bt_of(cs), datetime.timezone.utc).strftime("%m-%d")
    for i, r in enumerate(rows):
        if not r[3] or r[2] in seen or r[0] > cs + 1: continue
        if r[0] == cs and r[1] <= run_end: continue
        seen.add(r[2]); w = W[r[2]]
        w["n"] += 1; w["sol"].append(r[4] / 1e9); w["days"][dd] += 1; w["his"] += m in his
        wp = r[5] / r[6]; vs = []
        for D in DEC:
            en = (r[5], r[6])
            for y in rows[i:]:
                if (y[0], y[1]) > (r[0], r[1] + D) or y[0] > r[0]: break
                en = (y[5], y[6])
            if (en[0] / en[1]) / wp - 1 > 0.05: vs.append(-0.0001); continue
            v = pnl(en, end3); vs.append(v if v is not None else -STAKE)
        w["c3"][dd] += sum(vs) / len(vs); w["cnt"][dd] += 1
        # their own position at their size, entered at the state before their trade, marked at create + 3 (no tips)
        prev = (rows[i - 1][5], rows[i - 1][6]) if i > 0 else (r[5], r[6])
        o = pnl(prev, end3, stake=max(r[4] / 1e9, 0.01), tips=False)
        w["o3"][dd] += o if o is not None else -r[4] / 1e9
out = []
for k, w in W.items():
    if w["n"] < 30 or k == US: continue
    days = sorted(w["days"])
    out.append(dict(w=k, n=w["n"], nd=len(days), per_day=round(w["n"] / max(len(days), 1)), sol=round(st.median(w["sol"]), 3), his=w["his"],
                    c3=round(sum(w["c3"].values()), 3), c3pos=sum(1 for d in days if w["c3"][d] > 0), o3=round(sum(w["o3"].values()), 3),
                    o3pos=sum(1 for d in days if w["o3"][d] > 0), byd={d: round(w["c3"][d], 2) for d in days}, obyd={d: round(w["o3"][d], 2) for d in days}))
json.dump(out, open(SP + "/wallets/discover.json", "w"))
print(f"coins {len(T)}; wallets with >= 30 launch buys: {len(out)}")
out.sort(key=lambda o: -o["c3"])
print(f"{'wallet':9} {'buys':>5} {'/day':>5} {'size':>6} {'his':>5} {'copy@+3':>8} {'days+':>5} {'own@+3':>8} {'days+':>5}  copy by day")
for o in out[:35]:
    print(f"{o['w'][:8]:9} {o['n']:5d} {o['per_day']:5d} {o['sol']:6.2f} {o['his']:5d} {o['c3']:+8.2f} {o['c3pos']:3d}/{o['nd']} {o['o3']:+8.2f} {o['o3pos']:3d}/{o['nd']}  " + " ".join(f"{k[3:]}:{v:+.2f}" for k, v in o["byd"].items()))
