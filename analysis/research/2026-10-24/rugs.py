# Q2: every trade (ours or his) that lost > 20% today: who sold, when, how much; the dev's earlier launches today.
import json, statistics as St, math, collections, time
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"; A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"
T = json.load(open(SP + "/today/trades.json"))
T.sort(key=lambda r: r["cs"])
bydev = collections.defaultdict(list)
for r in T: bydev[r["dev"]].append(r)
def dump_profile(r, from_slot, upto):
    """insider (creator set + bundle wallet) sells and big outside sells between from_slot and upto"""
    ins_out = 0; ins_first = None; out_big = []
    for s, i, u, b, sol, tok, vs, vt, isin in r["trades"]:
        if s < from_slot or s > upto or b or u in (W, A): continue
        if isin:
            ins_out += sol
            if ins_first is None: ins_first = s
        elif sol > 1e9: out_big.append((s, sol / 1e9))
    return ins_out / 1e9, ins_first, out_big
def result(r, who): return r[who]["net"] / r[who]["cost"]
bad = []
for r in T:
    for who in ("us", "him"):
        if r[who] and r[who]["closed"] and result(r, who) < -0.20: bad.append((r, who))
print(f"trades below -20% today: {len(bad)} ({sum(1 for r, w in bad if w == 'us')} ours, {sum(1 for r, w in bad if w == 'him')} his)")
seen = set()
for r, who in bad:
    t = r[who]; bs = t["buy_slot"]; ss = t["sell_slot"]
    ins_sol, ins_first, big = dump_profile(r, bs, ss or bs + 15)
    prev = [p for p in bydev[r["dev"]] if p["cs"] < r["cs"]]
    def prev_res(p):
        return " ".join(f"{w}:{100*result(p, w):+.0f}%" for w in ("us", "him") if p[w] and p[w]["closed"])
    print(f"\n{time.strftime('%H:%M', time.gmtime(t['bt']))} {r['mint'][:8]} {who:3s} {100*result(r, who):+6.1f}% ({t['net']:+.3f} SOL, stake {t['cost']:.2f})"
          f"  dev {r['dev'][:8]} dev buy {r['dev_sol']:.2f}  bundle {r['bundle_sol']:.2f} SOL / {r['n_bw']} wallets  entry create+{bs - r['cs']}"
          f"  held {(ss - bs) if ss else '-'} slots  other wallet {'us' if who == 'him' and r['us'] else ('him' if who == 'us' and r['him'] else '-')}")
    print(f"   insiders sold {ins_sol:.2f} SOL while held, first at buy+{(ins_first - bs) if ins_first is not None else '-'}; big outside sells {[(s - bs, round(x, 2)) for s, x in big][:4]}")
    print(f"   this dev's earlier launches today: {len(prev)}" + (": " + "; ".join(f"{time.strftime('%H:%M', time.gmtime(p['us']['bt'] if p['us'] else p['him']['bt']))} {prev_res(p)}" for p in prev[-6:]) if prev else ""))
