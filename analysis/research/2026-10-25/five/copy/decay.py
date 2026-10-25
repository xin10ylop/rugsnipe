# (i) Copying his landed buys: the value as we land further behind him (k other buys between him and us, end of his
# slot, end of the next slot), on the 5-day coins (create slot and next complete). (ii) Filling behind whoever took the
# first spot (him or a rival) on the complete 12-slot tapes (full/, fp/: Oct 23 - Oct 24 12:00).
import json, glob, collections, statistics as st, os
exec(open("/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/dec/behind/second.py").read().split("# ---- A.")[0])
print("== (i) copy his landed buys: entry k buys behind him in his slot, or later; sell at create + 9")
res = collections.defaultdict(list); gaps = []
for m, b in his_buy.items():
    d = coins.get(m)
    if not d: continue
    seq = seq_of(d); cs = d["cs"]
    if b["slot"] > cs + 1 or (b["slot"] == cs + 1 and False): continue
    ex = state_end(seq, cs + 9)
    if not ex: continue
    after = [x for x in seq if x[0] == b["slot"] and x[1] > b["idx"] and x[5] == 1]
    f = b["fill"]; e0 = (f[2] if f[2] > 0 else K // f[3], f[3])
    entries = {"k=0 (right behind him)": e0}
    for k in (1, 2, 3, 5):
        entries[f"k={k} buys between"] = (after[k - 1][2], after[k - 1][3]) if len(after) >= k else e0 if not after else (after[-1][2], after[-1][3])
    entries["end of his slot"] = state_end(seq, b["slot"])
    if b["slot"] + 1 <= cs + 1: entries["end of the next slot"] = state_end(seq, b["slot"] + 1)
    for lab, en in entries.items():
        v = pnl(en, ex)
        if v is not None: res[lab].append(v)
    gaps.append(len(after))
for lab in ("k=0 (right behind him)", "k=1 buys between", "k=2 buys between", "k=3 buys between", "k=5 buys between", "end of his slot", "end of the next slot"):
    summ(lab, res[lab])
print(f"   buys landing behind him in his own slot: median {st.median(gaps)}, p75 {sorted(gaps)[3*len(gaps)//4]}, p90 {sorted(gaps)[9*len(gaps)//10]}")
# our real gap behind him on the 80 coins where our transaction landed behind his
g2 = []
for r in W[US]:
    if r["kind"] != "buy" or r["ok"] or r["code"] != 6042 or r["m"] not in his_buy: continue
    b = his_buy[r["m"]]; d = coins.get(r["m"])
    if not d or r["slot"] != b["slot"]: continue
    seq = seq_of(d)
    g2.append(sum(1 for x in seq if x[0] == b["slot"] and b["idx"] < x[1] < r["idx"] and x[5] == 1))
if g2: print(f"   our real position behind him (same slot, {len(g2)} cases): buys between him and us median {st.median(g2)}, p75 {sorted(g2)[3*len(g2)//4]}")
# (ii) complete tapes: fill behind the first spot, whoever took it
print("\n== (ii) fill behind the first spot (complete 12-slot tapes, Oct 23 - Oct 24 12:00), sell at create + 9")
cr = {}
for f in glob.glob(SP + "/univ/creates_*.json"):
    for c in json.load(open(f)): cr[c["mint"]] = c
res2 = collections.defaultdict(list)
for src in ("full", "fp"):
    for p in glob.glob(f"{SP}/{src}/*.json"):
        try: d = json.load(open(p))
        except Exception: continue
        if not isinstance(d, dict) or "rows" not in d: continue
        m = d["mint"]; c = cr.get(m)
        if not c: continue
        cs = d["cs"]; ci = d["ci"]
        seq = sorted((r[0], r[1], (e[4] if e[4] > 0 else K // e[5]), e[5], e[0], e[1], e[2]) for r in d["rows"] for e in r[3] if e[5] > 0)
        cset = {c["signer"], c["creator"], c["user"]}
        prev = ci; last = None; bw = set()
        for x in seq:
            if x[0] != cs or x[1] < ci: continue
            if x[1] == ci: last = x; continue
            if x[4] in cset and x[1] - prev <= 2: prev = x[1]; last = x; continue
            if x[5] == 1 and x[1] - prev <= 2 and x[1] - ci <= 5 and x[4] not in (A, US): prev = x[1]; last = x; bw.add(x[4])
            elif x[1] - prev > 2 or x[1] - ci > 5: break
        if not last: continue
        firsts = [x for x in seq if (x[0], x[1]) > (last[0], last[1]) and x[5] == 1 and x[4] not in cset and x[4] not in bw and x[0] <= cs + 1]
        if not firsts: continue
        fb = firsts[0]; who = "him" if fb[4] == A else "us" if fb[4] == US else "a rival"
        ex = state_end(seq, cs + 9)
        if not ex: continue
        pb = (last[2], last[3])
        res2[(who, "at the post-bundle state (first spot)")].append(pnl(pb, ex))
        res2[(who, "right behind the first buyer")].append(pnl((fb[2], fb[3]), ex))
        if len(firsts) >= 3: res2[(who, "behind the first 3 buyers")].append(pnl((firsts[2][2], firsts[2][3]), ex))
for k in sorted(res2): summ(f"first: {k[0]:8} entry {k[1]}", [x for x in res2[k] if x is not None])
