import json, math, collections, statistics, sys
rows = json.load(open('rows.json'))
SLIPC = {"C6002", "C6042", "C6003"}
PUMP = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
indep_excl = set(json.load(open('linked_wallets.json'))) if len(sys.argv) > 1 else set()

def q(xs, ps=(0.1, 0.25, 0.5, 0.75, 0.9)):
    xs = sorted(xs)
    if not xs: return None
    out = []
    for p in ps:
        k = (len(xs) - 1) * p; lo, hi = math.floor(k), math.ceil(k)
        out.append(xs[lo] + (xs[hi] - xs[lo]) * (k - lo))
    return out
def sh(k, n): 
    if not n: return 'n/a'
    p = k / n; se = math.sqrt(p * (1 - p) / n); return f"{100*p:.1f}% ({k}/{n}, ±{196*se:.1f})"

def fail_class(x):
    if x['ok']: return 'ok'
    e = x['err'] or ''
    if e in SLIPC: return 'slippage'
    if e == 'C0' and (x.get('err_prog') or '').startswith('1111'): return 'dup'
    if x.get('err_prog') and x['err_prog'] != PUMP and e.startswith('C'): return 'botguard'
    return 'other'

def per_launch(a):
    xs = [x for x in a['r0'] + a['r1'] if x['buy'] and not x['owner_linked'] and x['signer'] not in indep_excl
          and x.get('user', x['signer']) not in indep_excl]
    by = collections.defaultdict(list)
    for x in xs: by[(x['slot_off'], x['signer'])].append(x)
    res = {}
    for so in (0, 1):
        sig = {k[1]: v for k, v in by.items() if k[0] == so}
        res[f's{so}_att'] = len(sig)
        res[f's{so}_win'] = sum(1 for v in sig.values() if any(x['ok'] for x in v))
        res[f's{so}_fail_slip'] = sum(1 for v in sig.values() if not any(x['ok'] for x in v) and any(fail_class(x) == 'slippage' for x in v))
        res[f's{so}_fail_bot'] = sum(1 for v in sig.values() if not any(x['ok'] for x in v) and all(fail_class(x) in ('botguard','dup') for x in v))
        res[f's{so}_fail_other'] = res[f's{so}_att'] - res[f's{so}_win'] - res[f's{so}_fail_slip'] - res[f's{so}_fail_bot']
    succ = sorted([x for x in xs if x['ok'] and 'prem_avg' in x], key=lambda x: (x['slot_off'], x['i']))
    seen = set(); ordered = []
    for x in succ:
        if x['signer'] in seen: continue
        seen.add(x['signer']); ordered.append(x)
    res['ordered'] = ordered
    res['xs'] = xs
    return res

def group(name, G):
    n = len(G)
    if not n: return
    P = [per_launch(a) for a in G]
    print(f"\n=== {name}: n={n}")
    for so in (0, 1):
        w = [p[f's{so}_win'] for p in P]; at = [p[f's{so}_att'] for p in P]
        print(f" slot+{so}: launches with >=1 indep. winner {sh(sum(1 for x in w if x>=1), n)}, >=3 {sh(sum(1 for x in w if x>=3), n)}, >=5 {sh(sum(1 for x in w if x>=5),n)}; mean winners {statistics.fmean(w):.2f}, mean attempting wallets {statistics.fmean(at):.2f}; >=1 attempt {sh(sum(1 for x in at if x>=1), n)}")
        hist = collections.Counter(min(x, 6) for x in w); print('   winners hist', dict(sorted(hist.items())))
        A = sum(at); W = sum(w); S = sum(p[f's{so}_fail_slip'] for p in P); B = sum(p[f's{so}_fail_bot'] for p in P); O = sum(p[f's{so}_fail_other'] for p in P)
        print(f"   wallet-attempts {A}: win {sh(W,A)}, fail-slippage {sh(S,A)}, fail-botguard/dup-only {sh(B,A)}, other {sh(O,A)}")
    both = [p['s0_win'] + p['s1_win'] for p in P]
    print(f" slot0+1 combined: >=1 winner {sh(sum(1 for x in both if x>=1), n)}, >=3 {sh(sum(1 for x in both if x>=3), n)}, >=5 {sh(sum(1 for x in both if x>=5), n)}")
    for k in (1, 3, 5):
        pa = [p['ordered'][k-1]['prem_avg'] for p in P if len(p['ordered']) >= k]
        ps = [p['ordered'][k-1]['prem_start'] for p in P if len(p['ordered']) >= k]
        so = collections.Counter(p['ordered'][k-1]['slot_off'] for p in P if len(p['ordered']) >= k)
        if pa:
            print(f" reactive #{k}: n={len(pa)} slot={dict(so)} prem_avg p10/25/50/75/90 = {[round(100*v,1) for v in q(pa)]}%  prem_start = {[round(100*v,1) for v in q(ps)]}%")
    return P

if __name__ == '__main__':
    ok = [a for a in rows]
    solq = [a for a in ok if a['solq']]
    group('ALL creates', ok)
    group('SOL-quoted', solq)
    lab = [a for a in solq if a['labeled']]
    group('labeled SOL-quoted', lab)
    group('TRACKED (repeat>=3 or CEX-fresh)', [a for a in lab if a['tracked']])
    group('  repeat 3-49', [a for a in lab if a['repeat'] and not a['farm']])
    group('  repeat >=50 (farm)', [a for a in lab if a['farm']])
    group('  CEX-fresh', [a for a in lab if a['cex_fresh']])
    group('UNTRACKED', [a for a in lab if not a['tracked']])
    group('BUNDLED (>=1 SOL or >=2 bundle wallets)', [a for a in solq if a['bundled']])
    group('BUNDLED & TRACKED', [a for a in lab if a['bundled'] and a['tracked']])
    group('BUNDLED & untracked', [a for a in lab if a['bundled'] and not a['tracked']])
