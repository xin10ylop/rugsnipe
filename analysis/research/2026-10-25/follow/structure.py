# The launch's transaction structure against his take, over every bundled launch of last night's operators, and inside
# the cells he mostly takes (>= 60%) vs mostly skips.
import json, collections
S = json.load(open("prebuy.json"))
def struct(d):
    c = d.get("create") or {}; bt = d.get("bundle_txs") or []
    idxs = sorted(x["idx"] for x in bt if x["idx"] is not None)
    if any(x["idx"] == d.get("idx") for x in bt): return "bundle inside the create tx"
    if c and c.get("pump_ix") == ["create_v2"]: return "create, no dev buy"
    if c and not c.get("pump_ix"): return "create (no pump ix decoded)"
    if not c: return "create tx not decoded"
    return f"separate bundle txs ({min(len(bt), 4)}{'+' if len(bt) >= 4 else ''})"
cell = collections.defaultdict(list)
for m, d in S.items(): cell[(d["op"], d["dev_fees"])].append(d)
rate = {k: sum(d["he"] for d in v) / len(v) for k, v in cell.items()}
T = collections.defaultdict(lambda: collections.Counter())
for k, v in cell.items():
    g = "cells he takes >= 60%" if rate[k] >= 0.6 else ("cells 10-60%" if rate[k] >= 0.1 else "cells < 10%")
    for d in v:
        s = struct(d); T[(s, "all")][d["he"]] += 1; T[(s, g)][d["he"]] += 1
for s in sorted({k[0] for k in T}):
    print(s)
    for g in ("all", "cells he takes >= 60%", "cells 10-60%", "cells < 10%"):
        c = T[(s, g)]; n = c[0] + c[1]
        if n: print(f"   {g:22s} {c[1]:4d} of {n:4d} ({100*c[1]/n:5.1f}%)")
