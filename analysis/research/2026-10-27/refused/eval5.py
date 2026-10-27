# The 5-day copy of him, refused landings only (premium > 5% at our position), on the 60-slot tapes. Per copy x landing:
# the premium, what bought between (classes from earlier coins only), and what each mechanism would make:
# a fill there (exits: his sell - 1 as the bot, create + 3/5/7/12/15/20, right after his sell), and a re-entry when the
# price comes back within x of his price before he sells.   -> SP/refused/rec5.json
import sys, json, collections
_LANDS = sys.argv[1] if len(sys.argv) > 1 else "dec"
exec(open("/home/user/rugsnipe/analysis/research/2026-10-27/refused/eng.py").read())
LANDS = _LANDS
if LANDS == "live":
    LV = json.load(open(SP + "/refused/land_him.json"))       # [(his_i, dslot, our_i)]
    def landings(his_i, k=10): return sorted(LV, key=lambda x: abs(x[0] - his_i))[:k]
sel = [r for r in rows if r["tip"] >= 2e6 or r["sol"] >= 2.0]
EXITS = ["anticipate", "c3", "c5", "c7", "c12", "c15", "c20", "follow"]
recs = []; nolong = 0
for r in sel:
    L = long_seq(r["m"])
    if not L: nolong += 1; continue
    seq, end = L
    b = his_buy[r["m"]]; cs = r["cs"]; hp = r["hisp"]; hk = (b["slot"], b["idx"])
    s = r["s"]; s_slot = s["slot"] if s else 10**12
    pos_list = [(b["slot"], b["idx"] + D + 1) for D in dec] if LANDS == "dec" else \
               [((b["slot"], b["idx"] + (oi - hi)) if ds == 0 else (b["slot"] + ds, oi)) for hi, ds, oi in landings(b["idx"])]
    for pos in pos_list:
        en = st_before(seq, pos)
        if not en: continue
        prem = (en[0] / en[1]) / hp - 1
        if prem <= 0.05: continue
        btw = [x for x in between(seq, hk, pos) if x[5] == 1 and x[4] != A]
        cl = collections.defaultdict(float)
        for x in btw: cl[cls(x[4], cs)] += x[6] / 1e9
        sells_btw = sum(1 for x in between(seq, hk, pos) if x[5] == 0)
        v = {}
        for ek in EXITS:
            ex = exit_state(r, seq, kind=ek)
            v[ek] = pnl_s(en, ex, 0.5) if ex else None
            v[ek + "@0.2"] = pnl_s(en, ex, 0.2) if ex else None
        # re-entry: the first slot after ours (before his sale and before create + H - 2) whose closing price is within
        # x of his; we land in the next slot at our median index, bound 5% over his price
        for xr in (0.0, 0.03, 0.05):
            v[f"re{xr}"] = None; v[f"re{xr}:slot"] = None
            last = min(cs + r["H"] - 3, s_slot - 2)
            for sl in range(pos[0], last + 1):
                e2 = st_end(seq, sl)
                if not e2 or (e2[0] / e2[1]) / hp - 1 > xr: continue
                en2 = st_before(seq, (sl + 1, 100))
                if (en2[0] / en2[1]) / hp - 1 > 0.05: continue
                ex = exit_state(r, seq, kind="anticipate")
                v[f"re{xr}"] = pnl_s(en2, ex, 0.5) if ex else None; v[f"re{xr}:slot"] = sl + 1 - cs
                break
        recs.append(dict(m=r["m"], day=r["day"], pos=pos, prem=prem, at=b["slot"] - cs, dslot=pos[0] - b["slot"],
                         n_btw=len(btw), sol_btw=sum(x[6] for x in btw) / 1e9, cl=dict(cl), sells_btw=sells_btw,
                         H=r["H"], soff=(s_slot - cs) if s else None, tip=r["tip"] / 1e6, sol=r["sol"], his=r["his"],
                         v=v, end=end - cs))
json.dump(dict(lands=LANDS, n=len(pos_list), recs=recs), open(SP + f"/refused/rec5_{LANDS}.json", "w"))
print(len(sel), "copies;", nolong, "without a long tape;", len(recs), "refused landing records,", len({x["m"] for x in recs}), "coins")
