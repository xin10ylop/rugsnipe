# From the sample tapes: the price we would buy at behind its buy (end of its slot, end of the next slot, the first
# trade after it), as a premium over the state right after its buy; who trades right after it; and before its sell,
# whether others sell ahead of it.        python3 -I entry_slip.py WALLET
import sys, json, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
W = sys.argv[1]
D = json.load(open(f"{SP}/h9long/{W[:8]}_etapes.json"))
def q(xs, ps=(10, 25, 50, 75, 90)):
    xs = sorted(xs); return [round(xs[min(len(xs) - 1, int(p / 100 * len(xs)))], 4) for p in ps] if xs else None
for kind in ("pump, other quote", "pump, SOL quote"):
    xs = [v for v in D.values() if v and v["kind"] == kind]
    p0, p1, p2, nb, first_user = [], [], [], [], collections.Counter()
    ahead = []
    for v in xs:
        b, s = v["b"], v["s"]; tr = v["near"]
        # its own buy row in the tape
        oi = b["i"] or 0
        after = [r for r in tr if (r[0], r[1]) > (b["s"], oi) and r[0] <= b["s"] + 2]
        def prem(rs):
            return (b["vt"] / rs[-1][5]) ** 2 - 1 if rs else 0.0
        p0.append(prem([r for r in after if r[0] == b["s"]]))
        p1.append(prem([r for r in after if r[0] <= b["s"] + 1]))
        p2.append(prem(after))
        nb.append(sum(1 for r in after if r[0] <= b["s"] + 1))
        if after: first_user[(after[0][2][:6], after[0][3])] += 1
        # before its sell: trades in the slot before and in its slot ahead of it
        if True:
            si = s["i"] or 0
            pre = [r for r in tr if r[0] >= s["s"] - 1 and (r[0], r[1]) < (s["s"], si)]
            ahead.append(sum(1 for r in pre if r[3] == 0))
    print(f"== {kind}: {len(p0)} trips with its buy in the tape")
    print("   premium over the state after its buy, at the end of its slot", q(p0), " mean", round(sum(p0) / len(p0), 4))
    print("   ... at the end of the next slot", q(p1), " mean", round(sum(p1) / len(p1), 4))
    print("   ... at the end of slot + 2", q(p2), " mean", round(sum(p2) / len(p2), 4))
    print("   trades after its buy through the next slot", q(nb), " first trader after it", first_user.most_common(6))
    print("   sells by others in the slot before its sell and ahead of it in its slot", q(ahead))
    print("   transactions on the curve during its hold", q([v["n_between"] for v in xs]))
