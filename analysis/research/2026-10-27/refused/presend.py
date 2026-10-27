# Where does a buy sent at the create land, against his buy? The old rule bot (Oct 21-25) sent its buys when it saw the
# create, before his buy existed: on the coins he bought too, our landed or 6042-failed buy against his. P(ours after his),
# and the trades between his and ours when it is after (the ghost copy's j), from the 5-day launch tapes.
import json, collections, statistics as st
exec(open("/home/user/rugsnipe/analysis/research/2026-10-27/refused/eng.py").read())
ours = collections.defaultdict(list)
for r in W[US]:
    if r["kind"] == "buy" and (r["ok"] or r.get("code") == 6042): ours[r["m"]].append(r)
n = after = 0; js = []; same = collections.Counter(); by_slot = collections.Counter()
for m, b in his_buy.items():
    d = coins.get(m)
    if not d or m not in ours: continue
    cs = d["cs"]
    if b["slot"] > cs + 1: continue
    o = min(ours[m], key=lambda r: (r["slot"], r["idx"]))
    if o["slot"] > cs + 2: continue
    seq = seq_of(d); n += 1
    hk, ok = (b["slot"], b["idx"]), (o["slot"], o["idx"])
    by_slot[(b["slot"] - cs, o["slot"] - cs)] += 1
    if ok > hk:
        after += 1
        js.append(sum(1 for x in seq if hk < (x[0], x[1]) < ok))
print(f"coins with his buy and our create-sent buy: {n}; ours after his: {after} ({after / max(n, 1):.0%})")
print("   (his slot, our slot) after the create:", by_slot.most_common(8))
if js:
    q = sorted(js)
    print(f"   trades between his buy and ours when ours is after: median {st.median(q)}, p25 {q[len(q) // 4]}, p75 {q[3 * len(q) // 4]}, "
          f"p90 {q[9 * len(q) // 10]}; 0: {sum(1 for j in q if j == 0)}, 1-2: {sum(1 for j in q if 1 <= j <= 2)}, 3+: {sum(1 for j in q if j >= 3)}")
json.dump(dict(n=n, after=after, js=js), open(SP + "/refused/presend.json", "w"))
