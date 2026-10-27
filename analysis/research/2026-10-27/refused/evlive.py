# Live, at our real positions (Oct 25 22:08 UTC on): each copy we sent, the copied wallet's buy before ours, the premium
# at our position, and what filling there would make at the copy's exit (him: one slot before his first sell, as the
# bot aims; the other wallets: create + their hold); the in-between buyers; a re-entry when the price comes back.
#   -> SP/refused/live_recs.json, SP/refused/land_him.json (his landing distances, for the 5-day model)
import json, os, bisect, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
FEE = 0.0125; COST = 0.0045
WAL = {"AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2": ("him", None, 0.05),
       "3yrJE1A8dgSuy5NLimG5GSYxA6wMkvnAYLD9Y4xR2fjJ": ("3yrJE1A8", 29, 0.05),
       "JBokzLFRg8CEcThrjweWa9WoZoZPYitRSVffXuxdHHVu": ("JBokzLFR", 5, 0.05),
       "H9kuqGKNYgPFYCA17uUTkaqXGum7MK8y1GwbS7cxsjDg": ("H9kuqGKN", 12, 0.20),
       "CxyKYmQoaYLCds6puxSbWndk6ktqax8PwAeq8enRzph8": ("CxyKYmQo", 12, 0.05),
       "D5BDDkBtjjva35nwu45WfmZgytsog9GST2tHQaZUYuFX": ("D5BDDkBt", 9, 0.05)}
US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
L = json.load(open(SP + "/refused/live.json"))["mine"]
def pnl(en, ex, stake=0.5):
    v0, t0 = en; k = v0 * t0; s_in = stake * 1e9 / (1 + FEE); q = t0 - k / (v0 + s_in)
    vr, tr = ex
    if tr <= q: return None
    return vr * q / (tr - q) * (1 - FEE) / 1e9 - stake - COST
def st_before(seq, key):
    i = bisect.bisect_left(seq, key, key=lambda x: (x[0], x[1]))
    return (seq[i - 1][6], seq[i - 1][7]) if i else None
def st_end(seq, slot): return st_before(seq, (slot + 1, -1))
recs = []; land_him = []; miss = collections.Counter()
for m, att in L.items():
    p = f"{SP}/refused/tape/{m}.json"
    if not os.path.exists(p): miss["no tape"] += 1; continue
    t = json.load(open(p)); seq = [r for r in t["rows"] if r[7] > 0]; cs = t["c0"]; end = cs + t.get("span", 60)
    att = sorted(att, key=lambda a: (a["slot"], a["idx"] or 0)); a0 = att[0]; pos = (a0["slot"], a0["idx"] or 0)
    trig = [x for x in seq if x[2] in WAL and x[3] == 1 and x[0] <= cs + 1 and (x[0], x[1]) < pos]
    if not trig: miss["no copied buy before ours"] += 1; continue
    w = trig[-1]; who, hold, bound = WAL[w[2]]
    wp = w[6] / w[7]; en = st_before(seq, pos)
    if not en: miss["no state"] += 1; continue
    prem = (en[0] / en[1]) / wp - 1
    ws = [x for x in seq if x[2] == w[2] and x[3] == 0]
    if who == "him":
        ex_slot = (ws[0][0] - 1) if ws else cs + 10
    else:
        ex_slot = cs + hold - 1
    ex_slot = max(ex_slot, pos[0])
    if ex_slot >= end: miss["exit past the tape"] += 1; continue
    ex = st_end(seq, ex_slot)
    status = "filled" if not a0["err"] else ("refused" if a0["code"] == "6042" else "failed")
    btw = [x for x in seq if (w[0], w[1]) < (x[0], x[1]) < pos]
    v = dict(fill=pnl(en, ex), fill02=pnl(en, ex, 0.2))
    for ek in (3, 5, 7, 12, 15, 20):
        e2 = st_end(seq, max(cs + ek - 1, pos[0]))
        v[f"c{ek}"] = pnl(en, e2) if e2 and cs + ek - 1 < end else None
    for xr in (0.0, 0.03, 0.05):
        v[f"re{xr}"] = None
        last = min(ex_slot - 2, (ws[0][0] - 2) if ws else 10**12)
        for sl in range(pos[0], last + 1):
            e1 = st_end(seq, sl)
            if not e1 or (e1[0] / e1[1]) / wp - 1 > xr: continue
            e2 = st_before(seq, (sl + 1, 100))
            if (e2[0] / e2[1]) / wp - 1 > max(bound, 0.05): continue
            v[f"re{xr}"] = pnl(e2, ex); v[f"re{xr}:slot"] = sl + 1 - cs
            break
    if who == "him": land_him.append((w[1], pos[0] - w[0], pos[1]))
    recs.append(dict(m=m, who=who, bt=a0["bt"], status=status, prem=prem, bound=bound, at=w[0] - cs, dslot=pos[0] - w[0],
                     behind=(pos[1] - w[1]) if pos[0] == w[0] else None, w_sol=w[4] / 1e9, v=v,
                     n_btw=sum(1 for x in btw if x[3] == 1), sol_btw=sum(x[4] for x in btw if x[3] == 1) / 1e9,
                     btw_users=[x[2] for x in btw if x[3] == 1], sells_btw=sum(1 for x in btw if x[3] == 0),
                     w_sold=bool(ws), w_first_sell=(ws[0][0] - cs) if ws else None, ex_slot=ex_slot - cs))
json.dump(recs, open(SP + "/refused/live_recs.json", "w")); json.dump(land_him, open(SP + "/refused/land_him.json", "w"))
print(len(recs), "copies", dict(miss), collections.Counter((r["who"], r["status"]) for r in recs).most_common())
