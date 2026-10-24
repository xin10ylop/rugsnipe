# Corrected backtest core (2026-10-24 evening): complete first-12-slot tapes with the real pump fee (full/<mint>.json),
# the first outside buyer behind the dev bundle, its realized value at every sell slot; with each coin's tweet status,
# the copied wallet's own action and his ex-ante history on the deployer (his create+H setting, P&L, stake level).
import json, glob, os, collections, bisect, random
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
FEE = 0.0125
D0 = {23: 1792713600, 24: 1792800000}
def load():
    cr = {}
    for f in glob.glob(SP + "/univ/creates_*.json"):
        for c in json.load(open(f)): cr[c["mint"]] = c
    T = sorted([t for t in json.load(open(SP + "/month/AHwJ_T.json")) if t["stake"] >= 0.1 and t["hold"] is not None], key=lambda t: t["bt"])
    his_by_mint = {t["mint"]: t for t in T}
    # his history per deployer key (creator, and the signer where the create was decoded)
    H = collections.defaultdict(list)  # key -> [(bt, H, pnl, stake)]
    for t in T:
        rec = (t["bt"], t["slot_off"] + t["hold"], t["pnl"], t["stake"])
        H[("c", t["creator"])].append(rec)
        c = cr.get(t["mint"])
        if c and c["signer"] != t["creator"]: H[("s", c["signer"])].append(rec)
    feats = {}
    for day in (23, 24):
        for r in json.load(open(SP + f"/pm/dataset_{day}.json")): r["day"] = day; feats[r["mint"]] = r
    return cr, T, his_by_mint, H, feats
def history(H, c, bt, days=7):
    out = [x for k in (("c", c["creator"]), ("s", c["signer"])) for x in H.get(k, []) if bt - days * 86400 <= x[0] < bt]
    return sorted(set(out))
def val(seq, who, hz):
    spent = got = held = 0; st = None
    for s, i, u, b, sol, tok, vs, vt, f in seq:
        if s > hz: break
        st = (vs, vt)
        if u != who: continue
        if b: spent += sol + f; held += tok
        else: got += sol - f; held -= tok
    if not spent: return None
    mark = (st[0] - st[0] * st[1] / (st[1] + held)) * (1 - FEE) if held > 0 else 0
    return (got + mark) / spent - 1
def coin(c, tape):
    seq = [(s, i, u, bool(b), sol, tok, vs, vt, f) for s, i, sg, evs in tape["rows"] for (u, b, sol, tok, vs, vt, f) in evs]
    cs, ci = c["slot"], c["idx"] or 0
    cset = {c["creator"], c["user"], c["signer"]}; prev = ci; end = (cs, ci); pb = None; bw = set()
    for s, i, u, b, sol, tok, vs, vt, f in seq:
        if (s, i) < (cs, ci): continue
        if (s, i) == (cs, ci) or (s == cs and u in cset and (i - prev) <= 2): prev = max(prev, i); end = max(end, (s, i)); pb = vs; continue
        if s == cs and b and u not in (A, US) and (i - prev) <= 2 and (i - ci) <= 5: prev = i; end = (s, i); bw.add(u); pb = vs
        elif (s, i) > end: break
    ins = cset | bw
    fo = next((x for x in seq if (x[0], x[1]) > end and x[3] and x[2] not in ins), None)
    r = dict(pb_sol=pb / 1e9 if pb else None, n_bw=len(bw), fo=None)
    if fo:
        r.update(fo=fo[2], fo_who="him" if fo[2] == A else "us" if fo[2] == US else "rival", d=fo[0] - cs, fo_i=fo[1],
                 vC={h: val(seq, fo[2], cs + h) for h in range(max(1, fo[0] - cs), 13)},
                 vE={k: val(seq, fo[2], fo[0] + k) for k in range(1, 13 - (fo[0] - cs))})
    # insiders' first sell slot (creator set + bundle run)
    r["ins_sell"] = next((s - cs for s, i, u, b, *_ in seq if u in ins and not b and (s, i) > end), None)
    r["his_buy_landed"] = any(u == A and b for s, i, u, b, *_ in seq)
    return r
def build(days_hours=((23, 0, 24), (24, 0, 12))):
    cr, T, his_by_mint, H, feats = load()
    out = []
    for p in glob.glob(SP + "/full/*.json"):
        if p.endswith(".tmp"): continue
        tape = json.load(open(p)); m = tape["mint"]; c = cr.get(m); f = feats.get(m)
        if not c or not f or not tape.get("reached"): continue
        if not any(f["day"] == d and lo <= f["hour"] < hi for d, lo, hi in days_hours): continue
        x = coin(c, tape)
        h = history(H, c, c["bt"])
        lastH = h[-1][1] if h else None
        modeH = collections.Counter(z[1] for z in h).most_common(1)[0][0] if len(h) >= 2 else None
        pnl = sum(z[2] for z in h); stake_med = sorted(z[3] for z in h)[len(h) // 2] if h else None
        t = his_by_mint.get(m)
        x.update(mint=m, day=f["day"], hour=f["hour"], bt=c["bt"], signer=c["signer"], creator=c["creator"], host=f["host"], dev=f["dev"],
                 sid=f["sid"], delay=f["delay"], rank=f["tweet_rank"], author=f.get("author"),
                 first_fresh=bool(f["sid"] and f["delay"] is not None and f["delay"] < 3600 and f["tweet_rank"] == 1),
                 he=f["he"], his_H=(t["slot_off"] + t["hold"]) if t else None, his_ret=(t["pnl"] / t["stake"]) if t else None, his_stake=t["stake"] if t else None,
                 h_n=len(h), h_lastH=lastH, h_modeH=modeH, h_pnl=pnl, h_stake=stake_med)
        out.append(x)
    return out
def stats(v, cost=0.0, w=None):
    v = [a - cost for a in v if a is not None]
    if len(v) < 5: return None
    m = sum(v) / len(v); random.seed(5)
    bs = sorted(sum(random.choice(v) for _ in v) / len(v) for _ in range(800))
    return dict(m=m, lo=bs[20], hi=bs[779], n=len(v), win=sum(a > 0 for a in v) / len(v))
fmt = lambda s: "n<5" if not s else f"{100*s['m']:+6.2f}% [{100*s['lo']:+6.2f},{100*s['hi']:+6.2f}] n {s['n']:4d}"
