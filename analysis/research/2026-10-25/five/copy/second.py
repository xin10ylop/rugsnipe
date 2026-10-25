# Our buys that landed but failed on our own slippage bound (error 6042, max_premium = 0): what they would have made
# had the bound let them fill at the curve state they met, at their real block positions; and a pure copy of his
# landed buys (right behind him in the same block, or the next slot), sold at create + 9 / + 10 / one slot before his sell.
import json, glob, collections, statistics as st, datetime, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"; F = SP + "/dec/five"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
K = 30_000_000_000 * 1_073_000_000_000_000
FEE = 0.0125; STAKE = 0.5; TIP = 0.003; SELL_TIP = 0.0005; BASE = 0.00001
W = {w: json.load(open(f"{F}/walk_{w[:6]}.json"))["rows"] for w in (A, US)}
coins = {}
for p in glob.glob(F + "/coin/*.json"):
    d = json.load(open(p))
    if d.get("create") and d.get("rows"): coins[d["mint"]] = d
def seq_of(d):
    out = []
    for r in d["rows"]:
        vs, vt = r[6], r[7]
        if vt <= 0: continue
        if vs <= 0: vs = K // vt
        out.append((r[0], r[1], vs, vt, r[2], r[3], r[4]))
    out.sort(key=lambda x: (x[0], x[1]))
    return out
def state_before(seq, slot, idx):
    xs = [x for x in seq if (x[0], x[1]) < (slot, idx)]
    return (xs[-1][2], xs[-1][3]) if xs else None
def state_end(seq, slot):
    xs = [x for x in seq if x[0] <= slot]
    return (xs[-1][2], xs[-1][3]) if xs else None
def pnl(entry, exit_):
    v0, t0 = entry; k = v0 * t0
    s_in = STAKE * 1e9 / (1 + FEE)
    q = t0 - k / (v0 + s_in)
    vr, tr = exit_
    if tr <= q: return None
    out = vr * q / (tr - q) * (1 - FEE)
    return out / 1e9 - STAKE - TIP - SELL_TIP - 2 * BASE
def post_bundle(d, seq):
    c = d["create"]; cs = d["cs"]; ci = c["idx"]
    cset = {c["signer"], c["creator"]} | {x[4] for x in seq if x[0] == cs and x[1] == ci}
    prev = ci; last = None
    for x in seq:
        if x[0] != cs or x[1] < ci: continue
        if x[1] == ci: last = x; continue
        if x[4] in cset and x[1] - prev <= 2: prev = x[1]; last = x; continue
        if x[5] == 1 and x[1] - prev <= 2 and x[1] - ci <= 5 and x[4] not in (A, US): prev = x[1]; last = x
        elif x[1] - prev > 2 or x[1] - ci > 5: break
    return (last[2], last[3]) if last else None
his_buy = {}; his_sell = {}; his_net = collections.defaultdict(int)
for r in W[A]:
    his_net[r["m"]] += r["delta"]
    if r["kind"] == "buy" and r["ok"] and r["fill"]:
        if r["m"] not in his_buy or (r["slot"], r["idx"]) < (his_buy[r["m"]]["slot"], his_buy[r["m"]]["idx"]): his_buy[r["m"]] = r
    elif r["kind"] == "sell" and r["ok"]:
        if r["m"] not in his_sell or r["slot"] < his_sell[r["m"]]["slot"]: his_sell[r["m"]] = r
def day(bt): return datetime.datetime.fromtimestamp(bt, datetime.timezone.utc).strftime("%m-%d")
def summ(lab, xs):
    if not xs: print(f"   {lab:44} n 0"); return
    print(f"   {lab:44} n {len(xs):4d}  total {sum(xs):+8.3f} SOL  mean {sum(xs)/len(xs):+.4f} ({100*sum(xs)/len(xs)/STAKE:+5.1f}%)  median {100*st.median(xs)/STAKE:+5.1f}%  wins {sum(x > 0 for x in xs)/len(xs):4.0%}")
# ---- A. our 6042 failures, filled at their real position
print("== A. our buys that landed and failed on our own min_tokens bound (6042), had they filled where they landed")
rows = collections.defaultdict(list); prem = []
for r in W[US]:
    if r["kind"] != "buy" or r["ok"] or r["code"] != 6042: continue
    d = coins.get(r["m"])
    if not d: continue
    seq = seq_of(d); cs = d["cs"]
    if r["slot"] > cs + 1: continue
    e = state_before(seq, r["slot"], r["idx"]); pb = post_bundle(d, seq)
    if not e or not pb: continue
    ahead = "him" if r["m"] in his_buy and (his_buy[r["m"]]["slot"], his_buy[r["m"]]["idx"]) < (r["slot"], r["idx"]) else "a rival"
    prem.append(((e[0] / e[1]) / (pb[0] / pb[1]) - 1, ahead))
    for lab, ex in (("+9", state_end(seq, cs + 9)), ("+10", state_end(seq, cs + 10))):
        if ex: rows[(ahead, lab)].append(pnl(e, ex))
for k in sorted(rows): summ(f"ahead of us: {k[0]:8} sell at create {k[1]}", [x for x in rows[k] if x is not None])
for who in ("him", "a rival"):
    ps = [p for p, a in prem if a == who]
    if ps: print(f"   premium we would have paid over the post-bundle price (ahead: {who}): median {100*st.median(ps):.1f}%  p75 {100*sorted(ps)[3*len(ps)//4]:.1f}%  p90 {100*sorted(ps)[9*len(ps)//10]:.1f}%")
# ---- B. copy every buy he landed: right behind him (same block), or the next slot
print("\n== B. copy every buy he landed (5-day coin set), 0.5 SOL, tip 3M, sell tip 0.5M, fees 1.25% each way")
res = collections.defaultdict(list); byday = collections.defaultdict(lambda: collections.defaultdict(float)); hisn = collections.defaultdict(float)
for m, b in his_buy.items():
    d = coins.get(m)
    if not d: continue
    seq = seq_of(d); cs = d["cs"]
    if b["slot"] > cs + 1: continue
    f = b["fill"]; e0 = (f[2] if f[2] > 0 else K // f[3], f[3])         # right behind him: the state his buy left
    e1 = state_end(seq, b["slot"])                                       # the next slot: his slot's last state
    s = his_sell.get(m)
    exits = {"+9": state_end(seq, cs + 9), "+10": state_end(seq, cs + 10)}
    if s and s["slot"] - 1 >= b["slot"] + 1: exits["his sell - 1"] = state_end(seq, s["slot"] - 1)
    hisn["all"] += his_net[m] / 1e9
    for en_lab, en in (("same block, right behind him", e0), ("next slot", e1)):
        if not en: continue
        for ex_lab, ex in exits.items():
            if not ex: continue
            v = pnl(en, ex)
            if v is None: continue
            res[(en_lab, ex_lab)].append(v)
            if ex_lab == "+9": byday[en_lab][day(b["bt"])] += v
print(f"   his own net on these coins: {hisn['all']:+.2f} SOL ({len(his_buy)} landed buys, {sum(1 for m in his_buy if m in coins)} with a tape)")
for k in sorted(res): summ(f"{k[0]:28} sell {k[1]}", res[k])
for en_lab, dd in byday.items():
    print(f"   by day, {en_lab}, sell +9: " + "  ".join(f"{k} {v:+.2f}" for k, v in sorted(dd.items())))
