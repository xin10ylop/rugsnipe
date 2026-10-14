"""Fetch blocks (getBlock full json) and extract compact pump.fun-relevant records."""
import base64, json, os, sys, threading, time, random, struct
import requests
sys.path.insert(0, '/home/user/rugsnipe/analysis')
from rugsnipe_analysis.events import b58decode, _decode_any, EVENT_IX_TAG, PUMP_PROGRAM, CreateEvent, TradeEvent

SP = os.path.dirname(os.path.abspath(__file__))
TIPS = json.load(open(os.path.join(SP, '..', 'tip_accounts.json')))
OUT = os.path.join(SP, 'blocks')
os.makedirs(OUT, exist_ok=True)
EPS = ["https://solana-rpc.publicnode.com", "https://api.mainnet-beta.solana.com"]
VOTE = "Vote111111111111111111111111111111111111111"
CB = "ComputeBudget111111111111111111111111111111"
IDL = json.load(open(os.path.join(SP, '..', 'pump_idl.json')))
IXDISC = {bytes(i['discriminator']): i['name'] for i in IDL['instructions']}
_loc = threading.local()

def sess():
    s = getattr(_loc, 's', None)
    if s is None:
        s = requests.Session(); s.headers['content-type'] = 'application/json'; _loc.s = s
    return s

