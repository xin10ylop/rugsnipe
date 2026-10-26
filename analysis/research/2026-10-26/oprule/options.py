# The operator rule's options with the arrival timed and the box's own table lag (an operator is listed only after the
# first 4-hourly refresh that follows his first trade on it; unlisted copies exit at the default +9), by day, with and
# without the one operator (8ZN71X) that carries most of the profit.
import json, collections, io, contextlib, bisect, datetime
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
with contextlib.redirect_stdout(io.StringIO()):
    exec(open(SP + "/oprule/arrival.py").read().split('print("by day: "')[0])
Z = open(SP + "/oprule/op8zn.txt").read().strip()
def n24(r):
    bt = his_buy[r["m"]]["bt"]; return len(times_before(r, bt - 86400, bt))
def boxlisted(r):
    R = last_refresh(his_buy[r["m"]]["bt"]); return bool(times_before(r, R - 7 * 86400, R))
for r in K2:
    r["n24"] = n24(r); r["box"] = boxlisted(r); r["z"] = r["signer"] == Z
    r["vb"] = r["v"] if r["box"] else r["v9"]          # the bot: table hold - 1 when listed, else the default +9
def line(lab, sel, k="vb"):
    byd = collections.defaultdict(float); n = 0
    for r in sel:
        if r[k] is None: continue
        byd[r["day"]] += r[k]; n += 1
    pos = sum(1 for d in DAYS if byd[d] > 0)
    print(f"   {lab:60} n {n:4d} copy {sum(byd.values()):+6.2f}  days+ {pos}/5  " + " ".join(f"{byd[d]:+5.2f}" for d in DAYS))
print("by day: " + " ".join(DAYS))
for lab, f in (("live: >= 2 in 24 h AND in the box's table", lambda r: r["n24"] >= 2 and r["box"]),
               (">= 2 in 24 h, listed or not (no table lag)", lambda r: r["n24"] >= 2),
               ("   of which not yet in the box's table (the lag)", lambda r: r["n24"] >= 2 and not r["box"]),
               (">= 1 in 24 h, listed or not", lambda r: r["n24"] >= 1),
               ("in the box's table, any count", lambda r: r["box"]),
               ("no operator rule at all", lambda r: True),
               ("   of which brand new (D)", lambda r: r["cls"] == "D")):
    line(lab, [r for r in K2 if f(r)])
    line("   ... without 8ZN71X", [r for r in K2 if f(r) and not r["z"]])
