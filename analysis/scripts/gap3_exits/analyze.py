import json, sys, statistics as st, collections, math
sys.path.insert(0, '/home/user/rugsnipe/analysis')
from rugsnipe_analysis.curve import buy_tokens_out, sell_sol_out, reserves_from_mcap
SUP = 1e9; SPLIT = 454_896_000; POS_SOL = 0.5; HOR = 900
import glob
OPS = '--ops' in sys.argv
if OPS:
    sel = {}
    for f in glob.glob('ops_sel_*.json'):
        for s_ in json.load(open(f)): sel[s_['mint']] = s_
else:
    sel = {s['mint']: s for s in json.load(open('sel.json'))}
outc = {}
for l in open('/home/user/rugsnipe/data/cache/gap1/outcomes.jsonl'):
    o = json.loads(l); outc[o['mint']] = o
fund = {}
for l in open('/home/user/rugsnipe/data/cache/gap1/funding.jsonl'):
    f = json.loads(l); fund[f['sig']] = f
TR = {}
for l in open('ops_trades.jsonl' if OPS else 'trades.jsonl'):
    r = json.loads(l); TR[r['mint']] = r
XF = {}
try:
    for l in open('xfers.jsonl'):
        r = json.loads(l); XF[r['mint']] = r
except FileNotFoundError:
    pass
ONLY_XF = '--xf' in sys.argv

def q(xs, p):
    xs = sorted(xs)
    if not xs: return None
    k = (len(xs) - 1) * p; f = math.floor(k); c = min(f + 1, len(xs) - 1)
    return xs[f] + (xs[c] - xs[f]) * (k - f)

def wq(pairs, p):  # weighted quantile of (value, weight)
    pairs = sorted(pairs)
    if not pairs: return None
    tot = sum(w for _, w in pairs); acc = 0
    for v, w in pairs:
        acc += w
        if acc >= p * tot: return v
    return pairs[-1][0]

def val(mcap, qtok):
    # history state h (without us); actual state = h with our q tokens still outstanding;
    # selling q returns the curve to h. Proceeds = vsol(h - q) - vsol(h), minus 1.25% fee.
    vs, vt = reserves_from_mcap(mcap)
    k = vs * vt; qr = qtok * 1e6
    vta = max(vt - qr, vt * 0.05)
    return (k / vta - vs) * (1 - 0.0125) / 1e9

def classify_xf(xs):
    out = collections.Counter()
    for x in xs:
        neg = [k for k, v in x['d'].items() if v < 0]; pos = [k for k, v in x['d'].items() if v > 0]
        ix = ' '.join(x['ix'])
        amt = sum(v for v in x['d'].values() if v > 0)
        if not pos: out['burn/close'] += 1; continue
        if amt < 0.001 * SUP: out['dust(<0.1%)'] += 1; continue
        if 'strmRq' in ix: out['streamflow lock'] += 1
        elif len(pos) >= 3 and len(neg) == 1: out['distribution 1->many'] += 1
        elif len(neg) >= 2 and len(pos) == 1: out['consolidation many->1'] += 1
        elif 'FLASHX' in ix: out['FLASHX bot 1->1'] += 1
        elif 'advanceNonce' in ix: out['nonce-tx 1->1'] += 1
        else: out['other 1->1'] += 1
    return dict(out)

# ---------- pass 1: raw bundles ----------
def raw_bundle(s, tr):
    S0 = s['slot']; csig = s['sig']
    C = {s['creator'], s['user'], s['fee_payer'], *s.get('signers', [])}
    s0 = [t for t in tr if t[0] == S0]
    ctx = next((t[1] for t in s0 if t[9] == csig), None)
    if ctx is None: return None
    idx = set()
    for i, t in enumerate(tr):
        if t[0] != S0: break
        if t[5] and (t[9] == csig or t[4] in C): idx.add(i)
    prev = ctx
    for i, t in enumerate(tr):
        if t[0] != S0 or t[1] <= ctx: continue
        if t[1] - prev <= 2 and t[1] - ctx <= 5 and t[5]:
            idx.add(i); prev = t[1]
        elif t[1] - prev > 2 or t[1] - ctx > 5:
            break
    return C, idx, ctx

