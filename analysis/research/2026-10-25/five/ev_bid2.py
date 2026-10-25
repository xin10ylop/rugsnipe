# EV of a flat bid with displacement: when we take the first spot from a guarded winner (him: exact-state guard), his
# buy and sell drop out of the tape. Value at +9 (the slot before his +10 sells) with his buy removed when he still
# held, our sell at that state; plus the tail: how much of the net comes from the top coins.
import json, collections, statistics as st
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
import sys
AU = json.load(open(sys.argv[1] if len(sys.argv) > 1 else "big/auction.json")); R = json.load(open("an_rows.json"))
W = json.load(open(f"walk_AHwJSn.json"))["rows"]
his = collections.defaultdict(list)
for r in W: his[r["m"]].append(r)
FEE = 0.0125
def curve_trade(v, t, sol_in):
    k = v * t; v1 = v + sol_in; return v1, k / v1, t - k / v1
def value(m, stake, B, hold=9):
    r = R[m]; d = json.load(open(f"coin/{m}.json")); cs = d["cs"]
    v0 = r["pb"] * 1e9; t0 = v0 / r["p0"]
    vh = r.get(f"v{hold}")
    if vh is None: return None
    ph = r["p0"] * (1 + vh)
    # displacement: his landed buy at the first spot, still held at create + hold -> remove its price impact
    if AU[m]["win"] == "HIM":
        hb = [x for x in his[m] if x["kind"] == "buy" and x["ok"] and x.get("fill")]
        hs = [x for x in his[m] if x["kind"] == "sell" and x["ok"] and x["slot"] - cs <= hold]
        if hb and not hs:
            s = hb[0]["fill"][0]
            ph = ph * (v0 / (v0 + s)) ** 2
    # our buy at the post-bundle state, the market's move to ph applied to the curve with our tokens in, our sell
    v1, t1, tok = curve_trade(v0, t0, stake * 1e9 * (1 - FEE))
    pa = v1 / t1; scale = ph / r["p0"]
    kk = v1 * t1; ps = pa * scale; vv = (kk * ps) ** 0.5; tt = (kk / ps) ** 0.5
    out = (vv - kk / (tt + tok)) * (1 - FEE)
    return out / 1e9 - stake - B / 1e3
def other_max(x):
    if x["win"] != "US": return x["win_bid"]
    o = [l[1] for l in x["lost"] if l[0] != "US"]
    return max(o) if o else 0.0
print(f"{'bid':>5} {'stake':>5} {'wins':>5} {'net':>8} {'net/win':>8} {'median':>8} {'net w/o top5':>12} {'net w/o top10':>13} {'losing wins':>11}")
for stake in (0.49, 1.0):
    for B in (1, 4, 8, 12, 16, 24, 32):
        won = [m for m, x in AU.items() if B > other_max(x)]
        vals = sorted(v for v in (value(m, stake, B) for m in won) if v is not None)
        n = sum(vals)
        print(f"{B:4d}M {stake:5.2f} {len(vals):5d} {n:+8.3f} {n/len(vals):+8.4f} {st.median(vals):+8.4f} {sum(vals[:-5]):+12.3f} {sum(vals[:-10]):+13.3f} {sum(v < 0 for v in vals):11d}")
    print()

# per signer and by the operator's own bundle tip (seen before we fire)
RC = json.load(open("big/race.json"))
def btip(m):
    d = json.load(open(f"coin/{m}.json")); c = d["create"]
    ins = {c["signer"], c["creator"]} | set(R[m]["bw"])
    return sum(r["tip"] for r in RC[m] if r["payer"] in ins and r["slot"] == 0 and r["ok"]) / 1e6
BT = {m: btip(m) for m in AU}
print("\nper group: flat bid B, stake 0.49 / 1.0 -> wins, net, net without the top 3")
groups = {} if len(sys.argv) > 2 else {"8ZN71X": lambda m: R[m]["signer"].startswith("8ZN71X"), "JDQKDr": lambda m: R[m]["signer"].startswith("JDQKDr"),
          "7naFFw": lambda m: R[m]["signer"].startswith("7naFFw"), "other signers": lambda m: not R[m]["signer"].startswith(("8ZN71X", "JDQKDr", "7naFFw")),
          "bundle tip <= 12M": lambda m: BT[m] <= 12, "bundle tip > 12M": lambda m: BT[m] > 12}
for gname, f in groups.items():
    ms = [m for m in AU if f(m)]
    line = []
    for B in (4, 12, 24):
        for stake in (0.49, 1.0):
            won = [m for m in ms if B > other_max(AU[m])]
            vals = sorted(v for v in (value(m, stake, B) for m in won) if v is not None)
            line.append(f"{B}M/{stake:.2f}: {len(vals):3d} {sum(vals):+6.2f} ({sum(vals[:-3]):+6.2f})")
    print(f"  {gname:18} n {len(ms):3d} | " + " | ".join(line))
