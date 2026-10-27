# Shared engine: the 5-day copy of him (scratch.py's model and key) on the 60-slot tapes (SP/refused/tape), the live
# landings, the in-between buyers' classes learned only from earlier coins, and the mechanisms' fills.
import os, io, contextlib, json, glob, bisect, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
_cwd = os.getcwd(); os.chdir(SP + "/dec/copy")
src = open("scratch.py").read().split("for sl, sel in")[0].replace('PREM = float(__import__("os").environ.get("PREM", "0.10"))', "PREM = 0.05")
with contextlib.redirect_stdout(io.StringIO()):
    exec(src)
os.chdir(_cwd)
BIAS = 0.0155; REF = -0.0001
TD = SP + "/refused/tape"
def long_seq(m):
    p = f"{TD}/{m}.json"
    if not os.path.exists(p): return None
    t = json.load(open(p))
    out = [(r[0], r[1], r[6] if r[6] > 0 else K // r[7], r[7], r[2], r[3], r[4]) for r in t["rows"] if r[7] > 0]
    out.sort(key=lambda x: (x[0], x[1]))
    return out, t["c0"] + t.get("span", 60)
def st_before(seq, key):
    i = bisect.bisect_left(seq, key, key=lambda x: (x[0], x[1]))
    return (seq[i - 1][2], seq[i - 1][3]) if i else None
def st_end(seq, slot): return st_before(seq, (slot + 1, -1))
# launch-buyer classes from the 5-day launch tapes, by create slot (no look-ahead: only coins created >= 150 slots earlier)
_W = collections.defaultdict(list)          # wallet -> sorted [(cs, on_his_coin, after_him)]
his_coins = set(his_buy)
for p in glob.glob(SP + "/dec/five/coin/*.json"):
    d = json.load(open(p)); cs = d.get("cs")
    if not cs or not d.get("rows"): continue
    rr = sorted(d["rows"], key=lambda x: (x[0], x[1]))
    hb = next(((x[0], x[1]) for x in rr if x[2] == A and x[3] == 1), None)
    seen = set()
    for x in rr:
        u = x[2]
        if x[3] != 1 or x[0] > cs + 1 or u in seen: continue
        seen.add(u); _W[u].append((cs, d["mint"] in his_coins, bool(hb and (x[0], x[1]) > hb)))
for u in _W: _W[u].sort()
_Wcs = {u: [c for c, _, _ in v] for u, v in _W.items()}
N_ALL = sorted(json.load(open(p)).get("cs") or 0 for p in glob.glob(SP + "/dec/five/coin/*.json"))
def cls(u, cs0):
    """'sniper' (buys launches at the base rate of his coins), 'reactor' (>= 2.5x enriched on his coins, after him), or
    'new' (fewer than 5 earlier launch buys)"""
    v = _W.get(u)
    if not v: return "new"
    k = bisect.bisect_left(_Wcs[u], cs0 - 150)
    if k < 5: return "new"
    on = sum(1 for _, h, _ in v[:k] if h)
    n_all = bisect.bisect_left(N_ALL, cs0 - 150); n_his = bisect.bisect_left(HIS_CS, cs0 - 150)
    base = n_his / max(n_all, 1)
    return "reactor" if on / k >= 2.5 * base else "sniper"
HIS_CS = sorted(coins[m]["cs"] for m in his_coins if m in coins)
def between(seq, a, b):
    return [x for x in seq if a < (x[0], x[1]) < b]
def pnl_k(en, ex, stake):
    return pnl_s(en, ex, stake)
def exit_state(r, seq, h=None, kind="anticipate"):
    """our sale: 'anticipate' = end of create + H - 1 or his first sell if earlier (the bot's his sell - 1); 'c<h>' = end
    of create + h - 1 (h-slot hold); 'follow' = right after his first sell"""
    s = r["s"]; cs = r["cs"]
    if kind == "anticipate":
        if s is not None and cs + r["H"] - 1 >= s["slot"]:
            f = s["fill"]; return ((f[2] if f[2] > 0 else K // f[3]), f[3])
        return st_end(seq, cs + r["H"] - 1)
    if kind == "follow":
        if not s: return None
        f = s["fill"]; return ((f[2] if f[2] > 0 else K // f[3]), f[3])
    h = int(kind[1:]); return st_end(seq, cs + h - 1)