raw = {}
wall_launch = collections.defaultdict(set)
for m, s in sel.items():
    r = TR.get(m)
    if not r or not r['trades']: continue
    tr = r['trades']
    rb = raw_bundle(s, tr)
    if not rb: continue
    raw[m] = (tr, rb)
    C, idx, ctx = rb
    for i in idx:
        u = tr[i][4]
        if u not in C: wall_launch[u].add(s['creator'])
PRO_CAND = {w for w, cs in wall_launch.items() if len(cs) >= 4}
PRO = set()  # inspected: all 10 candidates are reused bundle wallets (fixed amount, fixed +1..+3 offset), not snipers

# ---------- operators (union-find) ----------
par = {}
def find(x):
    par.setdefault(x, x)
    while par[x] != x:
        par[x] = par[par[x]]; x = par[x]
    return x
def union(a, b):
    ra, rb_ = find(a), find(b)
    if ra != rb_: par[ra] = rb_

# ---------- pass 2: per-launch analysis ----------
L = []; GRAD = []
for m, (tr, (C, idx, ctx)) in raw.items():
    s = sel[m]; S0 = s['slot']; bt = s['bt']
    idx = {i for i in idx if tr[i][4] in C or tr[i][4] not in PRO}
    bt_ = [tr[i] for i in sorted(idx)]
    B = {t[4] for t in bt_ if t[4] not in C}
    dev_sol = sum(t[6] for t in bt_ if t[4] in C)
    per_w = collections.defaultdict(float); per_wtok = collections.defaultdict(float)
    for t in bt_:
        if t[4] not in C: per_w[t[4]] += t[6]; per_wtok[t[4]] += t[7]
    bsol = sum(per_w.values())
    tok_all = sum(t[7] for t in bt_); tok_ex = sum(per_wtok.values())
    s0all = [t for t in tr if t[0] == S0]
    if any(t[10] != 'pump' for t in s0all) or dev_sol + bsol >= 80 or sum(t[7] for t in s0all if t[5]) >= 0.79e9:
        GRAD.append(m); continue
    if not B and dev_sol < 1.0: continue
    kind = 'multi' if B else 'dev_only'
    pb = max(idx)
    # fill mcap (carry forward zeros)
    last = 27.959
    mc = []
    for t in tr:
        if t[8] and t[8] > 0: last = t[8]
        mc.append(last)
    me = mc[pb]
    o = outc.get(m, {})
    solusd = o.get('sol_usd') or 107.0
    s0_buyers = {t[4] for t in tr if t[0] == S0 and t[5] and t[6] * solusd >= 5}
    s0_third = {t[4] for t in tr if t[0] == S0 and t[5] and t[4] not in C and t[4] not in B}
    # operator links
    node = 'L:' + m; find(node)
    union(node, 'W:' + s['user'])
    f = fund.get(s['sig'])
    if f and f.get('chain'):
        h = f['chain'][0]
        if not h.get('cat') and not h.get('via') and h.get('status') == 'ok' and h.get('n_pre', 0) < 1000:
            union(node, 'F:' + h['addr'])
    for w in B: union(node, 'B:' + w)
    vs, vt = reserves_from_mcap(me)
    qtok = buy_tokens_out(int(POS_SOL * 1e9), int(vs), int(vt))[0] / 1e6
    # horizon
    W0 = TR[m].get('W0', 1800); trn = TR[m]['trunc']
    hor = min(HOR, W0 if not trn else (300 if trn == [W0] else 60))
    hend = len(tr)
    for i, t in enumerate(tr):
        if t[3] > bt + hor: hend = i; break
    after = list(range(pb + 1, hend))
    ms_slot = 217 if S0 >= SPLIT else 400
    # holdings via trades
    hold = collections.defaultdict(float)
    for i in range(0, pb + 1):
        t = tr[i]; hold[t[4]] += t[7] if t[5] else -t[7]
    # transfers
    xf = XF.get(m)
    xs = []
    if xf:
        for x in xf['xs']:
            if x['pump'] or not x['d']: continue
            xs.append(x)
    recv_slot = {}  # owner -> list of (slot, amount received)
    cons_events = []  # (slot, recipient, cumulative received)
    cum = collections.defaultdict(float)
    for x in sorted(xs, key=lambda x: x['slot']):
        for own, d in x['d'].items():
            if d > 0:
                cum[own] += d
                recv_slot.setdefault(own, []).append((x['slot'], d))
                cons_events.append((x['slot'], own, cum[own], d, [k for k, v in x['d'].items() if v < 0]))
    # walk
    M = me; tp = d50 = None
    for i in after:
        if val(mc[i], qtok) >= 2 * POS_SOL and tp is None: tp = i
        M = max(M, mc[i])
        if d50 is None and mc[i] <= 0.5 * M: d50 = i
        if tp is not None and d50 is not None: break
    status = 'hit' if (tp is not None and (d50 is None or tp < d50)) else ('miss' if d50 is not None else 'open')
    horizon_val = val(mc[hend - 1], qtok) if hend - 1 > pb else val(me, qtok)
    rec = dict(m=m, w=s['w'], kind=kind, nB=len(B), dev_sol=dev_sol, bsol=bsol, per_w=list(per_w.values()),
               share_all=tok_all / SUP, share_ex=tok_ex / SUP, me=me, me_usd=me * solusd, solusd=solusd,
               s0_buyers=len(s0_buyers), s0_third=len(s0_third), status=status, S0=S0, bt=bt, ms_slot=ms_slot,
               qtok=qtok, horizon_val=horizon_val, hor=hor, floor_ret=val(27.959, qtok) / POS_SOL - 1, tp_val=(val(mc[tp], qtok) / POS_SOL - 1) if tp is not None else None, xf=bool(xf), n_xs=len(xs), complete=TR[m]['complete'])
    # ---- dump characterisation
    if d50 is not None:
        sd = tr[d50][0]
        ins = [i for i in range(pb + 1, hend) if tr[i][0] == sd]
        pre_i = ins[0] - 1
        pre = mc[pre_i]; end = mc[ins[-1]]
        sells = [tr[i] for i in ins if not tr[i][5]]
        vol = collections.Counter(); sellers = collections.Counter()
        recv_before = lambda u: sum(a for sl, a in recv_slot.get(u, []) if sl <= sd)
        # bought-vs-sold inference (no RPC): cumulative tokens bought by trades before this slot
        hb = collections.defaultdict(float)
        for i in range(0, ins[0]):
            t = tr[i]; hb[t[4]] += t[7] if t[5] else -t[7]
        sells = []
        for i in ins:
            t = tr[i]
            if t[5]: hb[t[4]] += t[7]; continue
            sells.append((t, hb[t[4]])); hb[t[4]] -= t[7]
        XR = rec.setdefault('xr_examples', [])
        for t, hbu in sells:
            u = t[4]
            if u in C: cat = 'creator'
            elif u in B: cat = 'bundle'
            elif recv_before(u) > 0 or t[7] > hbu * 1.02 + 1:
                cat = 'xfer_recv'; XR.append((u, t[9], t[7], hbu))
            else: cat = 'other'
            vol[cat] += t[6]; sellers[cat] += 1
        tot = sum(vol.values()) or 1e-9
        top = max((t for t, _ in sells), key=lambda t: t[6]) if sells else None
        top_x = None
        if top is not None:
            u = top[4]; rv = recv_slot.get(u, [])
            if rv:
                top_x = sd - max(sl for sl, a in rv if sl <= sd) if any(sl <= sd for sl, a in rv) else None
        nsel = len({t[4] for t, _ in sells})
        if vol['creator'] / tot >= 0.5: mech = 'creator/signer sell'
        elif vol['xfer_recv'] / tot >= 0.5: mech = 'consolidated-wallet sell'
        elif vol['bundle'] / tot >= 0.5: mech = 'bundle same-slot (>=2 wallets)' if sellers['bundle'] >= 2 else 'bundle single wallet'
        elif nsel >= 3: mech = 'many unattributed wallets same slot'
        else: mech = 'other (1-2 unattributed sellers)'
        drop = 1 - end / pre if pre else 0
        if drop < 0.3: mech_g = 'gradual (in-slot drop <30%)'
        else: mech_g = mech
        jj = d50
        for k2 in range(d50, len(tr)):
            if tr[k2][0] <= sd + 1: jj = k2
            else: break
        rec['d50_exit_e1'] = val(mc[jj], qtok) / POS_SOL - 1
        rec.update(d50_slots=sd - S0, d50_s=tr[d50][3] - bt, d50_ms=(sd - S0) * ms_slot, inslot_drop=drop,
                   mech=mech_g, mech_raw=mech, n_sellers=nsel, vol=dict(vol), top_x_gap=top_x,
                   d50_vs_entry=mc[d50] / me, peak_vs_entry=M / me)
    # ---- signals
    Bset = B
    def first(pred):
        for i in after:
            if tp is not None and i >= tp: return None
            if pred(tr[i]): return ('t', i, tr[i][0])
        return None
    sig = {}
    sig['bundle_sell_any'] = first(lambda t: not t[5] and t[4] in Bset)
    sig['creator_sell'] = first(lambda t: not t[5] and t[4] in C)
    sig['bundle_or_creator_sell'] = first(lambda t: not t[5] and (t[4] in Bset or t[4] in C))
    sig['big_sell_2pct'] = first(lambda t: not t[5] and t[7] >= 0.02 * SUP)
    btok_ = sum(per_wtok.values()); itok_ = tok_all
    for nm, S_, tot_ in (('bundle_dump50', Bset, btok_), ('insider_dump50', Bset | C, itok_)):
        acc = 0.0; got = None
        if tot_ > 0:
            for i in after:
                if tp is not None and i >= tp: break
                t = tr[i]
                if not t[5] and t[4] in S_:
                    acc += t[7]
                    if acc >= 0.5 * tot_: got = ('t', i, t[0]); break
        sig[nm] = got
    # trailing price stops
    for x in (0.2, 0.3):
        MM = me; got = None
        for i in after:
            if tp is not None and i >= tp: break
            MM = max(MM, mc[i])
            if mc[i] <= (1 - x) * MM: got = ('t', i, tr[i][0]); break
        sig[f'trail_{int(x*100)}'] = got
    # consolidation (needs xfers)
    if xf:
        senders = collections.defaultdict(set)
        for sl, own, c, d, frm in cons_events:
            senders[own].update(frm)
        rec['xf_types'] = classify_xf(xs)
        for X in (0.01, 0.02, 0.05, 0.10, 'multi1'):
            got = None
            snd = collections.defaultdict(set)
            for sl, own, c, d, frm in cons_events:
                snd[own].update(frm)
                if (X == 'multi1' and c >= 0.01 * SUP and len(snd[own]) >= 2) or (X != 'multi1' and c >= X * SUP):
                    got = ('x', sl, own); break
            if got:
                # position = after last trade in slot <= sl
                j = max([i for i in range(pb, hend) if tr[i][0] <= got[1]], default=pb)
                if tp is not None and tr[tp][0] <= got[1]: got = None
                else: got = ('x', j, got[1])
            sig[f'consol_{X}' if X == 'multi1' else f'consol_{int(X*100)}pct'] = got
    # idle timers (seconds -> slots)
    for T in (3, 5, 10, 20):
        N = math.ceil(T * 1000 / ms_slot); got = None
        prev = pb
        for i in after + [None]:
            nxt_slot = tr[i][0] if i is not None else (tr[hend - 1][0] + 10**9 if hend < len(tr) or True else None)
            if i is not None and tp is not None and i >= tp: break
            if nxt_slot - tr[prev][0] >= N:
                got = ('i', prev, tr[prev][0] + N); break
            if i is None: break
            prev = i
        if got and i is None and hend >= len(tr) and not TR[m]['complete']: got = None
        sig[f'idle_{T}s'] = got
    sres = {}
    for k, g in sig.items():
        if g is None: sres[k] = None; continue
        typ, i, fslot = g
        # exit fills: E0 = end of fire slot; E1 = end of fire slot + 1 (all other sells in those slots land first)
        def end_of(slotv):
            j = i
            for jj in range(i, len(tr)):
                if tr[jj][0] <= slotv: j = jj
                else: break
            return j
        e0 = val(mc[end_of(fslot)], qtok) / POS_SOL - 1
        e1 = val(mc[end_of(fslot + 1)], qtok) / POS_SOL - 1
        before_d50 = d50 is None or fslot < tr[d50][0]
        same_slot = d50 is not None and fslot == tr[d50][0]
        lead_sl = (tr[d50][0] - fslot) if d50 is not None else None
        lead_s = (tr[d50][3] - (tr[i][3] if typ != 'i' else tr[i][3] + (fslot - tr[i][0]) * ms_slot / 1000)) if d50 is not None else None
        d50_within60 = d50 is not None and lead_s is not None and 0 <= lead_s <= 60
        sres[k] = dict(fslot=fslot - S0, e0=e0, e1=e1, before_d50=before_d50, same_slot=same_slot, lead_sl=lead_sl, lead_s=lead_s, w60=d50_within60)
    rec['sig'] = sres
    # bundle / creator exit timing (independent of TP)
    btok = sum(per_wtok.values()); ctok = sum(t[7] for t in bt_ if t[4] in C)
    bs = [tr[i] for i in after if not tr[i][5] and tr[i][4] in Bset]
    cs = [tr[i] for i in after if not tr[i][5] and tr[i][4] in C]
    rec['b_first_sell_slots'] = (bs[0][0] - S0) if bs else None
    rec['b_first_sell_s'] = (bs[0][3] - bt) if bs else None
    rec['b_first_sell_frac'] = (sum(t[7] for t in bs if t[0] == bs[0][0]) / btok) if bs and btok else None
    rec['b_sold_frac'] = min(1, sum(t[7] for t in bs) / btok) if btok else None
    rec['b_sold_frac_d50'] = (min(1, sum(t[7] for t in bs if d50 is not None and (t[0], t[1]) <= (tr[d50][0], tr[d50][1])) / btok) if btok and d50 is not None else None)
    rec['b_nwallets_first_slot'] = len({t[4] for t in bs if t[0] == bs[0][0]}) if bs else None
    rec['c_first_sell_slots'] = (cs[0][0] - S0) if cs else None
    rec['c_first_sell_s'] = (cs[0][3] - bt) if cs else None
    rec['c_sold_frac'] = min(1, sum(t[7] for t in cs) / ctok) if ctok else None
    rec['tp_i'] = tp; rec['d50_i'] = d50
    L.append(rec)

for r in L: r['op'] = sel[r['m']].get('op') or find('L:' + r['m'])
json.dump({'L': L, 'PRO': sorted(PRO), 'GRAD': GRAD, 'GRADW': [sel[m]['w'] for m in GRAD], 'NRAW': len(raw), 'NRAWW': sum(sel[m]['w'] for m in raw)}, open('launches_ops.json' if OPS else 'launches.json', 'w'))
print('slot0 graduations excluded', len(GRAD))
print('launches', len(L), 'pro wallets removed', len(PRO), 'with xfers', sum(r['xf'] for r in L))
