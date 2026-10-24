import json, csv, time, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"; FEE = 0.0125
T = sorted(json.load(open(SP + "/today/trades.json")), key=lambda r: r["cs"]); HC = json.load(open(SP + "/today/hc.json"))
LU = json.load(open(SP + "/today/legs_us.json")); fired = {x["mint"] for x in LU if x["kind"] == "buy" and x["mint"]}
def val(st, q): vs, vt = st; return (vs - vs * vt / (vt + q)) * (1 - FEE) / 1e9
def early(r):
    es = {int(k): v for k, v in r["end_state"].items()}; bs = r["us"]["buy_slot"]; ks = [k for k in es if k <= bs + 1]
    tot = q = 0
    for s, i, u, b, sol, tok, vs, vt, ins in r["trades"]:
        if u == W and not b: tot += val((vs + sol, vt - tok), tok); q += tok
    return r["us"]["net"] + (val(es[max(ks)], q) - tot) if ks and q else r["us"]["net"]
def ins_sold(r, a, b): return sum(sol for s, i, u, bb, sol, tok, vs, vt, isin in r["trades"] if isin and not bb and a <= s <= b) / 1e9
def his_gap(r):
    h = sorted(r["him_try"], key=lambda x: (x["s"], x["i"] or 0))
    return (h[0]["i"] or 0) - r["us"]["buy_idx"] if h and h[0]["s"] == r["us"]["buy_slot"] else ""
out = SP + "/today/our_trades_2026-10-24.csv"
with open(out, "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["time_utc", "mint", "creator", "entry_slot", "rank", "post_bundle_vsol", "stake_sol", "net_sol", "net_pct", "held_slots",
                "he_fired", "his_buy_txs_behind_ours", "insiders_sold_sol_while_held", "rug", "net_if_sold_at_once_sol", "skip_by_cap_40"])
    for r in T:
        u = r["us"]
        if not (u and u["closed"]): continue
        pb = r["post_bundle"][0] / 1e9 if r["post_bundle"] else None
        hc = HC[r["mint"]]["hc"]
        w.writerow([time.strftime("%H:%M:%S", time.gmtime(u["bt"])), r["mint"], r["dev"], f"create+{u['buy_slot'] - r['cs']}", u.get("rank"),
                    f"{pb:.2f}" if pb else "", f"{u['cost']:.3f}", f"{u['net']:+.4f}", f"{100 * u['net'] / u['cost']:+.1f}",
                    (u["sell_slot"] - u["buy_slot"]) if u["sell_slot"] else "", {"failed behind us, same slot": "yes, our slot"}.get(hc, "no" if hc == "none" else hc.replace("failed, ", "later, ")),
                    his_gap(r), f"{ins_sold(r, u['buy_slot'], u['sell_slot'] or u['buy_slot'] + 15):.2f}", "yes" if u["net"] / u["cost"] < -0.2 else "",
                    "" if hc == "failed behind us, same slot" else f"{early(r):+.4f}", "yes" if pb and pb > 40 else ""])
out2 = SP + "/today/his_trades_2026-10-24.csv"
with open(out2, "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["time_utc", "mint", "creator", "entry_slot", "post_bundle_vsol", "stake_sol", "net_sol", "net_pct", "held_slots", "we_fired", "rug", "skip_by_cap_40"])
    for r in T:
        h = r["him"]
        if not (h and h["closed"]): continue
        pb = r["post_bundle"][0] / 1e9 if r["post_bundle"] else None
        w.writerow([time.strftime("%H:%M:%S", time.gmtime(h["bt"])), r["mint"], r["dev"], f"create+{h['buy_slot'] - r['cs']}", f"{pb:.2f}" if pb else "",
                    f"{h['cost']:.3f}", f"{h['net']:+.4f}", f"{100 * h['net'] / h['cost']:+.1f}", (h["sell_slot"] - h["buy_slot"]) if h["sell_slot"] else "",
                    "yes" if r["mint"] in fired else "", "yes" if h["net"] / h["cost"] < -0.2 else "", "yes" if pb and pb > 40 else ""])
print(out); print(out2)
