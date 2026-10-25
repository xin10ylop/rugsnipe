# EV of a flat bid B on the 402 coins: we take the first spot iff B beats every other bid in the race (bid order holds
# in 52 of our 60 races), then sell at +10 (the operator table's hold for these signers) at the curve price, paying
# fees and our own price impact both ways. Calibrated on our live fills.
import json, collections, statistics as st
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
AU = json.load(open("big/auction.json")); R = json.load(open("an_rows.json"))
FEE = 0.0125     # pump fee each way (protocol + creator) on the curve
def value(m, stake, hold=10):
    # buy `stake` SOL at the post-bundle state, sell all at create + hold at that slot's state (constant product)
    r = R[m]; v0 = r["pb"] * 1e9; p0 = r["p0"]; t0 = v0 / p0
    k = v0 * t0; s_in = stake * 1e9 * (1 - FEE)
    v1 = v0 + s_in; t1 = k / v1; tok = t0 - t1
    vh = r.get(f"v{hold}")
    if vh is None: return None
    # the market moved the price by (1 + vh) from p0; apply it to the curve with our tokens in: price ph = p0 (1+vh)
    # state with our tokens held: approximate by scaling the post-buy state's price
    p_after = v1 / t1; ph = p_after * (1 + vh)
    # sell tok at price ph on a curve of the same depth: v = sqrt(k' * ph), t = sqrt(k'/ph) with k' = v1 t1
    kk = v1 * t1; vv = (kk * ph) ** 0.5; tt = (kk / ph) ** 0.5
    out = vv - kk / (tt + tok)
    return out * (1 - FEE) / 1e9 - stake
def other_max(x):
    # the bid we must beat: the winner's when it is not us, else the best failed attempt of someone else
    if x["win"] != "US": return x["win_bid"]
    o = [l[1] for l in x["lost"] if l[0] != "US"]
    return max(o) if o else 0.0
# calibration: our live results on the coins we won vs the model at our real stake
cal = []
for m, x in AU.items():
    if x["win"] == "US" and x["us_ok"]:
        v = value(m, 0.49)
        if v is not None: cal.append((x["us_net"], v - x["win_bid"] / 1e3))
print(f"calibration on our {len(cal)} wins: live net {sum(a for a, b in cal):+.3f} SOL, model {sum(b for a, b in cal):+.3f} SOL")
print(f"\n{'bid':>6} {'stake':>5} {'wins':>5} {'v10 of wins':>11} {'gross':>8} {'bids paid':>9} {'net':>8} {'net/win':>8} {'his on them':>11}")
for stake in (0.49, 1.0):
    for B in (1, 2, 4, 6, 8, 10, 12, 16, 20, 24, 32, 48, 64):
        won = [m for m, x in AU.items() if B > other_max(x)]
        vals = [value(m, stake) for m in won]; vals = [v for v in vals if v is not None]
        gross = sum(vals); paid = len(won) * B / 1e3
        v10 = [AU[m]["v10"] for m in won]
        his = sum(1 for m in won if AU[m]["win"] == "HIM")
        print(f"{B:5d}M {stake:5.2f} {len(won):5d} {100*st.mean(v10) if v10 else 0:+10.1f}% {gross:+8.3f} {paid:9.3f} {gross - paid:+8.3f} {(gross - paid)/max(1,len(won)):+8.4f} {his:11d}")
    print()
