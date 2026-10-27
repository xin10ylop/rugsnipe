# The ghost copy of him, 5 days, the full picture: today's keyed copies (tip >= 2M or size >= 2 SOL) and his other
# launch buys, which a ghost would also fill (it cannot see his tip; its on-chain size check can see his size).
# Ghost landing from the 83 create-sent buys (presend.json): after his buy, j more trades; each ghost after him with
# P_AFTER (else today's reactive copy). The bound fixed before his buy: post-bundle + CAP. Picked on P1 (Oct 21-23),
# scored on P2 (Oct 24-25); 0.0155 off every added fill; [each period without its best 2 coins]. His other buys come
# from a random 300 of the 1,109 (refused/items_rest300.json), scaled up.
#   python3 -I ghost4.py [P_AFTER]
import sys, json, collections, os
_a = sys.argv[1:]
exec(open("/home/user/rugsnipe/analysis/research/2026-10-27/refused/eng.py").read())
P_AFTER = float(_a[0]) if _a else 0.8
CF = 0.000015; LAUNCH_DAY = 1861; N_REST = 1109
J = json.load(open(SP + "/refused/presend.json"))["js"]; JP = {j: c / len(J) for j, c in collections.Counter(J).items()}
CAPS = (None, 0.05, 0.10, 0.15, 0.20, 0.30)
REST = {m for m, _ in json.load(open(SP + "/refused/items_rest300.json"))}
def fill_val(r, seq, en, ok):
    if not ok: return REF
    ex = exit_state(r, seq, kind="anticipate")
    return pnl_s(en, ex, 0.5) if ex else None
out = []; sk = collections.Counter()
for r in rows:
    keyed = r["tip"] >= 2e6 or r["sol"] >= 2.0
    if not keyed and r["m"] not in REST: continue
    Lg = long_seq(r["m"])
    if not Lg: sk["no 60-slot tape"] += 1; continue
    seq = Lg[0]; d = coins[r["m"]]
    pb = post_bundle(d, seq)
    if not pb: sk["no post-bundle state"] += 1; continue
    pbp = pb[0] / pb[1]
    b = his_buy[r["m"]]; hk = (b["slot"], b["idx"])
    after = [x for x in seq if (x[0], x[1]) > hk]
    s0 = st_before(seq, (hk[0], hk[1] + 1))
    vb = []
    for D in dec:
        en = st_before(seq, (b["slot"], b["idx"] + D + 1))
        if en: vb.append(fill_val(r, seq, en, (en[0] / en[1]) / r["hisp"] - 1 <= 0.05) if keyed else 0.0)
    vb = [v for v in vb if v is not None]
    if not vb: sk["no landing"] += 1; continue
    base = sum(vb) / len(vb); bfill = sum(1 for v in vb if v != REF and keyed) / len(vb)
    res = dict(m=r["m"], day=r["day"], keyed=keyed, sol=r["sol"], react=base)
    for cap in CAPS:
        e = (1 - P_AFTER) * base; fill = (1 - P_AFTER) * bfill
        for j, p in JP.items():
            s = s0 if j == 0 else ((after[j - 1][2], after[j - 1][3]) if j <= len(after) else None)
            ok = s is not None and (((s[0] / s[1]) / r["hisp"] - 1 <= 0.05) if cap is None else ((s[0] / s[1]) / pbp - 1 <= cap))
            v = fill_val(r, seq, s, ok) if s else None
            if v is None or v == REF: e += P_AFTER * p * base; fill += P_AFTER * p * bfill
            else: e += P_AFTER * p * v; fill += P_AFTER * p
        res[str(cap)] = e; res[f"{cap}:add"] = max(fill - bfill, 0.0)
    out.append(res)
json.dump(out, open(SP + "/refused/ghost4.json", "w"))
PER = {"P1": ("10-21", "10-22", "10-23"), "P2": ("10-24", "10-25")}
K = [x for x in out if x["keyed"]]; O = [x for x in out if not x["keyed"]]
print(f"P_AFTER {P_AFTER}; keyed {len(K)} of 575 with tapes, his other buys {len(O)} of the 300 sampled (x {N_REST / max(len(O), 1):.2f}); skipped {dict(sk)}")
print("   today's copy (keyed):", "  ".join(f"{p} {sum(x['react'] for x in K if x['day'] in ds):+.2f}" for p, ds in PER.items()))
def g(x, cap): return x[str(cap)] - BIAS * x[f"{cap}:add"] - x["react"]
print("   ghost gain over today, SOL (bias off; spray 0.028/day charged; [without each period's best 2])")
print(f"   {'bound':26} {'keyed P1':>15} {'keyed P2':>15}   {'his other buys, all sizes':>27}   {'other >= 0.7 SOL':>18}")
for cap in CAPS:
    cells = []
    for p, ds in PER.items():
        gs = sorted((g(x, cap) for x in K if x["day"] in ds), reverse=True); c = CF * LAUNCH_DAY * len(ds)
        cells.append(f"{sum(gs) - c:+.2f} [{sum(gs[2:]) - c:+.2f}]")
    sc = N_REST / max(len(O), 1)
    o_all = [g(x, cap) for x in O]; o_big = [g(x, cap) for x in O if x["sol"] >= 0.7]
    o1 = sum(g(x, cap) for x in O if x["day"] in PER["P1"]) * sc; o2 = sum(g(x, cap) for x in O if x["day"] in PER["P2"]) * sc
    lab = "5% over his price (oracle)" if cap is None else f"post-bundle + {cap:.0%}"
    print(f"   {lab:26} {cells[0]:>15} {cells[1]:>15}   {sum(o_all) * sc:+6.2f} (P1 {o1:+.2f} P2 {o2:+.2f})   {sum(o_big) * sc:+6.2f}")
