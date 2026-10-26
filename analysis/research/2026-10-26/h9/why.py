# Why the bot did not see some launch buys of the copied wallets (Oct 26): each coin's create on chain (token program,
# mayhem, quote mint) and the wallet's first buy slot against the create slot.
import sys, json
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, "/home/user/rugsnipe/analysis"); sys.path.insert(0, "/home/user/rugsnipe/analysis/scripts/strategy")
import tri
import operators_refresh as orf
from rugsnipe_analysis import events as E
T2022 = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PS1t7LPmFSb5ZKt"
W = {"H9kuqGKNYgPFYCA17uUTkaqXGum7MK8y1GwbS7cxsjDg": "71aS9awB FHSqzn75 notAk7Y2 yxC5Yswu XsqE9cRR BALL9nV7 7Mxv83dk REtaGvoE 3Arw3S6q DBAaY3qu CATWnq78 7i1LUpid pumPK29D",
     "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2": "2XcbfcHG 9Nv4aEyF GzVcYCPW D3izRQqJ 6mQNS17e",
     "3yrJE1A8dgSuy5NLimG5GSYxA6wMkvnAYLD9Y4xR2fjJ": "2nh3hYzM 5JeRMtTB"}
T0 = 1793019600
def wallet_buys(w, want):
    sigs, before = [], None
    while True:
        page = tri.sigs(w, before)
        sigs += [x for x in page if (x.get("blockTime") or 0) >= T0 and not x.get("err")]
        if len(page) < 1000 or (page[-1].get("blockTime") or 0) < T0: break
        before = page[-1]["signature"]
    def one(x):
        t = tri.tx(x["signature"])
        return orf.parse_wallet_rpc_tx(t, w) if t else None
    first = {}
    with ThreadPoolExecutor(8) as ex:
        for p in ex.map(one, sigs):
            if p and p["mint"][:8] in want and p["tok"] > 0:
                if p["mint"] not in first or p["slot"] < first[p["mint"]]["slot"]: first[p["mint"]] = p
    return first
def create_of(mint):
    before, last = None, None
    for _ in range(60):
        page = tri.sigs(mint, before)
        if not page: break
        last = page[-1]; before = last["signature"]
        if len(page) < 1000: break
    if not last: return None
    t = tri.rpc("getTransaction", [last["signature"], {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}])
    evs = [e for e in E.extract_events(t) if isinstance(e, E.CreateEvent)] if t else []
    progs = E.top_level_programs(t) if t else []
    return dict(slot=t["slot"] if t else None, ev=evs[0] if evs else None, progs=progs)
for w, s in W.items():
    want = set(s.split())
    buys = wallet_buys(w, want)
    print(f"== {w[:8]}: {len(buys)} of {len(want)} found")
    for mint, p in sorted(buys.items(), key=lambda kv: kv[1]["slot"]):
        c = create_of(mint)
        ev = c and c["ev"]
        if not ev:
            print(f"   {mint[:8]}  no pump.fun CreateEvent in the first tx (programs {[x[:6] for x in (c or {}).get('progs', [])]})")
            continue
        at = p["slot"] - c["slot"]
        why = []
        if not ev.sol_quoted: why.append(f"quoted in {ev.quote_mint[:8]}")
        if ev.is_mayhem_mode: why.append("mayhem")
        if ev.token_program != T2022: why.append("legacy SPL token (create v1)")
        if at > 1: why.append(f"its buy at create+{at}")
        print(f"   {mint[:8]}  its buy create+{at:<3}  {p['delta'] / -1e9:6.3f} SOL  token {('2022' if ev.token_program == T2022 else 'spl')}  "
              f"mayhem {ev.is_mayhem_mode}  quote {'SOL' if ev.sol_quoted else ev.quote_mint[:8]}  ->  {', '.join(why) or 'IN CLASS at create+0/1 (cashback? feed miss?)'}")
