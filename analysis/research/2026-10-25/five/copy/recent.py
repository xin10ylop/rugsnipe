# The copy's "his operator" test made explicit and independent of the table builder: his landed buys on the same
# create signer within the previous W hours (counted before this buy, as the bot can count them live) >= K. Exit: the
# replay table's hold H - 1 when listed there, else his modal first-sell offset on the signer from earlier coins - 1,
# else +9. Tip >= 4M, bound 5%, 0.5 SOL, our landing distances. All 5 days, by day.
import json, sys, collections, bisect
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/dec/copy/weakdays.py").read().split("L = [r for r in rows if r[\"listed\"]]")[0]
exec(src)
PREM = 0.05
DAYS = ["10-21", "10-22", "10-23", "10-24", "10-25"]
order = sorted(rows, key=lambda r: his_buy[r["m"]]["bt"])
times = collections.defaultdict(list); offs = collections.defaultdict(list)
for r in order:
    bt = his_buy[r["m"]]["bt"]
    r["prev_t"] = list(times[r["signer"]]); r["prev_off"] = list(offs[r["signer"]])
    times[r["signer"]].append(bt)
    if r["soff"] is not None: offs[r["signer"]].append(r["soff"])
def hold(r):
    if r["listed"]: return r["H"] - 1
    po = [o for o in r["prev_off"] if o]
    return (collections.Counter(po).most_common(1)[0][0] - 1) if po else 9
def v_of(r):
    vs = []
    for D in dec:
        en = entry_at(r, D)
        if not en: continue
        if (en[0] / en[1]) / r["hisp"] - 1 > PREM: vs.append(-0.0001); continue
        ex = exit_at(r["seq"], r["cs"], max(hold(r), 1), r["s"])
        v = pnl_s(en, ex, 0.5) if ex else None
        if v is not None: vs.append(v)
    return sum(vs) / len(vs) if vs else None
T = [r for r in rows if r["tip"] >= 4e6]
for r in T: r["v"] = v_of(r)
def show(lab, f):
    sel = [r for r in T if r["v"] is not None and f(r)]
    byd = collections.defaultdict(float)
    for r in sel: byd[r["day"]] += r["v"]
    pos = sum(1 for d in DAYS if byd[d] > 0)
    print(f"   {lab:56} n {len(sel):4} total {sum(byd.values()):+6.2f}  days+ {pos}/5  " + " ".join(f"{byd[d]:+.2f}" for d in DAYS))
print("his tip >= 4M, bound 5%; by day " + " ".join(DAYS))
show("listed in the backtest's tables (the measured rule)", lambda r: r["listed"])
show("every signer", lambda r: True)
for W in (2, 4, 8, 24):
    for K in (1, 2, 3, 5):
        show(f"his buys on the signer in the last {W}h >= {K}",
             lambda r, W=W, K=K: sum(1 for t in r["prev_t"] if t >= his_buy[r["m"]]["bt"] - W * 3600) >= K)
