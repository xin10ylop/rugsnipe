# Per copy: the landed buys between the copied buy and ours, in the copied slot and the next, with their priority fee and
# tip (SOL), against ours; and the next-slot race: our place among the buyers that opened the next slot.
import json
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
B = json.load(open(SP + "/fails/bids.json")); EACH = {r["m"]: r for r in json.load(open(SP + "/fails/each.json"))}
D = json.load(open(SP + "/fails/coins.json"))
US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
buyers = {}
for mint, tp in D["tapes"].items():
    if tp: buyers[mint] = {(r[0] - tp["c0"], r[1]): (r[2], r[3], r[4]) for r in tp["rows"]}
for mint, rows in B.items():
    if not rows: continue
    e = EACH[mint[:8]]
    if e["who"] == "JBok": continue
    lead = (e["lead_at"], e["lead_idx"]); ours = (e["land_at"], e["land_idx"])
    by = buyers.get(mint, {})
    us = [r for r in rows if r[2] == US]
    between = [r for r in rows if lead < (r[0], r[1]) < ours and by.get((r[0], r[1]), (None, 0))[1] == 1]
    tag = "FILLED" if e["filled"] else "failed"
    u = us[0] if us else None
    print(f"{e['m']} {e['who']:4} {tag}  its +{lead[0]}#{lead[1]}  ours +{ours[0]}#{ours[1]}  premium {e['prem']:+.0%}  "
          f"our prio {u[3] / 1e9:.4f} tip {u[4] / 1e9:.4f}" if u else f"{e['m']} (ours not found)")
    for r in between:
        w, b, sol = by.get((r[0], r[1]), ("?", 0, 0))
        print(f"      between +{r[0]}#{r[1]:<4} {r[2][:8]} buy {sol / 1e9:5.2f} SOL  prio {r[3] / 1e9:.4f}  tip {r[4] / 1e9:.4f}")
