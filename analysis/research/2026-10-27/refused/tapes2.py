# Coin tapes, cheap: the curve's signatures anchored at a block past create + SPAN (no paging through the coin's whole
# life), every trade of the create slot and the 3 after (where his buy, the in-between buyers and ours sit), then only
# each later slot's closing trades (the state at its end: exits, re-entries), plus every trade of his ATA's (his sells).
#   python3 -I tapes2.py ITEMS.json [SPAN] [THREADS]   items: [[mint, slot_guess], ...] -> SP/refused/tape/<mint>.json
import sys, json, os, collections, hashlib
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open("/home/user/rugsnipe/analysis/research/2026-10-27/refused/pull_live.py").read()
_a = sys.argv; sys.argv = [_a[0]]; exec(src[:src.index("ours = [s for s")]); sys.argv = _a
SPAN = int(sys.argv[2]) if len(sys.argv) > 2 else 60
TH = int(sys.argv[3]) if len(sys.argv) > 3 else 4
DENSE = 3
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; AT = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
T22 = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PiQyztk3gB2c6Ub"
def pda(seeds, pid):
    for bump in range(255, -1, -1):
        h = hashlib.sha256(b"".join(seeds) + bytes([bump]) + orf.b58d(pid) + b"ProgramDerivedAddress").digest()
        if not orf._on_curve(h): return orf.b58e(h)
D = SP + "/refused/tape"; os.makedirs(D, exist_ok=True)
items = [x for x in json.load(open(sys.argv[1])) if not os.path.exists(f"{D}/{x[0]}.json")]
print(len(items), "mints to fetch", flush=True)
def anchor(slot):
    for s in range(slot, slot + 8):
        try:
            b = api.rpc("getBlock", [s, {"transactionDetails": "signatures", "rewards": False, "maxSupportedTransactionVersion": 0, "commitment": "confirmed"}])
            if b and b.get("signatures"): return b["signatures"][-1]
        except Exception:
            continue
    return None
def rows_of(sig, mint):
    tx = api.rpc("getTransaction", [sig, orf.TX_OPTS], cache=False)
    if not tx: return []
    return [[tx["slot"], tx.get("transactionIndex") or 0, e.user, int(e.is_buy), e.sol_amount, e.token_amount,
             e.virtual_sol_reserves, e.virtual_token_reserves] for e in trades(tx, mint)]
def tape(item):
    mint, guess = item
    try:
        bc = orf.bonding_curve(mint); anc = anchor(guess + SPAN + 2)
        ss, before = [], anc
        for _ in range(60):
            o = {"limit": 1000}
            if before: o["before"] = before
            page = api.rpc("getSignaturesForAddress", [bc, o]) or []
            ss += page
            if len(page) < 1000 or page[-1]["slot"] < guess - 3: break
            before = page[-1]["signature"]
        ss = [x for x in ss if not x.get("err")]
        if not ss: return mint, None
        c0 = min(x["slot"] for x in ss)
        by = collections.defaultdict(list)
        for x in ss:
            if c0 <= x["slot"] <= c0 + SPAN: by[x["slot"]].append(x["signature"])
        want = []
        for s, sigs in by.items():
            want += sigs if s <= c0 + DENSE else sigs[:2]      # newest first: the slot's last trades
        ata = pda([orf.b58d(A), orf.b58d(T22), orf.b58d(mint)], AT)
        his = [x["signature"] for x in (api.rpc("getSignaturesForAddress", [ata, {"limit": 100}]) or []) if not x.get("err")]
        want = list(dict.fromkeys(want + his))
        rows = []
        for sg in want: rows += rows_of(sg, mint)
        rows = sorted({(r[0], r[1], r[2], r[3]): r for r in rows}.values(), key=lambda r: (r[0], r[1]))
        return mint, dict(c0=c0, span=SPAN, dense=DENSE, rows=rows, n_sigs=sum(len(v) for v in by.values()))
    except Exception as ex:
        print("err", mint[:8], str(ex)[:80], flush=True)
        return mint, None
n = 0
with ThreadPoolExecutor(TH) as ex:
    for m, t in ex.map(tape, items):
        if t: json.dump(t, open(f"{D}/{m}.json", "w"))
        n += 1
        if n % 25 == 0: print(n, "/", len(items), flush=True)
print("done", flush=True)
