# Who is first outside buyer on tweet-first (A) vs other (E) template+tool coins; is any fast rival selective?
import json, glob, collections, sys
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.argv = [sys.argv[0], "8"]
src = open(SP + "/univ/late2.py").read().split("for lab, rule in")[0]
exec(src)
st = collections.defaultdict(lambda: collections.Counter()); gaps = collections.defaultdict(list); allb = collections.defaultdict(lambda: collections.Counter())
for m, r in D.items():
    if not (r["dev"] >= 0.95 and TEMPLATE(r["dev"]) and r["host"] in TOOLS) or m not in creates: continue
    if m in tapes: seq, last = seq_from_tape(tapes[m])
    elif m in early: seq, last = seq_from_early(early[m])
    else: continue
    g = bool(r["sid"] and r["delay"] is not None and r["delay"] < 3600 and r["tweet_rank"] == 1)
    c = creates[m]; fo, end, ins = first_out(c, seq)
    # every outside buyer within create .. create+1, with position
    seen = set()
    for x in seq:
        if (x[0], x[1]) <= end or not x[3] or x[2] in ins or x[2] in seen or x[0] > c["slot"] + 1: continue
        seen.add(x[2]); allb[x[2]]["A" if g else "E"] += 1
    if not fo: continue
    st[fo[2]]["A" if g else "E"] += 1
    if fo[0] == c["slot"]: gaps[fo[2]].append(fo[1] - end[1])
tot = collections.Counter()
for w, c in st.items(): tot["A"] += c["A"]; tot["E"] += c["E"]
print(f"first outside buyer on template+tool coins: A (tweet-first&fresh) {tot['A']}, E (other) {tot['E']}; A share {tot['A']/(tot['A']+tot['E']):.0%}")
print(f"{'wallet':12s} {'first A':>7s} {'first E':>7s} {'A share':>7s} | {'any early buy A':>15s} {'E':>4s} {'A share':>7s} | d=0 gap median")
for w, c in sorted(st.items(), key=lambda kv: -(kv[1]["A"] + kv[1]["E"]))[:20]:
    a, e = c["A"], c["E"]; aa, ee = allb[w]["A"], allb[w]["E"]; g = sorted(gaps[w])
    print(f"{w[:12]} {a:7d} {e:7d} {a/(a+e):7.0%} | {aa:15d} {ee:4d} {aa/max(aa+ee,1):7.0%} | {g[len(g)//2] if g else '-'}")
