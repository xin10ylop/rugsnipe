# The copy's exit, optimized on his own hold logic (docs/16-17: he sells at a create-relative slot per operator, the
# operator table's hold H, and eats the dumps inside it). Copies of his landed buys at create + 0/1, entered at OUR
# measured landing distances (deciles of our trigger-to-landing positions), 0.5 SOL, bound +10%. Exits:
#   H-k on listed signers (k = 0..3), unlisted signers at a flat U, or at his last first-sell offset on that signer
#   seen before this coin (learned live from his sells; ex ante) - 1;
#   + a take-profit: sell at the end of the first slot whose state values the position at >= +TP (before the hold).
# A hold at or past his first sell sells after it (the state right after his sell: optimistic).
import json, glob, collections, sys, bisect
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/dec/copy/scratch.py").read().split("for r in rows:\n    b = his_buy")[0]
sys.argv = ["x", "0.0", "10"]
exec(src)
B = json.load(open(SP + "/dec/five/bids.json"))
for r in rows:
    b = his_buy[r["m"]]; r["tip"] = (B.get(b["sig"]) or [0])[0]; r["sol"] = b["fill"][0] / 1e9
    r["signer"] = coins[r["m"]]["create"]["signer"]
# his last first-sell offset per signer before each coin (ex ante, from his own sells)
ev = sorted((his_buy[r["m"]]["bt"], r["signer"], r["soff"]) for r in rows if r["soff"] is not None)
def last_hold(signer, bt):
    best = None
    for t, s, o in ev:
        if t >= bt: break
        if s == signer: best = o
    return best
for r in rows:
    r["last"] = last_hold(r["signer"], his_buy[r["m"]]["bt"])
def value_at(r, en, h):
    ex = exit_at(r["seq"], r["cs"], max(h, 1), r["s"])
    return pnl_s(en, ex, 0.5) if ex else None
def copy_val(r, D, hold, tp):
    en = entry_at(r, D)
    if not en: return None
    if (en[0] / en[1]) / r["hisp"] - 1 > PREM: return -0.0001
    if tp:
        for k in range(2, hold):
            if r["s"] is not None and r["cs"] + k >= r["s"]["slot"]: break
            v = value_at(r, en, k)
            if v is not None and v >= tp * 0.5: return v
    return value_at(r, en, hold)
def run(lab, sel, hold_of, tp=None):
    tot = 0.0; n = 0; byd = collections.defaultdict(float)
    for r in sel:
        h = hold_of(r)
        vs = [copy_val(r, D, h, tp) for D in dec]; vs = [v for v in vs if v is not None]
        if not vs: continue
        v = sum(vs) / len(vs); tot += v; n += 1; byd[r["day"]] += v
    print(f"   {lab:54} n {n:4} total {tot:+6.2f}  by day " + " ".join(f"{k} {v:+.2f}" for k, v in sorted(byd.items())))
    return tot
for sl, sel in (("his buy >= 0.7 SOL", [r for r in rows if r["sol"] >= 0.7]), ("his tip >= 8M", [r for r in rows if r["tip"] >= 8e6]),
                ("all his landed buys", rows)):
    lst = [r for r in sel if r["listed"]]; un = [r for r in sel if not r["listed"]]
    print(f"\n== {sl}: {len(sel)} ({len(lst)} on listed signers, {len(un)} unlisted; of these {sum(1 for r in un if r['last'] is not None)} with an earlier sell of his on the signer)")
    print(" listed signers:")
    for k in (0, 1, 2, 3):
        run(f"H - {k}", lst, lambda r, k=k: r["H"] - k)
    print(" unlisted signers:")
    for u in (5, 6, 7, 9, 10):
        run(f"flat +{u}", un, lambda r, u=u: u)
    run("his last sell offset on the signer - 1, else +9", un, lambda r: (r["last"] - 1) if r["last"] else 9)
    run("his last sell offset on the signer - 1, else +6", un, lambda r: (r["last"] - 1) if r["last"] else 6)
    print(" all, best hold rule + a take-profit:")
    best = lambda r: (r["H"] - 1) if r["listed"] else ((r["last"] - 1) if r["last"] else 9)
    run("hold rule alone", sel, best)
    for tp in (0.15, 0.25, 0.4, 0.6):
        run(f"hold rule + take-profit at +{tp:.0%}", sel, best, tp)
