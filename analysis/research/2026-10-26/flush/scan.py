# Dumps in the first 13 slots of every launch with a complete tape (wide/coin, Oct 23-25): a sell (one trade) that takes
# the curve price down by >= DROP from its running high, after the coin rose >= RISE over its launch price. Who sold
# (the dev / its bundle run / another early buyer), how much of the high the price kept, how far above launch, and the
# price at the end of the tape relative to the price right after the dump (a first look at the bounce).
import json, glob, collections, statistics as st, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
K = 30_000_000_000 * 1_073_000_000_000_000
P0 = 30_000_000_000 / 1_073_000_000_000_000
DROP = float(sys.argv[1]) if len(sys.argv) > 1 else 0.25; RISE = float(sys.argv[2]) if len(sys.argv) > 2 else 0.3
ev = []; n = 0
for p in glob.glob(SP + "/wide/coin/*.json"):
    d = json.load(open(p))
    if not d.get("create") or not d.get("rows"): continue
    n += 1; c = d["create"]; cs = d["cs"]; ci = c["idx"]
    rows = sorted(d["rows"], key=lambda r: (r[0], r[1]))
    bund = {c["signer"], c["creator"]}; prev = ci
    for r in rows:
        if r[0] != cs or r[1] < ci: continue
        if r[1] == ci or (r[1] - prev <= 2 and r[1] - ci <= 5): bund.add(r[2]); prev = r[1]
        elif r[1] - prev > 2: break
    hi = 0.0; pr = []
    for r in rows:
        if r[7] <= 0: continue
        p_ = (r[6] if r[6] > 0 else K // r[7]) / r[7]; pr.append((r, p_))
    for i, (r, p_) in enumerate(pr):
        before = pr[i - 1][1] if i else p_
        if r[3] == 0 and hi >= P0 * (1 + RISE) and p_ <= hi * (1 - DROP) and before > hi * (1 - DROP):
            who = "dev" if r[2] in (c["signer"], c["creator"]) else "bundle" if r[2] in bund else "other"
            end = pr[-1][1]
            ev.append(dict(m=d["mint"], off=r[0] - cs, who=who, sol=r[4] / 1e9, hi=hi / P0, kept=p_ / hi, above=p_ / P0,
                           end_rel=end / p_, n_after=len(pr) - i - 1, tape_end=pr[-1][0][0] - cs))
            break
        hi = max(hi, p_)
print(f"coins {n}; with a dump (a sell taking >= {DROP:.0%} off the high, after a rise >= {RISE:.0%}): {len(ev)}")
by = collections.defaultdict(list)
for e in ev: by[e["who"]].append(e)
for w, v in by.items():
    print(f"  seller {w:6}: {len(v):5d}  sell size median {st.median(e['sol'] for e in v):.2f} SOL  at create+{st.median(e['off'] for e in v):.0f}  "
          f"high {st.median(e['hi'] for e in v):.2f}x launch  after the dump {st.median(e['above'] for e in v):.2f}x launch  "
          f"tape end vs the dump price: median {st.median(e['end_rel'] for e in v) - 1:+.1%} (trades after {st.median(e['n_after'] for e in v):.0f})")
json.dump(ev, open(SP + f"/flush/events_{DROP}_{RISE}.json", "w"))
