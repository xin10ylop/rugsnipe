# The lamport-level key a guard program could apply when it executes after his buy: from his wallet's drop (his balance
# when we sent, passed in, minus his balance now), minus the base fee and his token-account rent (one of three exact
# values, told apart by the low digits), modulo 0.1 SOL: x = his tip (+ priority fee, which ends in 1 lamport) (- the
# lamport his tier sometimes loses). Key: x >= 2M with no priority remainder, or his size >= 2 SOL. Its money: the ghost
# on the buys it passes (ghost4.json at P_AFTER 0.8: keyed gain over today, others' loss, sampled and scaled), by period.
import json, collections
exec(open("/home/user/rugsnipe/analysis/research/2026-10-27/refused/eng.py").read())
RENTS = (1513840, 2164080, 1488440)
G = {x["m"]: x for x in json.load(open(SP + "/refused/ghost4.json"))}
NO = sum(1 for x in G.values() if not x["keyed"]); SC = 1109 / NO
def guard(r):
    b = his_buy[r["m"]]; drop = -b["delta"]
    if r["sol"] >= 2: return True
    for R in RENTS:
        x = (drop - 5000 - R) % 100_000_000
        if x < 60_000_000: return x >= 1_999_000 and x % 10_000 in (0, 9999)
    return False
PER = {"P1 Oct 21-23": ("10-21", "10-22", "10-23"), "P2 Oct 24-25": ("10-24", "10-25")}
for cap in ("0.05", "0.1"):
    print(f"== ghost bound post-bundle + {float(cap):.0%}, ghost only when the guard key passes")
    for p, ds in PER.items():
        gk = go = 0.0; nk = no = 0; miss = 0.0
        for r in rows:
            x = G.get(r["m"])
            if not x or r["day"] not in ds: continue
            if not guard(r):
                continue
            if x["keyed"]: gk += x[cap] - BIAS * x[cap + ":add"] - x["react"]; nk += 1
            else: go += (x[cap] - BIAS * x[cap + ":add"]) * SC; no += 1
        print(f"   {p}: keyed passed {nk}, gain {gk:+.2f}; others passed ~{no * SC:.0f}, {go:+.2f}  ->  {gk + go:+.2f} SOL before the spray "
              f"({(gk + go) / len(ds):+.2f} a day)")

# without each period's best 2 coins, at both bounds; and the same at other P_AFTER (rerun ghost4.py with it first)
print("\n   without each period's best 2 coins:")
for cap in ("0.05", "0.1"):
    cells = []
    for p, ds in PER.items():
        vals = []
        for r in rows:
            x = G.get(r["m"])
            if not x or r["day"] not in ds or not guard(r): continue
            vals.append((x[cap] - BIAS * x[cap + ":add"] - x["react"]) if x["keyed"] else (x[cap] - BIAS * x[cap + ":add"]) * SC)
        vals.sort(reverse=True)
        cells.append(f"{p} {sum(vals[2:]):+.2f} ({sum(vals[2:]) / len(ds):+.2f}/day)")
    print(f"   bound post-bundle + {float(cap):.0%}: " + "   ".join(cells))

# with the cheap pre-filter: a ghost only on launches whose signer he bought in the 72 h before (11.3% of in-class
# launches on Oct 23-24, prefilter.py), and the spray at that rate (16,902 in-class launches a day then; 15,000 lamports)
import bisect
ev = collections.defaultdict(list)
for r in rows:
    d = coins.get(r["m"])
    if d: ev[d["create"]["signer"]].append(his_buy[r["m"]]["bt"])
for s in ev: ev[s].sort()
def pre(r):
    v = ev[coins[r["m"]]["create"]["signer"]]; t = his_buy[r["m"]]["bt"]
    return bisect.bisect_left(v, t) - bisect.bisect_left(v, t - 3 * 86400) > 0
SPRAY = 16902 * 0.113 * 15000 / 1e9
print(f"\n   with the pre-filter (signer bought in the 72 h before), spray {SPRAY:.3f} SOL a day:")
for cap in ("0.05", "0.1"):
    cells = []
    for p, ds in PER.items():
        vals = []
        for r in rows:
            x = G.get(r["m"])
            if not x or r["day"] not in ds or not guard(r) or not pre(r): continue
            vals.append((x[cap] - BIAS * x[cap + ":add"] - x["react"]) if x["keyed"] else (x[cap] - BIAS * x[cap + ":add"]) * SC)
        vals.sort(reverse=True)
        cells.append(f"{p} {sum(vals) / len(ds) - SPRAY:+.3f}/day [{sum(vals[2:]) / len(ds) - SPRAY:+.3f}]")
    print(f"   bound post-bundle + {float(cap):.0%}: " + "   ".join(cells))
