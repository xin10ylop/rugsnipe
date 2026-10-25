# The post-execution copy (filled at the end of his slot, or one slot later) keyed on his tip, the conviction his own
# landed transaction shows (docs/17 s3), and his size; exit at H-1 / +9 / H; all 5 days and Oct 23+ by day.
import json, sys, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/dec/copy/copyval.py").read().split("def show(")[0]
sys.argv = ["x", "0.0", "10"]
exec(src)
B = json.load(open(SP + "/dec/five/bids.json"))
for r in rows:
    b = his_buy[r["m"]]; r["tip"] = (B.get(b["sig"]) or [None])[0]; r["sol"] = b["fill"][0] / 1e9
have = [r for r in rows if r["tip"] is not None]
print(f"his landed buys at create+0/1 with a tip read: {len(have)} of {len(rows)}")
def show(lab, sel, entry, h):
    xs = []; byd = collections.defaultdict(float)
    for r in sel:
        v = val(r, entry, h(r))
        if v is None: continue
        xs.append(v); byd[r["day"]] += v
    summ(lab, xs)
    print("        by day: " + "  ".join(f"{k} {v:+.2f}" for k, v in sorted(byd.items())))
for tl, tf in (("tip < 1.5M", lambda r: r["tip"] < 1.5e6), ("tip 1.5-4M", lambda r: 1.5e6 <= r["tip"] < 4e6),
               ("tip >= 4M", lambda r: r["tip"] >= 4e6), ("tip >= 8M", lambda r: r["tip"] >= 8e6), ("tip >= 12M", lambda r: r["tip"] >= 12e6),
               ("tip >= 4M and >= 0.7 SOL", lambda r: r["tip"] >= 4e6 and r["sol"] >= 0.7)):
    sel = [r for r in have if tf(r)]
    print(f"\n== {tl}: {len(sel)} coins, his net {sum(r['his'] for r in sel):+.2f} SOL; not fired by the newest bot (Oct 23+): "
          f"{sum(1 for r in sel if r['bt'] >= T0 and not r['fired'])}")
    show("end of his slot, sell H-1", sel, "e_slot", lambda r: r["H"] - 1)
    show("end of his slot, sell +9", sel, "e_slot", lambda r: 9)
    show("end of his slot, sell H", sel, "e_slot", lambda r: r["H"])
    show("one slot later, sell H-1", sel, "e_next", lambda r: r["H"] - 1)
