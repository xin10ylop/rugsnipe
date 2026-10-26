# The operator rule with the operators' arrival timed: his key copies (tip >= 2M or size >= 2 SOL, landed at create + 0/1,
# Oct 21-25, 0.5 SOL, our landing distances, 5% bound, live exit) split by what the bot could know at the buy:
#   A  his launch buys on the operator in the previous 24 h >= 2 (the live rule), now counted with his history before
#      Oct 21 too (the month's trips), so day one is not an empty start
#   B  fewer, but he traded the operator in the 7 days before the last table refresh (what the box's table lists:
#      --keep-days 7 --min-n 1, refreshed 03/07/11/15/19/23:50)
#   C  fewer, not in that table, but he traded it earlier in the month (back after more than 7 days)
#   D  never before in the month (Sep 24 on): a brand-new operator
import json, collections, io, contextlib, bisect, datetime
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
with contextlib.redirect_stdout(io.StringIO()):
    exec(open(SP + "/dec/copy/ablate.py").read().split("live = [r for r in rows")[0])
KEY2 = lambda r: r["tip"] >= 2e6 or r["sol"] >= 2.0
D21 = int(datetime.datetime(2026, 10, 21, tzinfo=datetime.timezone.utc).timestamp())
TT = json.load(open(SP + "/month/AHwJ_T.json"))
ev = collections.defaultdict(list)                       # operator key -> his launch-buy times
for t in TT:
    if t["bt"] < D21: ev[t["creator"]].append(t["bt"])
for r in rows:
    c = coins[r["m"]]["create"]; bt = his_buy[r["m"]]["bt"]
    for k in {c["signer"], c["creator"]}: ev[k].append(bt)
for v in ev.values(): v.sort()
def keys(r):
    c = coins[r["m"]]["create"]; return {c["signer"], c["creator"]}
def times_before(r, lo, hi):
    out = set()
    for k in keys(r):
        v = ev.get(k, []); out |= set(v[bisect.bisect_left(v, lo):bisect.bisect_left(v, hi)])
    return out
def last_refresh(t):
    d = datetime.datetime.fromtimestamp(t, datetime.timezone.utc)
    base = d.replace(minute=50, second=0, microsecond=0)
    while base.timestamp() > t or base.hour not in (3, 7, 11, 15, 19, 23): base -= datetime.timedelta(hours=1)
    return int(base.timestamp())
def cls(r):
    bt = his_buy[r["m"]]["bt"]
    if len(times_before(r, bt - 86400, bt)) >= 2: return "A"
    R = last_refresh(bt)
    if times_before(r, R - 7 * 86400, R): return "B"
    if times_before(r, 0, bt): return "C"
    return "D"
K2 = [r for r in rows if KEY2(r)]
for r in K2:
    r["cls"] = cls(r); r["old"] = cnt(r, 24) >= 2
    r["v"] = val(r); r["v9"] = val(r, hold_fn=lambda r: 9); r["v5"] = val(r, hold_fn=lambda r: 5)
print("by day: " + " ".join(DAYS))
def line(lab, sel, k="v"):
    byd = collections.defaultdict(float); hb = collections.defaultdict(float); n = 0
    for r in sel:
        if r[k] is None: continue
        byd[r["day"]] += r[k]; hb[r["day"]] += r["his"]; n += 1
    print(f"   {lab:54} n {n:4d} his {sum(hb.values()):+6.2f}  copy {sum(byd.values()):+6.2f}  /copy {sum(byd.values())/max(n,1):+.4f}  " + " ".join(f"{byd[d]:+5.2f}" for d in DAYS))
print("== old count (history only from Oct 21) vs the count with the month's history")
line("old: >= 2 (the ablation's live set)", [r for r in K2 if r["old"]])
line("new: >= 2 (A)", [r for r in K2 if r["cls"] == "A"])
line("moved into A by the history (Oct 21 mostly)", [r for r in K2 if r["cls"] == "A" and not r["old"]])
for c, lab in (("A", "A  >= 2 in 24 h (live rule)"), ("B", "B  < 2, traded within 7 d of the last refresh"), ("C", "C  < 2, back after > 7 days"), ("D", "D  brand new this month")):
    s = [r for r in K2 if r["cls"] == c]
    print(f"== {lab}: {len(s)} key buys, listed in the backtest's 24 h tables {sum(r['listed'] for r in s)}")
    line("copy, live exit (table hold - 1, else modal - 1, else +9)", s)
    line("copy, flat create + 5", s, "v5")
    line("copy, flat create + 9", s, "v9")
print("== the rule, with the arrival timed")
line("A only (live)", [r for r in K2 if r["cls"] == "A"])
line("A + B (drop the count, keep the 7-day table)", [r for r in K2 if r["cls"] in "AB"])
line("A + B + C", [r for r in K2 if r["cls"] in "ABC"])
line("all (no operator rule)", K2)
# his big day
d23 = [r for r in K2 if r["day"] == "10-23"]
print("== Oct 23 (his big day): his key buys by class")
for c in "ABCD":
    s = [r for r in d23 if r["cls"] == c]
    print(f"   {c}: {len(s):3d} buys, his net {sum(r['his'] for r in s):+6.2f}, copy {sum(r['v'] or 0 for r in s):+6.2f}; top his coins: " +
          ", ".join(f"{r['m'][:8]} {r['his']:+.2f}" for r in sorted(s, key=lambda r: -r['his'])[:4]))
json.dump([dict(m=r["m"], day=r["day"], cls=r["cls"], his=r["his"], v=r["v"], signer=r["signer"]) for r in K2], open(SP + "/oprule/arrival.json", "w"))
