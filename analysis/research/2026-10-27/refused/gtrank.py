# Ghost targets, stage 1 ranking (gt1.json): per wallet and exit, the ghost copy's value on the early tapes (before
# Oct 25 00:00 UTC) and the late ones (Oct 25 on), each without its best 2 coins. Picked on early, scored on late; the
# ghost spray is charged later (it depends on how many launches get ghosts). Also our six wallets, and every wallet's
# best exit picked on early.
import json, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
G = json.load(open(SP + "/refused/gt1.json"))
T1 = 1792886400
HS = (3, 5, 7, 9, 12)
SIX = {"AHwJSnFN": "him", "3yrJE1A8": "2 3yr", "JBokzLFR": "3 JBo", "H9kuqGKN": "4 H9", "CxyKYmQo": "5 Cxy", "D5BDDkBt": "6 D5"}
def s(xs, h, drop=2):
    v = sorted((x[f"g{h}"] for x in xs if x.get(f"g{h}") is not None), reverse=True)
    return sum(v[drop:]), len(v)
rows = []
for w, xs in G.items():
    e = [x for x in xs if x["t"] < T1]; l = [x for x in xs if x["t"] >= T1]
    if len(e) < 15 or len(l) < 15: continue
    best = max(HS, key=lambda h: s(e, h)[0])
    ev, en = s(e, best); lv, ln = s(l, best)
    rows.append((ev, lv, w, best, en, ln, s(e, best, 0)[0], s(l, best, 0)[0]))
rows.sort(reverse=True)
print(f"wallets with 15+ launch buys in both periods: {len(rows)}")
pos = [r for r in rows if r[0] > 0]
print(f"positive early (without its best 2): {len(pos)}; of those positive late (without its best 2): {sum(1 for r in pos if r[1] > 0)}")
print("   early-2  late-2   early   late   wallet    exit  n early/late")
for r in rows[:25]:
    print(f"   {r[0]:+6.2f}  {r[1]:+6.2f}  {r[6]:+6.2f} {r[7]:+6.2f}  {r[2][:8]}  c{r[3]:<3} {r[4]}/{r[5]}")
print("\nlate survivors (positive in both, without each period's best 2), by late value:")
for r in sorted([r for r in pos if r[1] > 0], key=lambda r: -r[1])[:20]:
    print(f"   {r[2]}  exit c{r[3]}  early {r[0]:+.2f} ({r[4]})  late {r[1]:+.2f} ({r[5]})  per late buy {r[1] / r[5]:+.4f}")
print("\nour six at the ghost position (every launch buy on the tapes, no key), per exit: early / late [without best 2]")
for w8, lab in SIX.items():
    w = next((k for k in G if k.startswith(w8)), None)
    if not w: continue
    xs = G[w]; e = [x for x in xs if x["t"] < T1]; l = [x for x in xs if x["t"] >= T1]
    print(f"   {lab:7} n {len(e)}/{len(l)}  " + "  ".join(f"c{h} {s(e, h)[0]:+.2f}/{s(l, h)[0]:+.2f}" for h in HS))
