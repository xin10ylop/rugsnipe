# The early files store no pump fee per trade (ev = user, is_buy, sol, tok, vs, vt). On coins whose full tape (with
# fee + creator fee) exists, recompute the first outside buyer's value with and without the fees: the backtest's bias.
import json, glob, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
FEE = 0.0125; A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
creates = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): creates[c["mint"]] = c
rows24 = {x["mint"]: x for x in json.load(open(SP + "/univ/rows_24.json"))}
tapes = {}
for f in ("tapes.jsonl", "tapes2.jsonl"):
    for l in open(f"{SP}/today/{f}"):
        o = json.loads(l); tapes[o["mint"]] = o
def seq_tape(o, fees=True):
    rows = sorted(o["txs"], key=lambda t: (t["s"], t["i"] if t["i"] is not None else 10**6))
    return [(t["s"], t["i"] or 0, e[1], bool(e[2]), e[3], e[4], e[5], e[6], e[9] if fees else 0, e[10] if fees else 0) for t in rows if not t["err"] for e in t["ev"] if e[0] == "T" and len(e) <= 12]
def val(seq, who, hz):
    spent = got = held = 0; st = None
    for s, i, u, b, sol, tok, vs, vt, f1, f2 in seq:
        if s > hz: break
        st = (vs, vt)
        if u != who: continue
        if b: spent += sol + f1 + f2; held += tok
        else: got += sol - f1 - f2; held -= tok
    if not spent: return None
    mark = (st[0] - st[0] * st[1] / (st[1] + held)) * (1 - FEE) if held > 0 else 0
    return (got + mark) / spent - 1
d = collections.defaultdict(list)
for m, x in rows24.items():
    if x["src"] != "tape" or x["d"] is None or m not in tapes: continue
    c = creates[m]; cs = c["slot"]
    fo_slot = cs + x["d"]
    who = {"him": A, "us": US}.get(x["who"])
    seq_f = seq_tape(tapes[m], True); seq_n = seq_tape(tapes[m], False)
    # the first outside buyer of rows.py: the first buy after the bundle by a non-insider; take it from the stored row
    fo = next((s for s in seq_f if s[0] == fo_slot and s[3]), None)
    if not fo: continue
    for k in (1, 3, 10):
        a, b = val(seq_f, fo[2], fo[0] + k), val(seq_n, fo[2], fo[0] + k)
        if a is not None and b is not None: d[k].append((a, b))
for k, v in sorted(d.items()):
    print(f"hold {k:2d}: n {len(v)}  with fees {100*sum(a for a, b in v)/len(v):+6.2f}%  without {100*sum(b for a, b in v)/len(v):+6.2f}%  bias {100*sum(b - a for a, b in v)/len(v):+5.2f} points")
src = collections.Counter(x["src"] for x in rows24.values()); print("Oct 24 rows by source:", dict(src))
print("Oct 23 rows by source:", dict(collections.Counter(x["src"] for x in json.load(open(SP + "/pm/bak/rows_23.json")))))
