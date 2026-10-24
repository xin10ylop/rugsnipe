# His hold over the month: what it follows (same creator's previous hold, his previous trade's hold, the creator's
# 7-day modal hold), and how it varies with stake, tip, entry slot, post-bundle SOL and hour.
import json, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
T = sorted([t for t in json.load(open(SP + "/month/AHwJ_T.json")) if t["stake"] >= 0.1 and t["hold"] is not None], key=lambda t: t["bt"])
print("trades", len(T), "| hold distribution:", dict(sorted(collections.Counter(t["hold"] for t in T).items())))
byday = collections.defaultdict(collections.Counter)
for t in T: byday[t["day"]][t["hold"]] += 1
for d, c in sorted(byday.items()):
    n = sum(c.values()); top = c.most_common(3)
    print(f"  {d}: n {n:4d}  top holds {top}")
last_c = {}; hist_c = collections.defaultdict(list); prev = None
m = collections.Counter()
for t in T:
    c = t["creator"]
    if c in last_c: m["same_creator_prev_n"] += 1; m["same_creator_prev_eq"] += last_c[c] == t["hold"]
    if prev is not None: m["prev_any_n"] += 1; m["prev_any_eq"] += prev == t["hold"]
    h7 = [h for (b, h) in hist_c[c] if t["bt"] - 7 * 86400 <= b < t["bt"]]
    if len(h7) >= 3:
        mode = collections.Counter(h7).most_common(1)[0][0]; m["mode7_n"] += 1; m["mode7_eq"] += mode == t["hold"]
    last_c[c] = t["hold"]; hist_c[c].append((t["bt"], t["hold"])); prev = t["hold"]
print(f"hold == same creator's previous hold: {100*m['same_creator_prev_eq']/m['same_creator_prev_n']:.0f}% (n {m['same_creator_prev_n']})")
print(f"hold == his previous trade's hold (any creator): {100*m['prev_any_eq']/m['prev_any_n']:.0f}%")
print(f"hold == creator's modal hold over the prior 7 days (3+ trades): {100*m['mode7_eq']/m['mode7_n']:.0f}% (n {m['mode7_n']})")
def by(key, f):
    g = collections.defaultdict(list)
    for t in T: g[f(t)].append(t["hold"])
    print(f"hold by {key}:", "  ".join(f"{k}: mean {sum(v)/len(v):.1f} (n {len(v)})" for k, v in sorted(g.items(), key=lambda kv: str(kv[0]))))
by("stake", lambda t: "<0.7" if t["stake"] < 0.7 else "0.7-1.5" if t["stake"] < 1.5 else ">=1.5")
by("tip", lambda t: "<=2M" if (t["tip"] or 0) <= 2_000_000 else ">2M")
by("entry slot after create", lambda t: t["slot_off"])
by("post-bundle vSOL", lambda t: "<36" if (t["v_before"] or 0) < 36 else "36-40" if t["v_before"] < 40 else ">=40")
by("hour UTC", lambda t: f"{((t['bt'] % 86400)//3600)//6*6:02d}-{((t['bt'] % 86400)//3600)//6*6+6:02d}")
