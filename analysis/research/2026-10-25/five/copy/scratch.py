# A copy bot from scratch: no rule, no filter but his own trades. Every landed buy of his at create + 0/1 is copied:
# our buy lands D block positions after his (D drawn from OUR measured trigger-to-landing distances: the positions our
# create-slot buys landed after the create, 703 of them), bound at his price + PREM (else a failed buy, 0.0001 SOL);
# exit (a) one slot before the operator's table hold (anticipating his sell; after his sell when that came first),
# (b) behind his own first sell (the state right after it: optimistic), (c) the better of the two (an oracle).
# Stake 0.5 SOL or his own size (capped at 2 SOL). Value = the mean over our D distribution (deciles).
import json, glob, collections, statistics as st, sys, bisect
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/dec/copy/copyval.py").read().split("def val(")[0]
sys.argv = ["x", "0.0", "10"]
exec(src)
PREM = float(__import__("os").environ.get("PREM", "0.10"))
B = json.load(open(SP + "/dec/five/bids.json"))
# our trigger-to-landing distances (create slot landings, positions after the create)
crm = {}
for p in glob.glob(SP + "/dec/five/coin/*.json"):
    d = json.load(open(p))
    if d.get("create"): crm[d["mint"]] = (d["cs"], d["create"]["idx"])
Ds = sorted(r["idx"] - crm[r["m"]][1] for r in W[US] if r["kind"] == "buy" and r["m"] in crm and (r["ok"] or r.get("code") == 6042) and r["slot"] == crm[r["m"]][0])
dec = [Ds[int(len(Ds) * q)] for q in (0.05, 0.15, 0.25, 0.35, 0.45, 0.55, 0.65, 0.75, 0.85, 0.95)]
print("our trigger-to-landing positions, decile mids:", dec)
def pnl_s(entry, ex, stake):
    v0, t0 = entry; k = v0 * t0; s_in = stake * 1e9 / (1 + FEE); q = t0 - k / (v0 + s_in)
    vr, tr = ex
    if tr <= q: return None
    return vr * q / (tr - q) * (1 - FEE) / 1e9 - stake - TIP - SELL_TIP - 2 * BASE
def entry_at(r, D):
    b = his_buy[r["m"]]
    xs = [x for x in r["seq"] if (x[0], x[1]) <= (b["slot"], b["idx"] + D) and x[0] <= b["slot"]]
    return (xs[-1][2], xs[-1][3]) if xs else None
def one(r, D, exit_kind, stake):
    en = entry_at(r, D)
    if not en: return None
    if (en[0] / en[1]) / r["hisp"] - 1 > PREM: return -0.0001
    s = r["s"]
    ant = exit_at(r["seq"], r["cs"], r["H"] - 1, s)
    fol = (((s["fill"][2] if s["fill"][2] > 0 else K // s["fill"][3]), s["fill"][3]) if s else None)
    vals = [pnl_s(en, x, stake) for x in (ant, fol) if x]
    vals = [v for v in vals if v is not None]
    if not vals: return None
    if exit_kind == "anticipate": return pnl_s(en, ant, stake) if ant else None
    if exit_kind == "follow": return pnl_s(en, fol, stake) if fol else None
    return max(vals)
for r in rows:
    b = his_buy[r["m"]]; r["tip"] = (B.get(b["sig"]) or [0])[0]; r["sol"] = b["fill"][0] / 1e9
def run(lab, sel, exit_kind, stake_of):
    tot = 0.0; n = 0; byd = collections.defaultdict(float); per = []
    for r in sel:
        vs = [one(r, D, exit_kind, stake_of(r)) for D in dec]
        vs = [v for v in vs if v is not None]
        if not vs: continue
        v = sum(vs) / len(vs); tot += v; n += 1; byd[r["day"]] += v; per.append(v)
    print(f"   {lab:58} n {n:4}  total {tot:+7.2f} SOL  per copy {tot/max(n,1):+.4f}  by day " + " ".join(f"{k} {v:+.2f}" for k, v in sorted(byd.items())))
for sl, sel in (("all his landed buys", rows), ("his buy >= 0.7 SOL", [r for r in rows if r["sol"] >= 0.7]),
                ("his tip >= 8M", [r for r in rows if r["tip"] >= 8e6]), ("his tip >= 12M", [r for r in rows if r["tip"] >= 12e6])):
    print(f"\n== {sl}: {len(sel)} coins, his net {sum(r['his'] for r in sel):+.2f} SOL (5 days)")
    for ek in ("anticipate", "follow", "best of both (oracle)"):
        run(f"exit {ek}, 0.5 SOL", sel, ek, lambda r: 0.5)
    run("exit anticipate, his size (<= 2 SOL)", sel, "anticipate", lambda r: min(max(r["sol"], 0.1), 2.0))
# what the entry distance alone does (anticipated exit, his >= 0.7 SOL buys)
print("\n== his >= 0.7 SOL buys, exit anticipate, 0.5 SOL: value by a FIXED landing distance D")
sel = [r for r in rows if r["sol"] >= 0.7]
for D in (0, 1, 3, 10, 20, 40, 60, 90, 150, 240):
    vs = [one(r, D, "anticipate", 0.5) for r in sel]; vs = [v for v in vs if v is not None]
    print(f"   D = {D:3}: {sum(vs):+7.2f} SOL over {len(vs)} copies")
