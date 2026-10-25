# Copiers of AHwJSnFN on the 5-day tapes: wallets whose buys land after his in his slot or the next, on his coins, and
# rarely on the other coins of the set. Per wallet: his coins bought after him, their position D after his buy (block
# index; the next slot apart), and how many of the set's other coins it bought (precision).
import json, glob, collections, statistics as st, os
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
tapes = {}
for d in ("full", "fp"):
    for p in glob.glob(f"{SP}/{d}/*.json"):
        t = json.load(open(p))
        if not isinstance(t, dict) or "rows" not in t: continue
        if t["mint"] not in tapes or len(t["rows"]) > len(tapes[t["mint"]]["rows"]): tapes[t["mint"]] = t
his = {}; buys = collections.defaultdict(list)   # wallet -> [(mint, slot, idx)]
for m, t in tapes.items():
    cs = t["cs"]
    for slot, idx, payer, evs in t["rows"]:
        for e in evs:
            if not e[1]: continue
            if e[0] == A:
                if m not in his and slot <= cs + 1: his[m] = (slot, idx, cs)
            else:
                buys[e[0]].append((m, slot, idx))
print(f"{len(tapes)} tapes, his launch buys on {len(his)}")
stats = []
for w, xs in buys.items():
    first = {}
    for m, slot, idx in sorted(xs, key=lambda x: (x[1], x[2])):
        first.setdefault(m, (slot, idx))
    after = []; before_or_none = 0; other = 0
    for m, (slot, idx) in first.items():
        if m in his:
            hs, hi, cs = his[m]
            if (slot, idx) > (hs, hi) and slot <= hs + 1: after.append((slot - hs, idx - hi if slot == hs else None))
            else: before_or_none += 1
        else:
            other += 1
    if len(after) >= 15:
        same = [d for s, d in after if s == 0]
        stats.append(dict(w=w, after=len(after), other=other, before=before_or_none, prec=len(after) / (len(after) + other + before_or_none),
                          same=len(same), d_med=st.median(same) if same else None, d_p10=sorted(same)[len(same) // 10] if same else None))
stats.sort(key=lambda s: -s["after"] * s["prec"])
print(f"{'wallet':10} {'his_after':>9} {'other':>6} {'ahead':>6} {'prec':>5} {'same_slot':>9} {'D_med':>6} {'D_p10':>6}")
for s in stats[:30]:
    print(f"{s['w'][:8]:10} {s['after']:9d} {s['other']:6d} {s['before']:6d} {s['prec']:5.2f} {s['same']:9d} {str(s['d_med']):>6} {str(s['d_p10']):>6}")
json.dump(stats, open(f"{SP}/dec/copiers/stats.json", "w"))
json.dump({m: v for m, v in his.items()}, open(f"{SP}/dec/copiers/his.json", "w"))
