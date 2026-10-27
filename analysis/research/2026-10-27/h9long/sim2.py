# The copy on the entry tapes (entry_tapes.py): we land at the end of its slot ("same") or of the next slot ("next"),
# with every trade between its buy and our landing ahead of us; out right before its sell (we run its own exit rule a
# step ahead: TP at 2.5x its post-buy price, SL at 0.7x, the 30-minute stop) or right after it. Constant product in vt.
# Costs: its own median round-trip cost on the curve and routes (2.9% SOL coins, 5.1% other quotes), our transaction fees
# (low 0.0045 / high 0.0095 SOL a transaction) and the model bias 0.0155 SOL a fill. Writes one row per trip and setting.
#   python3 -I sim2.py WALLET        ->  <w8>_sim2.json
import sys, json, datetime
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = sys.argv[1]
D = json.load(open(f"{SP}/h9long/{W[:8]}_etapes.json"))
Q = json.load(open(f"{SP}/h9long/{W[:8]}_quote.json"))
COST = {"pump, other quote": 0.9487, "pump, SOL quote": 0.9713}
BIAS = 0.0155
def land(v, where):
    b = v["b"]; tr = v["near"]; oi = b["i"] or 0
    last = b["s"] if where == "same" else b["s"] + 1
    ahead = [r for r in tr if (r[0], r[1]) > (b["s"], oi) and r[0] <= last]
    return (ahead[-1][5] if ahead else b["vt"]), ahead
def copy(v, stake, where, exit_):
    b, s = v["b"], v["s"]
    its = b["v"] or 0
    if its <= 0.05 or not b["vt"] or not s["vt"]: return None
    vbb, vab = b["vt"] + b["tok"], b["vt"]
    Qits = 1 / vab - 1 / vbb                    # its quote in (units of k)
    va, ahead = land(v, where)
    q_in = Qits * stake / its
    vt2 = 1 / (1 / va + q_in); tok = va - vt2
    if exit_ == "before": vx = s["vt"] - s["tok"]
    elif exit_ == "after": vx = s["vt"]
    else:                                      # scalp: sell at the end of slot + 2 after its buy
        tr = [r for r in v["near"] if (r[0], r[1]) > (b["s"], b["i"] or 0) and r[0] <= b["s"] + 2]
        vx = tr[-1][5] if tr else va
    # the curve state we sell into carries our tokens too; trades after ours moved it as they did without us
    vx_us = vx - tok
    if vx_us <= 0: return None
    gross = (1 / vx_us - 1 / vx) / q_in
    prem = (vab / va) ** 2 - 1
    return dict(gross=gross, prem=prem, n_ahead=len(ahead), net_curve=stake * (gross * COST[v["kind"]] - 1))
out = []
for m, v in D.items():
    if not v: continue
    b = v["b"]
    base = dict(m=m, kind=v["kind"], bt=b["bt"], its_sol=b["v"], tip=b["tip"], ix=b["ix"], quote=Q.get(m, "via SOL"),
                sold_before=(1_073_000_191e6 - (b["vt"] + b["tok"])) / 1e12, n_between=v["n_between"], hold=v["s"]["bt"] - b["bt"],
                its_mult=((b["vt"] / (v["s"]["vt"] - v["s"]["tok"])) ** 2) if v["s"]["vt"] - v["s"]["tok"] > 0 else None)
    for stake in (0.5, 1.0, "its"):
        st_ = (b["v"] or 0) if stake == "its" else stake
        for where in ("same", "next"):
            for ex in ("before", "after", "scalp"):
                c = copy(v, st_, where, ex)
                if c: out.append(dict(base, stake=stake, st=st_, where=where, exit=ex, **c))
json.dump(out, open(f"{SP}/h9long/{W[:8]}_sim2.json", "w"))
print(len(D), "trips,", len(out), "rows")
