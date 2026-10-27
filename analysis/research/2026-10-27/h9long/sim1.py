# Copy model from its own trades only (no tapes): 0.5 SOL in at the curve state right after its buy (+ an entry
# slippage e), out at the state right before its sell (we exit first: its TP / SL / 30-min stop are known levels) or
# right after it (we mirror its sell). Constant product: price ~ 1/vt^2, our tokens and our proceeds from vt alone.
# Costs: the round-trip cost it pays itself (median: 2.9% SOL coins, 5.1% other quotes), 0.003 SOL a transaction,
# and the model bias 0.0155 SOL a fill.     python3 -I sim1.py WALLET [entry_slippage ...]
import sys, json, collections, datetime
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = sys.argv[1]; SLIPS = [float(x) for x in sys.argv[2:]] or [0.0]
T = [x for x in json.load(open(f"{SP}/h9long/{W[:8]}_trips.json"))]
COST = {"pump, other quote": 0.9487, "pump, SOL quote": 0.9713}
STAKE, TXF, BIAS = 0.5, 0.003, 0.0155
def copy(b, s, e, ahead):
    vbb, vab = b["vt"] + b["tok"], b["vt"]
    its_sol = b["v"] or 0
    if its_sol <= 0.05: return None
    f = STAKE / its_sol
    Q = 1 / vab - 1 / vbb                         # its quote in, in units of k
    # our entry with slippage e in price: the curve is at vab / sqrt(1+e)
    va = vab / (1 + e) ** 0.5
    q_in = f * Q
    vt2 = 1 / (1 / va + q_in); tok = va - vt2
    vx = (s["vt"] - s["tok"]) if ahead else s["vt"]   # the state we sell into (without us)
    vx_us = vx - tok * (vab / va) ** 0 if False else vx - tok
    if vx_us <= 0: return None
    out = 1 / vx_us - 1 / vx
    gross = out / q_in
    return STAKE * (gross * COST[kindof[b["m"]]] - 1) - 2 * TXF - BIAS
kindof = {}
trips = []
for x in T:
    tr = x["tr"]
    if x["open"] or len(tr) != 2 or not tr[0]["buy"] or tr[1]["buy"] or not tr[0]["vt"] or not tr[1]["vt"]: continue
    tr[0]["m"] = x["m"]; kindof[x["m"]] = x["kind"]; trips.append((x, tr[0], tr[1]))
def period(t):
    d = datetime.datetime.fromtimestamp(t, datetime.timezone.utc)
    return "P1 Oct 19-22" if d.day <= 22 else "P2 Oct 23-27"
for e in SLIPS:
    print(f"\n== entry slippage {e:+.0%} over the state after its buy")
    for kind in ("pump, other quote", "pump, SOL quote"):
        for ahead in (True, False):
            g = collections.defaultdict(list); its = collections.defaultdict(float)
            for x, b, s in trips:
                if x["kind"] != kind: continue
                v = copy(b, s, e, ahead)
                if v is None: continue
                g[period(b["bt"])].append(v); its[period(b["bt"])] += x["net"]
            cells = []
            for p in sorted(g):
                v = sorted(g[p], reverse=True)
                cells.append(f"{p}: {len(v)} copies {sum(v):+7.2f} (w/o best 2 {sum(v[2:]):+7.2f}, its own {its[p]:+7.2f})")
            print(f"   {kind:18s} {'exit before its sell' if ahead else 'exit after its sell ':20s} " + " | ".join(cells))
