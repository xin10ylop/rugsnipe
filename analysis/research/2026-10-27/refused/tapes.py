# Coin tapes for a list of mints (RPS env: the client's rate): every landed trade from the create to create + SPAN (default 60).
#   python3 -I tapes.py MINTS.json [SPAN] [THREADS]  ->  SP/refused/tape/<mint>.json  (skips mints already there)
import sys, json, os
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open("/home/user/rugsnipe/analysis/research/2026-10-27/refused/pull_live.py").read()
_a = sys.argv; sys.argv = [_a[0]]; exec(src[:src.index("ours = [s for s")]); sys.argv = _a
SPAN = int(sys.argv[2]) if len(sys.argv) > 2 else 60
TH = int(sys.argv[3]) if len(sys.argv) > 3 else 4
D = SP + "/refused/tape"; os.makedirs(D, exist_ok=True)
ms = [m for m in json.load(open(sys.argv[1])) if not os.path.exists(f"{D}/{m}.json")]
print(len(ms), "mints to fetch", flush=True)
def tape(mint):
    try:
        bc = orf.bonding_curve(mint)
        ss = [x for x in sigs_of(bc) if not x.get("err")]
        if not ss: return mint, None
        c0 = min(x["slot"] for x in ss)
        ss = [x for x in ss if x["slot"] <= c0 + SPAN]
        rows = []
        for x in ss:
            tx = api.rpc("getTransaction", [x["signature"], orf.TX_OPTS], cache=True)
            if not tx: continue
            for e in trades(tx, mint):
                rows.append([tx["slot"], tx.get("transactionIndex") or 0, e.user, int(e.is_buy), e.sol_amount, e.token_amount,
                             e.virtual_sol_reserves, e.virtual_token_reserves])
        rows.sort(key=lambda r: (r[0], r[1]))
        return mint, dict(c0=c0, span=SPAN, rows=rows)
    except Exception:
        return mint, None
n = 0
with ThreadPoolExecutor(TH) as ex:
    for m, t in ex.map(tape, ms):
        if t: json.dump(t, open(f"{D}/{m}.json", "w"))
        n += 1
        if n % 25 == 0: print(n, "/", len(ms), flush=True)
print("done", flush=True)
