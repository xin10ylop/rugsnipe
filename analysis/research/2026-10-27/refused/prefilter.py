# A cheap pre-filter: which launches get a ghost? A random sample of ALL pump creates over Oct 23-24 (the mint
# authority's signatures), classified as the bot does (in class: create_v2, not mayhem, not cashback, SOL quote), with
# each one's create-time fields (signer, creator, metadata host, the create's programs = launch tool, dev buy). Then
# candidate filters from his buys BEFORE each launch (5-day set, no look-ahead): the share of all in-class launches that
# pass (the spray cost) against the share of his keyed buys that pass (the recall).   -> SP/refused/prefilter.json
#   python3 -I prefilter.py [N]
import sys, json, random, collections, struct, bisect, os
from concurrent.futures import ThreadPoolExecutor
_N = int(sys.argv[1]) if len(sys.argv) > 1 else 1000
exec(open("/home/user/rugsnipe/analysis/research/2026-10-27/refused/eng.py").read())
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, common as C
from rugsnipe_analysis import events as E
MA = "TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM"; PUMP = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
LO, HI = 1792713600, 1792886400            # Oct 23 00:00 - Oct 25 00:00 UTC
OUTP = SP + "/refused/prefilter.json"
if not os.path.exists(OUTP):
    sigs = []; before = None
    while True:
        o = {"limit": 1000}
        if before: o["before"] = before
        pg = tri.rpc("getSignaturesForAddress", [MA, o], cache=False) or []
        if not pg: break
        sigs += [x for x in pg if not x.get("err") and LO <= (x.get("blockTime") or 0) < HI]
        before = pg[-1]["signature"]
        if (pg[-1].get("blockTime") or 0) < LO or len(pg) < 1000: break
    print("creates Oct 23-24:", len(sigs), f"({len(sigs) / 2:.0f} a day)", flush=True)
    random.seed(3); smp = random.sample(sigs, min(_N, len(sigs)))
    def one(x):
        tx = tri.rpc("getTransaction", [x["signature"], {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}], cache=False)
        if not tx: return None
        logs = " ".join(tx["meta"]["logMessages"] or [])
        m = tx["transaction"]["message"]; la = tx["meta"].get("loadedAddresses") or {}
        keys = m["accountKeys"] + la.get("writable", []) + la.get("readonly", [])
        tail = None
        for ix in m["instructions"] + [i for g in tx["meta"].get("innerInstructions") or [] for i in g["instructions"]]:
            if keys[ix["programIdIndex"]] != PUMP: continue
            d = C.b58d(ix["data"])
            try:
                o = 8
                for _ in range(3): n = struct.unpack_from("<I", d, o)[0]; o += 4 + n
                o += 32; tail = list(d[o:o + 2]); break
            except Exception: continue
        ev = [e for e in E.extract_events(tx) if isinstance(e, E.CreateEvent)]
        tr = [e for e in E.extract_events(tx) if isinstance(e, E.TradeEvent)]
        e = ev[0] if ev else None
        host = (e.uri.split("/")[2] if e and "://" in e.uri else "") if e else ""
        progs = sorted({keys[ix["programIdIndex"]] for ix in m["instructions"]})
        return dict(slot=tx["slot"], bt=tx.get("blockTime"), signer=keys[0], creator=e.creator if e else None, host=host,
                    v2="Instruction: CreateV2" in logs, tail=tail, sol=(e.sol_quoted if e else None),
                    progs=progs, dev=sum(t.sol_amount for t in tr if t.is_buy) / 1e9, mint=e.mint if e else None)
    with ThreadPoolExecutor(8) as ex:
        S = [s for s in ex.map(one, smp) if s]
    json.dump(dict(n_creates=len(sigs), sample=S), open(OUTP, "w"))
P = json.load(open(OUTP)); S = P["sample"]
inc = [s for s in S if s["v2"] and s["tail"] and not s["tail"][0] and not (s["tail"][1:2] or [0])[0] and s["sol"] is not False]
day = P["n_creates"] / 2 * len(inc) / len(S)
print(f"creates Oct 23-24: {P['n_creates'] / 2:.0f} a day; in class {len(inc)} of {len(S)} sampled -> {day:.0f} in-class launches a day")
# his buys before each moment: signers, hosts, launch tools (5-day set)
def tool(progs): return ",".join(sorted(p[:6] for p in progs if p not in (PUMP,)))[:60]
his = []
for r in rows:
    d = coins.get(r["m"])
    if not d: continue
    c = d["create"]; his.append((his_buy[r["m"]]["bt"], c["signer"], c.get("host") or "", r["tip"] >= 2e6 or r["sol"] >= 2.0, c.get("progs") or []))
his.sort()
HT = [h[0] for h in his]
def prior(t, win):
    i = bisect.bisect_left(HT, t); j = bisect.bisect_left(HT, t - win)
    return his[j:i]
FILTERS = {
    "every in-class launch": lambda t, sg, host: True,
    "signer he bought in the 24 h before": lambda t, sg, host: any(h[1] == sg for h in prior(t, 86400)),
    "signer he bought in the 72 h before": lambda t, sg, host: any(h[1] == sg for h in prior(t, 3 * 86400)),
    "signer he bought keyed in the 72 h before": lambda t, sg, host: any(h[1] == sg and h[3] for h in prior(t, 3 * 86400)),
    "host he bought in the 24 h before": lambda t, sg, host: any(h[2] == host for h in prior(t, 86400)),
    "host he bought keyed 3+ times in the 24 h before": lambda t, sg, host: sum(1 for h in prior(t, 86400) if h[2] == host and h[3]) >= 3,
    "signer or host he bought in the 24 h before": lambda t, sg, host: any(h[1] == sg or h[2] == host for h in prior(t, 86400)),
}
print(f"   {'pre-filter':52} {'launches kept':>14} {'a day':>7} {'his keyed buys kept':>20}")
for name, f in FILTERS.items():
    kept = sum(1 for s in inc if s["bt"] and f(s["bt"], s["signer"], s["host"])) / len(inc)
    ks = [h for h in his if h[3] and h[0] >= LO]
    rec = sum(1 for h in ks if f(h[0] - 1, h[1], h[2])) / max(len(ks), 1)
    print(f"   {name:52} {kept:13.1%} {kept * day:7.0f} {rec:19.0%}")