def rpc_block(slot):
    body = {"jsonrpc": "2.0", "id": 1, "method": "getBlock", "params": [slot, {
        "transactionDetails": "full", "encoding": "json", "rewards": True,
        "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}]}
    delay = 1.0; last = None; archive = False
    for attempt in range(14):
        ep = EPS[1] if archive else (EPS[0] if attempt < 4 else EPS[attempt % 2])
        try:
            r = sess().post(ep, json=body, timeout=60)
            if r.status_code == 429 or r.status_code >= 500:
                last = f"http {r.status_code} {ep}"; raise RuntimeError(last)
            d = r.json()
            if 'error' in d:
                code = d['error'].get('code'); msg = d['error'].get('message', '')
                if code in (-32007, -32009):  # skipped slot / missing
                    if code == -32009 and ep == EPS[0]:
                        last = msg; raise RuntimeError(msg)  # pruned on publicnode -> try archive
                    return {"skipped": True, "code": code, "msg": msg}
                if 'cleaned up' in msg: archive = True
                last = f"{code} {msg}"; raise RuntimeError(last)
            return d['result']
        except Exception as e:
            last = str(e)[:200]; print("retry", slot, attempt, last[:100], flush=True)
            time.sleep(delay + random.random()); delay = min(delay * 1.7, 20)
    raise RuntimeError(f"getBlock {slot} failed: {last}")

def keys_of(tx):
    m = tx['transaction']['message']; la = tx['meta'].get('loadedAddresses') or {}
    return m['accountKeys'] + la.get('writable', []) + la.get('readonly', [])

def parse_err(err):
    if err is None: return None
    if isinstance(err, dict) and 'InstructionError' in err:
        e = err['InstructionError'][1]
        if isinstance(e, dict) and 'Custom' in e: return f"C{e['Custom']}"
        return str(e)[:40]
    return str(err)[:60]

def extract(slot, blk):
    if blk is None or blk.get('skipped'):
        return {"slot": slot, "skipped": True}
    leader = next((r['pubkey'] for r in (blk.get('rewards') or []) if r.get('rewardType') == 'Fee'), None)
    txs = blk['transactions']; n_vote = 0; rows = []
    for i, tx in enumerate(txs):
        keys = keys_of(tx)
        m = tx['transaction']['message']
        if VOTE in m['accountKeys'][:5] or (len(m['accountKeys']) <= 3 and VOTE in m['accountKeys']):
            n_vote += 1; continue
        if PUMP_PROGRAM not in keys: continue
        meta = tx['meta']; hdr = m['header']; nsig = hdr['numRequiredSignatures']
        signers = m['accountKeys'][:nsig]
        evs = []; ixn = []
        for ix in m['instructions']:
            if keys[ix['programIdIndex']] == PUMP_PROGRAM:
                try: ixn.append(IXDISC.get(b58decode(ix['data'])[:8], '?'))
                except Exception: pass
        for g in meta.get('innerInstructions') or []:
            for ix in g['instructions']:
                if keys[ix['programIdIndex']] != PUMP_PROGRAM: continue
                try: raw = b58decode(ix['data'])
                except Exception: continue
                if raw[:8] == EVENT_IX_TAG:
                    ev = _decode_any(raw[8:])
                    if isinstance(ev, CreateEvent):
                        evs.append({"t": "c", "mint": ev.mint, "bc": ev.bonding_curve, "user": ev.user, "creator": ev.creator,
                                    "vsol": ev.virtual_sol_reserves, "vtok": ev.virtual_token_reserves, "mayhem": ev.is_mayhem_mode,
                                    "quote": ev.quote_mint, "name": ev.name, "sym": ev.symbol})
                    elif isinstance(ev, TradeEvent):
                        evs.append({"t": "b" if ev.is_buy else "s", "mint": ev.mint, "sol": ev.sol_amount, "tok": ev.token_amount,
                                    "user": ev.user, "vsol": ev.virtual_sol_reserves, "vtok": ev.virtual_token_reserves, "ix": ev.ix_name})
                elif not ixn:
                    ixn.append('cpi:' + IXDISC.get(raw[:8], '?'))
        pre, post = meta['preBalances'], meta['postBalances']
        tips = {}
        for k_i, k in enumerate(keys[:len(pre)]):
            if k in TIPS:
                d = post[k_i] - pre[k_i]
                if d > 0: tips[TIPS[k]] = tips.get(TIPS[k], 0) + d
        cfg = m.get('transactionConfig') or {}
        cul = cfg.get('computeUnitLimit'); cup = None
        for ix in m['instructions']:
            if keys[ix['programIdIndex']] == CB:
                try:
                    d = b58decode(ix['data'])
                    if d[0] == 2: cul = struct.unpack_from('<I', d, 1)[0]
                    elif d[0] == 3: cup = struct.unpack_from('<Q', d, 1)[0]
                except Exception: pass
        row = {"i": i, "sig": tx['transaction']['signatures'][0], "signers": signers, "err": parse_err(meta['err']),
               "fee": meta['fee'], "nsig": nsig, "cu": meta.get('computeUnitsConsumed'), "cul": cul, "cup": cup,
               "tips": tips, "ixn": ixn[:3], "ev": evs, "v": tx.get('version'),
               "payer_pre": pre[0], "payer_d": post[0] - pre[0]}
        if meta['err'] is not None or not evs:
            row["wkeys"] = [k for j, k in enumerate(keys[:len(pre)]) if j >= nsig][:40]
        if meta['err'] is not None:
            e = meta['err']
            if isinstance(e, dict) and 'InstructionError' in e:
                try: row["err_prog"] = keys[m['instructions'][e['InstructionError'][0]]['programIdIndex']]
                except Exception: pass
            lm = meta.get('logMessages') or []
            row["elog"] = [l[:160] for l in lm if 'rror' in l or 'failed' in l][-4:]
        rows.append(row)
    return {"slot": slot, "leader": leader, "bt": blk.get('blockTime'), "parent": blk.get('parentSlot'),
            "n_tx": len(txs), "n_vote": n_vote, "pump": rows, "xv": 2}

def path(slot): return os.path.join(OUT, f"{slot}.json")

def get(slot):
    p = path(slot)
    if os.path.exists(p):
        try:
            r = json.load(open(p))
            if r.get('xv') == 2 or r.get('skipped'): return r
        except Exception: pass
    rec = extract(slot, rpc_block(slot))
    tmp = p + '.tmp'; json.dump(rec, open(tmp, 'w'), separators=(',', ':')); os.replace(tmp, p)
    return rec
