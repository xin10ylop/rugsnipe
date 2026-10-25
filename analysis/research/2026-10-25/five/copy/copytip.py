# Copying his landed buys by what his own transaction shows (its tip) and by the post-bundle size; entries: one buy
# between him and us (our real median position behind him), the end of his slot (a copy that waits for his tx).
import json, collections, statistics as st
exec(open("/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/dec/behind/second.py").read().split("# ---- A.")[0])
B = json.load(open(F + "/bids.json"))
res = collections.defaultdict(list)
def tb(t): return "a <=1M" if t <= 1.01 else "b 1-4M" if t <= 4 else "c 4-12M" if t <= 12 else "d >12M"
for m, b in his_buy.items():
    d = coins.get(m)
    if not d: continue
    seq = seq_of(d); cs = d["cs"]
    if b["slot"] > cs + 1: continue
    ex = state_end(seq, cs + 9); pb = post_bundle(d, seq)
    if not ex or not pb: continue
    tip = (B.get(b["sig"]) or [0])[0] / 1e6
    big = "pb > 40" if pb[0] / 1e9 > 40 else "pb <= 40"
    after = [x for x in seq if x[0] == b["slot"] and x[1] > b["idx"] and x[5] == 1]
    f = b["fill"]; e0 = (f[2] if f[2] > 0 else K // f[3], f[3])
    e1 = (after[0][2], after[0][3]) if after else e0
    es = state_end(seq, b["slot"])
    for lab, en in (("1 buy between", e1), ("end of his slot", es)):
        v = pnl(en, ex)
        if v is None: continue
        res[(lab, big, tb(tip))].append(v); res[(lab, big, "all")].append(v); res[(lab, "all", tb(tip))].append(v)
for lab in ("1 buy between", "end of his slot"):
    print(f"\n== copy his landed buys, entry {lab}, sell at create + 9")
    for big in ("pb <= 40", "pb > 40", "all"):
        for t in ("a <=1M", "b 1-4M", "c 4-12M", "d >12M", "all"):
            xs = res.get((lab, big, t))
            if xs: summ(f"{big:9} his tip {t}", xs)
