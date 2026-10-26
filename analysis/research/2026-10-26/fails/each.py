# Each coin we sent a copy on since the start: the copied buy, where ours landed, who was between, and what each way
# in would have made: filled where we landed (no limit), limits of 10 / 15 / 25%, landing right behind the copied buy,
# and re-entering on a pullback to the copied price + 5% after a failure. 0.5 SOL, 1.25% fee each way, 3M buy tip,
# 0.5M sell tip; sold at the copy's exit (him: create + 9; 3yrJE1A8: create + 29; JBokzLFR: create + 5).
import json, sys, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
D = json.load(open(SP + "/fails/coins.json"))
WAL = {"AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2": ("him", 9), "3yrJE1A8dgSuy5NLimG5GSYxA6wMkvnAYLD9Y4xR2fjJ": ("3yr", 29),
       "JBokzLFRg8CEcThrjweWa9WoZoZPYitRSVffXuxdHHVu": ("JBok", 5)}
US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
FEE = 0.0125; STAKE = 0.5; COST = 0.003 + 0.0005 + 0.00002
K = 30_000_000_000 * 1_073_000_000_000_000
def st(r): return (r[6] if r[6] > 0 else K // r[7], r[7])
def px(s): return s[0] / s[1]
def pnl(en, ex):
    v0, t0 = en; k = v0 * t0; s_in = STAKE * 1e9 / (1 + FEE); q = t0 - k / (v0 + s_in); vr, tr = ex
    return None if tr <= q else vr * q / (tr - q) * (1 - FEE) / 1e9 - STAKE - COST
def state_at_end(rows, slot):
    s = None
    for r in rows:
        if r[0] > slot: break
        if r[7] > 0: s = st(r)
    return s
def state_before(rows, slot, idx):
    s = None
    for r in rows:
        if (r[0], r[1]) >= (slot, idx): break
        if r[7] > 0: s = st(r)
    return s
out = []
for mint, atts in D["mine"].items():
    tp = D["tapes"].get(mint)
    if not tp or not tp["rows"]: continue
    rows = [r for r in tp["rows"] if r[7] > 0]; c0 = tp["c0"]
    # the copied buy: the first buy by a copied wallet at create + 0/1 before our first attempt
    a0 = min(atts, key=lambda a: (a["slot"], a["idx"] or 0))
    lead = next((r for r in rows if r[3] == 1 and r[2] in WAL and r[0] <= c0 + 1 and (r[0], r[1]) < (a0["slot"], a0["idx"] or 0)), None)
    if not lead: continue
    who, H = WAL[lead[2]]; E_ = c0 + H; pL = px(st(lead))
    ours = [r for r in rows if r[2] == US and r[3] == 1]
    filled = bool(ours)
    a = a0
    sb = state_before(rows, a["slot"], a["idx"] or 0); prem = px(sb) / pL - 1
    between = [r for r in rows if (lead[0], lead[1]) < (r[0], r[1]) < (a["slot"], a["idx"] or 0)]
    nb = sum(1 for r in between if r[3] == 1); sol_b = sum(r[4] for r in between if r[3] == 1) / 1e9
    ex = state_at_end([r for r in rows if r[2] != US], E_)
    v_land = pnl(sb, ex) if ex else None
    nxt = next((r for r in rows if (r[0], r[1]) > (lead[0], lead[1])), None)
    behind = st(lead)           # the state right after the copied buy: landing right behind it
    v_behind = pnl(behind, ex) if ex else None
    # re-entry after the failure: the first state at or under the copied price + 5% before exit - 2
    v_re = None; re_at = None
    if not filled:
        for r in rows:
            if (r[0], r[1]) <= (a["slot"], a["idx"] or 0) or r[0] > E_ - 2: continue
            if px(st(r)) <= pL * 1.05:
                v_re = pnl(st(r), ex) if ex else None; re_at = r[0] - c0; break
    mx = max((px(st(r)) for r in rows if (a["slot"], a["idx"] or 0) < (r[0], r[1]) and r[0] <= E_), default=pL)
    out.append(dict(m=mint[:8], who=who, lead_at=lead[0] - c0, lead_idx=lead[1], land_at=a["slot"] - c0, land_idx=a["idx"],
                    same_slot=a["slot"] == lead[0], filled=filled, code=a.get("code"), prem=prem, n_between=nb, sol_between=sol_b,
                    v_land=v_land, v_behind=v_behind, v_re=v_re, re_at=re_at, peak=mx / pL - 1, exit=(px(ex) / pL - 1) if ex else None,
                    lead_sol=lead[4] / 1e9))
json.dump(out, open(SP + "/fails/each.json", "w"))
def f(x, p=False): return "-" if x is None else (f"{x:+.0%}" if p else f"{x:+.3f}")
for who in ("him", "3yr", "JBok"):
    rs = [r for r in out if r["who"] == who]
    if not rs: continue
    print(f"== copies of {who}: {len(rs)} sent, {sum(r['filled'] for r in rs)} filled, {sum(not r['filled'] for r in rs)} failed")
    print("   mint      its  ours        premium  between(buys, SOL)  peak   at exit | filled where we landed | right behind it | re-entry")
    for r in sorted(rs, key=lambda r: r["filled"]):
        land = f"+{r['land_at']} #{r['land_idx']}" + (" same" if r["same_slot"] else "")
        print(f"   {r['m']} +{r['lead_at']} #{r['lead_idx']:<4} {land:14} {f(r['prem'], True):>6}  {r['n_between']:3d} {r['sol_between']:6.2f}       "
              f"{f(r['peak'], True):>6} {f(r['exit'], True):>6} | {'FILLED' if r['filled'] else 'failed'} {f(r['v_land'])} | {f(r['v_behind'])} | "
              f"{f(r['v_re'])}{'' if r['re_at'] is None else ' at +' + str(r['re_at'])}")
