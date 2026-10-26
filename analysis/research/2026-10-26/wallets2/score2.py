# Stage 2 of the second search: the candidates on all their launch-buy coins (their own trades pulled, tapes fetched),
# copied as the bot copies (our landing distances, +5%, 0.5 SOL, sold at create + hold whatever they do). The hold is
# picked per wallet on the first half (Oct 25 18:00 -> Oct 26 06:00) and scored on the second (06:00 -> the pull).
# Every buy of theirs counts here, not only those on the finalists' coins.
import json, glob, collections, os, sys, datetime, bisect
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/oos/discover2.py").read()
exec(src[:src.index("KNOWN = ")])
C = [l.strip() for l in open(SP + "/oos/cands2.txt") if l.strip()]
# his tip on each of his buys (his own transactions): the live copy of him takes tip >= 2M only
HIM = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; tip_of = {}
if os.path.exists(SP + "/oos/AHwJSnFN.jsonl"):
    for l in open(SP + "/oos/AHwJSnFN.jsonl"):
        r = json.loads(l)
        for t in r.get("tr") or []:
            if t[1] == HIM and t[2] == 1 and t[0] not in tip_of: tip_of[t[0]] = r.get("tip") or 0
RUNS = [(w, w[:8], None) for w in C]
MID = 1792994400
blocks = ["25 18h", "26 00h", "26 06h", "26 12h"]
print(f"{'wallet':12} {'copies':>6}  hold picked on the 1st half -> 1st half | 2nd half   by 6-hour block (that hold)   all holds 2nd half: +5 / +9 / +12")
for w, lab, keep in RUNS:
    rows = []
    for t in trades.get(w, []):
        if keep and not keep(t): continue
        bt = bt2(t["x"][0])
        if bt < T0: continue
        r = dict(bt=bt, blk=block(bt))
        for h in (5, 9, 12): r[h] = copy_val(t["m"], t["x"], None, h)
        rows.append(r)
    if not rows: print(f"{lab}: no launch buys on the tapes"); continue
    h1 = {h: sum(r[h] or 0 for r in rows if r["bt"] < MID) for h in (5, 9, 12)}
    h2 = {h: sum(r[h] or 0 for r in rows if r["bt"] >= MID) for h in (5, 9, 12)}
    hp = max(h1, key=h1.get)
    byb = collections.defaultdict(float)
    for r in rows: byb[r["blk"]] += r[hp] or 0
    n1 = sum(1 for r in rows if r["bt"] < MID); n2 = len(rows) - n1
    print(f"{lab:12} {len(rows):6d}  create+{hp:<2}  {h1[hp]:+6.2f} ({n1}) | {h2[hp]:+6.2f} ({n2})   "
          + " ".join(f"{b} {byb.get(b, 0):+.2f}" for b in blocks) + f"   {h2[5]:+.2f} / {h2[9]:+.2f} / {h2[12]:+.2f}")
