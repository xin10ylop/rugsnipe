# Calibration: the replay's price model on our real copies of wallet 4 (our real landing and sell slot) against our real
# net on chain. Then the same coins at other exits from our real landing.
import json, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
exec(open(SP + "/h9/replay.py").read().split("EXITS = ")[0].replace("W = sys.argv[1]", "W = 'H9kuqGKNYgPFYCA17uUTkaqXGum7MK8y1GwbS7cxsjDg'").replace("PREMS = [float(x) for x in sys.argv[2:]] or [0.05]", "PREMS = [0.05]"))
L = {x["m"]: x for x in json.load(open(SP + "/h9/land.json"))["H9kuqGKN"]}
tot_real = tot_model = 0; n = 0; alt = {h: 0 for h in (5, 9, 12, 15, 20, 30, 40)}; alt["first"] = 0
for m, x in L.items():
    p = f"{SP}/h9/coin/{m}.json"
    try: c = json.load(open(p))
    except OSError: continue
    seq = c.get("rows") or []
    if not seq or not x["sell_s"]: continue
    en = state_before(seq, (x["our_s"], x["our_i"]))
    ex = state_before(seq, (x["sell_s"], 0))
    v = pnl(en, ex) if en and ex else None
    if v is None: continue
    n += 1; tot_real += x["net"] / 1e9; tot_model += v
    cs = c["cs"]; sells = [t["s"] for t in own[m] if not t["buy"]]
    for h in alt:
        s = (sells[0] + 1 if sells else c["end"]) if h == "first" else cs + h
        s = min(max(s, x["our_s"] + 1), c["end"])
        e2 = state_end(seq, s - 1); w = pnl(en, e2) if e2 else None
        alt[h] += w or 0
    print(f"   {m[:8]}  real {x['net'] / 1e9:+.4f}  model {v:+.4f}")
print(f"== {n} real copies: real net {tot_real:+.4f}, model at our real landing and sell {tot_model:+.4f}")
print("   the same fills sold at: " + "  ".join(f"{('create+' + str(h)) if h != 'first' else 'its 1st sell+1'} {v:+.3f}" for h, v in alt.items()))
