# Q1: where his buy shows up relative to ours, and what selling early (when he did not fire) would have made.
import json, statistics as St, math, collections, time
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
FEE = 0.0125
T = json.load(open(SP + "/today/trades.json"))
def ci(v):
    if len(v) < 2: return f"n={len(v)}" + (f" {100*v[0]:+.1f}%" if v else "")
    m = St.mean(v); h = 1.96 * St.pstdev(v) / math.sqrt(len(v))
    return f"n={len(v):3d} wins {sum(1 for x in v if x > 0):3d}  mean {100*m:+6.2f}% [{100*(m-h):+6.2f},{100*(m+h):+6.2f}]  median {100*St.median(v):+6.2f}%"
def val(st, q):  # SOL from selling q tokens at curve state st=(vs, vt), after the curve fee
    vs, vt = st; return (vs - vs * vt / (vt + q)) * (1 - FEE) / 1e9
def his_class(r):
    u = r["us"]; bs, bi = u["buy_slot"], u["buy_idx"]
    h = sorted(r["him_try"], key=lambda x: (x["s"], x["i"] or 0))
    if not h: return "none", None
    x = h[0]
    if not x["err"]: return "he landed", x["s"] - bs
    if x["s"] == bs and (x["i"] or 0) > bi: return "failed behind us, same slot", 0
    if x["s"] > bs: return f"failed, slot +{x['s'] - bs}", x["s"] - bs
    return "failed before our buy", x["s"] - bs
U = [r for r in T if r["us"] and r["us"]["closed"]]
print(f"our closed trades today: {len(U)}; SOL {sum(r['us']['net'] for r in U):+.4f}")
groups = collections.defaultdict(list)
for r in U:
    c, d = his_class(r); r["hc"] = c; r["hd"] = d
    groups[c].append(r)
print("\n== our trades by what HE did on the same launch (chain-true, after the landed txs' fees and tips)")
for c, G in sorted(groups.items(), key=lambda kv: -len(kv[1])):
    print(f"  {c:30s} {ci([r['us']['net'] / r['us']['cost'] for r in G])}  SOL {sum(r['us']['net'] for r in G):+.4f}")
# where exactly his failed buy lands relative to ours in the same slot
gap = [ (sorted(r["him_try"], key=lambda x: (x["s"], x["i"] or 0))[0]["i"] or 0) - r["us"]["buy_idx"] for r in groups["failed behind us, same slot"]]
if gap: print(f"  his failed buy in our slot sits {St.median(gap):.0f} transactions behind ours (median; min {min(gap)}, max {max(gap)})")
errs = collections.Counter(x["errv"] for r in U for x in r["him_try"] if x["err"])
print("  his failed-buy errors:", dict(errs.most_common(4)))
# his bids on the races he lost to us vs the races he won today
lost = [x for r in U for x in r["him_try"] if x["err"]]
print(f"  his bid when we beat him: tip median {St.median([x['tip'] for x in lost]):.2f}M, priority median {St.median([x['prio'] * 1000 for x in lost]):.3f}M" if lost else "")
ours_bid = [(r["us"]["tip"] / 1e6, r["us"]["prio"] * 1000) for r in U]
print(f"  our landed buy: tip median {St.median([a for a, b in ours_bid]):.2f}M, priority median {St.median([b for a, b in ours_bid]):.2f}M")
# Q1b: sell early when he has not fired by the end of slot bs+k; the sell executes at the end of slot bs+k+1 (pessimistic)
# or at the end of slot bs+k (optimistic). net_cf = actual net + (value of the early sell - value of the actual sell).
def sells_value(r):
    """formula value of our actual sells, at the curve state right before each of our sells"""
    tot = 0.0; q_tot = 0
    for s, i, u, b, sol, tok, vs, vt, ins in r["trades"]:
        if u == "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff" and not b:
            pre = (vs + sol, vt - tok); tot += val(pre, tok); q_tot += tok
    return tot, q_tot
def state_at_end(r, slot):
    es = {int(k): v for k, v in r["end_state"].items()}
    ks = [k for k in es if k <= slot]
    return es[max(ks)] if ks else None
def cf(r, k, optimistic):
    u = r["us"]; bs = u["buy_slot"]
    actual, q = sells_value(r)
    if not q: return None
    # our own buy is in the tape; the state after slot X includes it; our actual sell is later, so exclude nothing
    st = state_at_end(r, bs + k + (0 if optimistic else 1))
    if st is None: return None
    return u["net"] + (val(st, q) - actual)
print("\n== Q1b: sell at once when he has not fired (no buy from him on the launch by the end of our slot + k)")
nf = [r for r in U if r["hc"] == "none" or (r["hd"] is not None and r["hd"] > 0)]
for k in (0, 1, 2):
    for opt in (True, False):
        rows = []
        for r in U:
            fired_by = r["hd"] is not None and r["hd"] <= k
            if fired_by:
                rows.append((r, r["us"]["net"]))
            else:
                v = cf(r, k, opt)
                rows.append((r, v if v is not None else r["us"]["net"]))
        exited = [(r, v) for r, v in rows if not (r["hd"] is not None and r["hd"] <= k)]
        print(f"  k={k} sell {'at the end of slot +%d (optimistic)' % k if opt else 'at the end of slot +%d (pessimistic)' % (k + 1):40s}: "
              f"exited {len(exited):2d} trades, they make {sum(v for r, v in exited):+.4f} SOL instead of {sum(r['us']['net'] for r, v in exited):+.4f};"
              f" day total {sum(v for r, v in rows):+.4f} SOL (actual {sum(r['us']['net'] for r in U):+.4f})")
json.dump({r["mint"]: dict(hc=r["hc"], hd=r["hd"]) for r in U}, open(SP + "/today/hc.json", "w"))
