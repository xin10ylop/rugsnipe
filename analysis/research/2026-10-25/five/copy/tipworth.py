# What landing earlier is worth to the live copy, and so the most a bigger tip could pay for: the copies of the live key
# (his tip >= 4M or his size >= 2 SOL, his buys on the signer >= 2 in 24 h, bound 5%, 0.5 SOL, exit H-1 / his modal
# first sell - 1 / +9), valued with our buy landing exactly D block positions after his (capped at the end of his
# slot), against our measured landing distances (the baseline). A copy the bound refuses fails; on the pay-on-success
# bundle route it costs nothing. Value at the model's 3M buy tip; the break-even extra tip is the gain per filled copy.
import collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/dec/copy/recent.py").read().split("T = [r for r in rows if r[\"tip\"] >= 4e6]")[0]
exec(src)
def hot(r): return sum(1 for t in r["prev_t"] if t >= his_buy[r["m"]]["bt"] - 24 * 3600) >= 2
S = [r for r in rows if hot(r) and (r["tip"] >= 4e6 or r["sol"] >= 2.0)]
def at(r, D):
    en = entry_at(r, D)
    if not en: return None, False
    if (en[0] / en[1]) / r["hisp"] - 1 > PREM: return -0.0001, False
    ex = exit_at(r["seq"], r["cs"], max(hold(r), 1), r["s"])
    v = pnl_s(en, ex, 0.5) if ex else None
    return v, v is not None
base = [];  nb = 0
for r in S:
    vs = [at(r, D) for D in dec]
    vs = [v for v in vs if v[0] is not None]
    if vs:
        base.append(sum(v for v, _ in vs) / len(vs)); nb += sum(f for _, f in vs) / len(vs)
B = sum(base)
print(f"{len(S)} copies of the live key; measured landing (D deciles {dec}): {B:+.2f} SOL, {nb:.0f} filled ({B / max(len(base), 1):+.4f} a copy)")
print(f"{'D':>4} {'total_sol':>9} {'filled':>6} {'per_copy':>9} {'gain_vs_measured':>16} {'break-even extra tip':>21}")
for D in (0, 5, 10, 20, 30, 45, 60, 90, 125, 200):
    tot = 0.0; nf = 0
    for r in S:
        v, f = at(r, D)
        if v is not None: tot += v; nf += f
    g = tot - B
    print(f"{D:4d} {tot:+9.2f} {nf:6d} {tot / max(len(base), 1):+9.4f} {g:+16.2f} {g / max(nf, 1) * 1e3:+17.1f}M")
