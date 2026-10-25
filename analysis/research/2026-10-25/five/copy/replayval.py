# The copies the copy replay itself made (entry.copy = on, copy_listed_only, his 0.7 SOL+ buys; the mints it added to
# the exact replay's fires), valued like listed.py: our measured landing distances, bound +10%, sold at H - 1.
import json, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/dec/copy/exitopt.py").read().split("for sl, sel in")[0]
exec(src)
added = set(json.load(open(SP + "/ext/replay_copy/added.json")))
sel = [r for r in rows if r["m"] in added]
print(f"replay copies {len(added)}, with a tape and his buy at create + 0/1: {len(sel)}")
run("the replay's copies, sold at H - 1", sel, lambda r: r["H"] - 1)
run("the same, sold at H", sel, lambda r: r["H"])
