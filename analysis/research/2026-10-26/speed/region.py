# Our at-create fires (old bot) and his buys over Oct 21-25: how far after the create they land (same slot: block
# positions; else next slot), by the region of the create slot's leader. The create is the trigger, so this is the
# whole see-it-then-send pipeline from Frankfurt, the same one the copy runs on his buy.
import json, glob, collections, statistics as st
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"; F = SP + "/dec/five"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
REG = json.load(open(SP + "/ext/replay_copy/leader_regions.json"))["regions"]
LEAD = {}
for p in glob.glob(SP + "/ext/replay_copy/leaders/*.json"):
    d = json.load(open(p))
    for k, v in d["schedule"].items():
        for i in v: LEAD[d["first_slot"] + i] = k
crm = {}
for p in glob.glob(F + "/coin/*.json"):
    d = json.load(open(p))
    if d.get("create"): crm[d["mint"]] = (d["cs"], d["create"]["idx"])
def reg(slot):
    k = LEAD.get(slot); return REG.get(k, "unknown") if k else "nosched"
for w, lab in ((US, "our fires"), (A, "his buys")):
    rows = json.load(open(f"{F}/walk_{w[:6]}.json"))["rows"]
    by = collections.defaultdict(list)
    for r in rows:
        if r["kind"] != "buy" or r["m"] not in crm: continue
        cs, ci = crm[r["m"]]
        if r["slot"] > cs + 1 or r["slot"] < cs: continue
        by[reg(cs)].append(r["idx"] - ci if r["slot"] == cs else None)
    print(f"\n{lab}: create-slot leader region -> share landing in the create slot, positions after the create there")
    for k, v in sorted(by.items(), key=lambda x: -len(x[1])):
        s = sorted(x for x in v if x is not None)
        q = lambda f: s[int(f * (len(s) - 1))] if s else "-"
        print(f"  {k:8} n {len(v):5d}  same slot {100*len(s)/len(v):4.0f}%  positions p10 {q(.1)} p50 {q(.5)} p90 {q(.9)}")
