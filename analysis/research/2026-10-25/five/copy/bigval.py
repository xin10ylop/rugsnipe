# The 40 vSOL cap with a 5% bound: the followed coins above 40 vSOL the newest bot would fire on without the cap
# (big402.txt, Oct 21-25). Entry where we really landed when we tried live, else one buy behind the first buyer
# behind the run (our real median position); bound 0 / 5 / 10% over the post-bundle price; sold at +9 (after his
# first sell when that came first: his post-sell state). Same pnl model as everywhere (0.5 SOL, 1.25% each way).
import json, glob, collections, os, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
exec(open(SP + "/dec/behind/second.py").read().split("# ---- A.")[0])
big = [l.split()[0] for l in open(SP + "/dec/five/big/big402.txt") if l.strip()]
first_sell = {}
for r in W[A]:
    if r["kind"] == "sell" and r["ok"] and r.get("fill"):
        if r["m"] not in first_sell or (r["slot"], r["idx"]) < (first_sell[r["m"]]["slot"], first_sell[r["m"]]["idx"]): first_sell[r["m"]] = r
ours = collections.defaultdict(list)
for r in W[US]:
    if r["kind"] == "buy" and (r["ok"] or r.get("code") == 6042): ours[r["m"]].append(r)
res = {b: collections.defaultdict(list) for b in (0.0, 0.05, 0.10)}; miss = collections.Counter(); daysum = {b: collections.defaultdict(float) for b in (0.0, 0.05, 0.10)}
for m in big:
    d = coins.get(m)
    if not d: miss["no tape"] += 1; continue
    seq = seq_of(d); cs = d["cs"]; pb = post_bundle(d, seq)
    if not pb: miss["no run"] += 1; continue
    s = first_sell.get(m)
    if s and s["slot"] <= cs + 9:
        f = s["fill"]; ex = ((f[2] if f[2] > 0 else K // f[3]), f[3])
    else:
        if max(x[0] for x in seq) < cs + 2 and not s: miss["tape ends at +1"] += 1; continue
        ex = state_end(seq, cs + 9)
    c = d["create"]; cset = {c["signer"], c["creator"]}
    o = sorted(ours[m], key=lambda x: (x["slot"], x["idx"]))
    if o:
        pre = [x for x in seq if (x[0], x[1]) < (o[0]["slot"], o[0]["idx"])]
        e = (pre[-1][2], pre[-1][3]) if pre else pb
        how = "live position"
    else:
        behind = [x for x in seq if x[0] <= cs + 1 and x[5] == 1 and x[4] not in cset and x[4] != US and (x[2] / x[3]) > (pb[0] / pb[1]) * 1.000001]
        e = pb if not behind else (behind[1][2], behind[1][3]) if len(behind) >= 2 else (behind[0][2], behind[0][3])
        how = "model"
    hl = m in his_buy
    bt = his_buy[m]["bt"] if hl else (o[0]["bt"] if o else None)
    for b in (0.0, 0.05, 0.10):
        if (e[0] / e[1]) / (pb[0] / pb[1]) - 1 > b + 1e-9: res[b]["refused"].append(0.0); continue
        v = pnl(e, ex)
        if v is None: continue
        res[b]["he landed" if hl else "he did not land"].append(v); res[b]["all"].append(v)
        if bt: daysum[b][day(bt)] += v
print("coverage:", dict(miss), "of", len(big))
for b in (0.0, 0.05, 0.10):
    print(f"\n== bound {b:.0%}")
    for k in ("he landed", "he did not land", "all"):
        summ(k, res[b][k])
    print(f"   refused by the bound: {len(res[b]['refused'])}; by day (dated coins): " + "  ".join(f"{k} {v:+.2f}" for k, v in sorted(daysum[b].items())))
