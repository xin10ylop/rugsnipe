# Our copy of him by hour, simulated on the 5-day tapes (dec/five, Oct 21-25): his landed launch buys our rule copies
# today (tip >= 2M, or 2 SOL and up), our measured landing distances, the 5% bound, sold one slot before the table's hold
# (his sell - 1), 0.5 SOL. -> hours/sim.json [(his buy time, our copy's value)]
import io, contextlib, json, os, sys
os.environ["PREM"] = "0.05"
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
os.chdir(SP + "/dec/copy")
src = open("scratch.py").read().split("for sl, sel in")[0]
with contextlib.redirect_stdout(io.StringIO()):
    exec(src)
out = []
for r in rows:
    if not (r["tip"] >= 2e6 or r["sol"] >= 2.0): continue
    vs = [one(r, D, "anticipate", 0.5) for D in dec]; vs = [v for v in vs if v is not None]
    if not vs: continue
    out.append([his_buy[r["m"]]["bt"], sum(vs) / len(vs), r["day"]])
json.dump(out, open(SP + "/hours/sim.json", "w"))
print(len(out), "copies", f"{sum(v for _, v, _ in out):+.2f} SOL")
