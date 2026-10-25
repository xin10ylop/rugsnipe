# The tip-keyed copy (listed signers, his tip >= 4M, bound 5%, sold at H-1) at different stakes, next to his own
# result on the same coins at his own stakes. Our landing distances as everywhere; fees, tips and our own price
# impact are in the model (pnl_s).
import json, sys, collections, statistics as st
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/dec/copy/weakdays.py").read().split("L = [r for r in rows if r[\"listed\"]]")[0]
exec(src)
PREM = 0.05
DAYS = ["10-21", "10-22", "10-23", "10-24", "10-25"]
sel = [r for r in rows if r["listed"] and r["tip"] >= 4e6]
his_stake = [r["sol"] for r in sel]
print(f"{len(sel)} coins; his stake there: median {st.median(his_stake):.2f} SOL, mean {st.mean(his_stake):.2f}; "
      f"his net {sum(r['his'] for r in sel):+.2f} SOL = {100*sum(r['his'] for r in sel)/sum(his_stake):+.1f}% of what he staked")
byd = collections.defaultdict(float)
for r in sel: byd[r["day"]] += r["his"]
print("   him, his stakes:            total %+6.2f  " % sum(byd.values()) + " ".join(f"{byd[d]:+.2f}" for d in DAYS))
def copy_at(stake_of):
    byd = collections.defaultdict(float); staked = 0.0
    for r in sel:
        stake = stake_of(r); vs = []
        for D in dec:
            en = entry_at(r, D)
            if not en: continue
            if (en[0] / en[1]) / r["hisp"] - 1 > PREM: vs.append(-0.0001); continue
            ex = exit_at(r["seq"], r["cs"], max(r["H"] - 1, 1), r["s"])
            v = pnl_s(en, ex, stake) if ex else None
            if v is not None: vs.append(v)
        if vs: byd[r["day"]] += sum(vs) / len(vs); staked += stake
    return byd, staked
for lab, f in (("us at 0.25 SOL", lambda r: 0.25), ("us at 0.5 SOL (the table)", lambda r: 0.5), ("us at 1.0 SOL", lambda r: 1.0),
               ("us at his own size", lambda r: r["sol"]), ("us at his size, capped 1 SOL", lambda r: min(r["sol"], 1.0))):
    byd, staked = copy_at(f)
    print(f"   {lab:28} total {sum(byd.values()):+6.2f}  " + " ".join(f"{byd[d]:+.2f}" for d in DAYS) + f"   ({100*sum(byd.values())/staked:+.1f}% of staked)")
