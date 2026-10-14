"""Per-create first-slot competition analysis from compact block records."""
import json, os
from collections import defaultdict
import blocks
SOLQ = "11111111111111111111111111111111"
SLIP = {"C6002", "C6042", "C6003"}
BUYIX = ('buy',)

def load_block(slot):
    p = blocks.path(slot)
    return json.load(open(p)) if os.path.exists(p) else None

def bid(r):
    return (r['fee'] - 5000 * r['nsig']) + sum(r['tips'].values())

def touches(r, mint, bc):
    if any(e.get('mint') == mint for e in r['ev']): return True
    wk = r.get('wkeys') or []
    return mint in wk or bc in wk

def is_buy_attempt(r, mint):
    evs = [e for e in r['ev'] if e.get('mint') == mint]
    if evs: return any(e['t'] == 'b' for e in evs) and not any(e['t'] == 'c' for e in evs)
    names = ' '.join(r['ixn'])
    return ('buy' in names) or (r['err'] is not None and 'sell' not in names)

def analyze(smp_row, bundle_sigs=None):
    s0 = smp_row['slot']; sig = smp_row['sig']
    B0 = load_block(s0); B1 = load_block(s0 + 1)
    if not B0 or B0.get('skipped'): return None
    cr = next((r for r in B0['pump'] if r['sig'] == sig), None)
    if cr is None: return {"sig": sig, "status": "create_not_found"}
    ce = next((e for e in cr['ev'] if e['t'] == 'c'), None)
    if ce is None: return {"sig": sig, "status": "no_create_event", "err": cr['err']}
    mint, bc, creator, user = ce['mint'], ce['bc'], ce['creator'], ce['user']
    ci = cr['i']
    out = {"sig": sig, "status": "ok", "slot": s0, "mint": mint, "creator": creator, "user": user,
           "signer": cr['signers'][0], "mayhem": ce['mayhem'], "solq": ce['quote'] in (None, SOLQ),
           "leader": B0['leader'], "n_tx": B0['n_tx'], "n_vote": B0['n_vote'], "ci": ci,
           "pos": ci / max(B0['n_tx'], 1), "bt": B0['bt'], "create_bid": bid(cr), "create_tips": cr['tips'],
           "create_ixn": cr['ixn'], "name": ce.get('name')}
    # pump rows for this mint in the create tx
    dev_trades = [e for e in cr['ev'] if e['t'] in 'bs' and e['mint'] == mint]
    out['dev_buy_sol'] = sum(e['sol'] for e in dev_trades if e['t'] == 'b') / 1e9
    out['create_tx_buyers'] = sorted({e['user'] for e in dev_trades if e['t'] == 'b'})
    owners = {creator, user, *cr['signers']} | set(out['create_tx_buyers'])
    # rows in slot0 touching mint
    rows0 = [r for r in B0['pump'] if r['sig'] != sig and touches(r, mint, bc)]
    out['pre_create_attempts'] = sum(1 for r in rows0 if r['i'] < ci)
    rows0 = [r for r in rows0 if r['i'] > ci]
    rows1 = [r for r in (B1['pump'] if B1 and not B1.get('skipped') else []) if touches(r, mint, bc)]
    out['b1_skipped'] = (not B1) or bool(B1.get('skipped'))
    out['b1_leader'] = B1.get('leader') if B1 else None
    out['b1_n_tx'] = B1.get('n_tx') if B1 else None
    # creator bundle: Jito bundle members + contiguous adjacent run after create + shared signers
    bset = set(bundle_sigs or [])
    bundle_rows = []
    nxt = ci + 1
    for r in rows0:
        in_jito = r['sig'] in bset
        adjacent = (r['i'] == nxt and r['i'] - ci <= 4)
        shared = bool(owners & set(r['signers']))
        if in_jito or adjacent or shared:
            bundle_rows.append(r)
            if adjacent: nxt += 1
            owners |= set(r['signers'])
            for e in r['ev']:
                if e.get('mint') == mint and e['t'] == 'b': owners.add(e['user'])
    bids = {id(r) for r in bundle_rows}
    out['bundle_n_tx'] = len(bundle_rows)
    out['bundle_jito_n'] = sum(1 for r in bundle_rows if r['sig'] in bset)
    out['bundle_wallets'] = sorted({e['user'] for r in bundle_rows for e in r['ev'] if e.get('mint') == mint and e['t'] == 'b'})
    out['bundle_sol'] = sum(e['sol'] for r in bundle_rows for e in r['ev'] if e.get('mint') == mint and e['t'] == 'b') / 1e9
    # post-bundle state: last successful event of mint among create tx + bundle rows (ordered by index)
    state_rows = [cr] + sorted(bundle_rows, key=lambda r: r['i'])
    st = None
    for r in state_rows:
        for e in r['ev']:
            if e.get('mint') == mint and 'vsol' in e and e.get('vsol'):
                st = (e['vsol'], e['vtok'])
    out['post_bundle_vsol'], out['post_bundle_vtok'] = st if st else (None, None)
    pb_price = st[0] / st[1] if st else None
    out['bundle_end_i'] = max([r['i'] for r in bundle_rows] + [ci])

    def reactive(rows, slot_off):
        res = []
        for r in sorted(rows, key=lambda r: r['i']):
            if id(r) in bids: continue
            evs = [e for e in r['ev'] if e.get('mint') == mint]
            buy = is_buy_attempt(r, mint)
            sells = [e for e in evs if e['t'] == 's']
            shared = bool(owners & set(r['signers'])) or any(e['user'] in owners for e in evs)
            rec = {"slot_off": slot_off, "i": r['i'], "gap": (r['i'] - ci) if slot_off == 0 else None,
                   "gap_bundle": (r['i'] - out['bundle_end_i']) if slot_off == 0 else None,
                   "signer": r['signers'][0], "ok": r['err'] is None, "err": r['err'], "buy": buy,
                   "sell": bool(sells), "owner_linked": shared, "bid": bid(r), "prio": r['fee'] - 5000 * r['nsig'],
                   "tips": r['tips'], "cu": r['cu'], "cul": r['cul'], "ixn": r['ixn'], "v": r.get('v'),
                   "payer_pre": r.get('payer_pre'), "err_prog": r.get('err_prog'), "nsig": r['nsig']}
            b = [e for e in evs if e['t'] == 'b']
            if b:
                e = b[0]
                rec['user'] = e['user']; rec['sol'] = sum(x['sol'] for x in b) / 1e9; rec['tok'] = sum(x['tok'] for x in b)
                vb_sol = e['vsol'] - e['sol']; vb_tok = e['vtok'] + e['tok']
                if pb_price and e['tok']:
                    rec['prem_start'] = (vb_sol / vb_tok) / pb_price - 1
                    rec['prem_avg'] = (e['sol'] / e['tok']) / pb_price - 1
                    rec['prem_end'] = (e['vsol'] / e['vtok']) / pb_price - 1
            if sells:
                rec['sell_sol'] = sum(x['sol'] for x in sells) / 1e9
                rec['sell_user'] = sells[0]['user']
                rec['sell_by_owner'] = any(x['user'] in owners for x in sells)
            res.append(rec)
        return res
    out['r0'] = reactive(rows0, 0)
    out['r1'] = reactive(rows1, 1)
    return out
