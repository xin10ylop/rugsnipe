# The operator's own bundle tip (seen before we fire) on every coin with a rebuilt race: value at +10 for the first
# spot, the clearing bid, and our live wins, at <= 40 and > 40.
import json, collections, statistics as st
RC = json.load(open("big/race.json")); R = json.load(open("an_rows.json"))
AU = json.load(open("big/auction.json")); AU.update(json.load(open("big/auction40.json")))
def btip(m):
    d = json.load(open(f"coin/{m}.json")); c = d["create"]
    ins = {c["signer"], c["creator"]} | set(R[m]["bw"])
    run = [r for r in RC[m] if r["payer"] in ins and r["slot"] == 0 and r["ok"]]
    return sum(r["tip"] for r in run) / 1e6, len(run)
BT = {m: btip(m) for m in AU}
json.dump(BT, open("big/btip.json", "w"))
def bt(b): return "a 0" if b == 0 else "b <=4M" if b <= 4 else "c 4-12M" if b <= 12 else "d 12-40M" if b <= 40 else "e 40-100M" if b <= 100 else "f >100M"
for lab, f in (("pb <= 40 (newest bot fires)", lambda m: AU[m]["pb"] <= 40), ("pb > 40 (followed rule)", lambda m: AU[m]["pb"] > 40)):
    g = collections.defaultdict(list)
    for m in AU:
        if f(m): g[bt(BT[m][0])].append(m)
    print(f"\n== {lab}: operator's bundle tip")
    print(f"{'bundle tip':12} {'coins':>5} {'v10':>7} {'median':>7} {'dump':>5} {'clear med':>9} {'we won':>6} {'our live':>8} {'per win':>8} {'he won':>6}")
    for k in sorted(g):
        ms = g[k]; v = [AU[m]["v10"] for m in ms]
        w = [m for m in ms if AU[m]["win"] == "US" and AU[m]["us_ok"]]
        print(f"{k:12} {len(ms):5d} {100*st.mean(v):+6.1f}% {100*st.median(v):+6.1f}% {sum(a < -0.25 for a in v):5d} {st.median(AU[m]['win_bid'] for m in ms):9.2f} {len(w):6d} "
              f"{sum(AU[m]['us_net'] for m in w):+8.3f} {(sum(AU[m]['us_net'] for m in w)/len(w)) if w else 0:+8.4f} {sum(AU[m]['win'] == 'HIM' for m in ms):6d}")
