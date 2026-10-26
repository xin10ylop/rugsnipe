# Insiders or followers? For each finalist's launch buys on the fresh tapes (oos/coin): is the coin's create signer or
# creator a wallet the finalist sent SOL to (since T0, from its own transactions), i.e. a dev wallet it funded?
import json, glob, os, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
F = [l.strip() for l in open(SP + "/oos/finalists1.txt") if l.strip()]
tapes = {}
for p in glob.glob(SP + "/oos/coin/*.json"):
    d = json.load(open(p))
    if d.get("create") and d.get("rows"): tapes[d["mint"]] = d
for w in F:
    p = f"{SP}/oos/{w[:8]}.jsonl"
    if not os.path.exists(p): continue
    funded = set(); bought = set()
    for l in open(p):
        r = json.loads(l)
        for s, d, lam in r.get("xf") or []:
            if s == w: funded.add(d)
        for t in r.get("tr") or []:
            if t[1] == w and t[2] == 1: bought.add(t[0])
    have = [m for m in bought if m in tapes]
    ins = [m for m in have if tapes[m]["create"]["signer"] in funded or tapes[m]["create"]["creator"] in funded]
    early = [m for m in have if any(r[2] == w and r[3] == 1 and r[0] <= tapes[m]["cs"] + 1 for r in tapes[m]["rows"])]
    ins_e = [m for m in early if m in ins]
    print(f"{w[:8]}: coins bought {len(bought)}, with a tape {len(have)}, launch buys {len(early)}; created by a wallet it funded: {len(ins)} ({len(ins_e)} of the launch buys)")
