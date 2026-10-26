# Wallet changes: each finalist's SOL counterparties since T0 (standalone transfers only: a trade pays the curve or
# pool by a system transfer, so transactions with a trade or another program are left out; tips excluded), and whether a
# counterparty is itself a launch buyer we saw (the 5-day / wide tapes, the broad discovery) -> the same trader on
# another wallet, the place it moves to when this one goes quiet.
import json, glob, os, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
F = [l.strip() for l in open(SP + "/oos/finalists1.txt") if l.strip()]
known = {o["w"]: o for o in json.load(open(SP + "/wallets/discover.json"))}
S = json.load(open(SP + "/wallets/screen2.json"))
buyers = collections.Counter()
for p in glob.glob(SP + "/wide/coin/*.json"):
    d = json.load(open(p))
    for r in d.get("rows") or []:
        if r[3] == 1: buyers[r[2]] += 1
for w in F:
    p = f"{SP}/oos/{w[:8]}.jsonl"
    if not os.path.exists(p): continue
    cin = collections.Counter(); cout = collections.Counter(); nin = collections.Counter(); nout = collections.Counter()
    for l in open(p):
        r = json.loads(l)
        if r.get("tr") or r.get("pix") or r.get("calls"): continue   # a trade pays the curve / pool by transfer: not a wallet link
        for s, d, lam in r.get("xf") or []:
            if d == w: cin[s] += lam; nin[s] += 1
            elif s == w: cout[d] += lam; nout[d] += 1
    def tag(k):
        t = []
        if k in known: t.append(f"launch buyer ({known[k]['n']} buys)")
        if k in S: t.append("screened")
        if buyers[k]: t.append(f"{buyers[k]} buys on the tapes")
        return ("  <- " + ", ".join(t)) if t else ""
    print(f"{w[:8]}: out {sum(cout.values()) / 1e9:.2f} SOL to {len(cout)} wallets, in {sum(cin.values()) / 1e9:.2f} from {len(cin)}")
    for k, v in cout.most_common(4): print(f"    out {v / 1e9:8.2f} x{nout[k]:<4d} {k}{tag(k)}")
    for k, v in cin.most_common(3): print(f"    in  {v / 1e9:8.2f} x{nin[k]:<4d} {k}{tag(k)}")
