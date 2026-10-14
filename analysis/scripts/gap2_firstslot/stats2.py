import json, collections, math
from stats import q, sh, fail_class, per_launch
rows = [a for a in json.load(open('rows.json')) if a['solq']]
reuse = json.load(open('wallet_reuse.json'))
def fmt(v, s=1e9, d=5): return [round(x / s, d) for x in v] if v else None
W0 = []; L0 = []; W1 = []; L1 = []
for a in rows:
    for x in a['r0']:
        if not x['buy'] or x['owner_linked']: continue
        (W0 if x['ok'] else L0).append((a, x))
    for x in a['r1']:
        if not x['buy'] or x['owner_linked']: continue
        (W1 if x['ok'] else L1).append((a, x))
print('slot0 winners', len(W0), 'slot0 failed txs', len(L0))
print('bid SOL p10..p90 winners s0', fmt(q([x['bid'] for a, x in W0])))
print('  prio only winners s0', fmt(q([x['prio'] for a, x in W0])))
print('bid slippage-failed s0', fmt(q([x['bid'] for a, x in L0 if fail_class(x) == 'slippage'])))
print('bid botguard-failed s0', fmt(q([x['bid'] for a, x in L0 if fail_class(x) == 'botguard'])))
print('bid winners s1', fmt(q([x['bid'] for a, x in W1])))
# first winner per launch in slot0
first = []
for a in rows:
    ws = sorted([x for x in a['r0'] if x['buy'] and not x['owner_linked'] and x['ok']], key=lambda x: x['i'])
    if ws: first.append((a, ws[0]))
print('n launches w/ slot0 winner', len(first))
print('first winner gap from create (tx idx) p10..p90', q([x['gap'] for a, x in first]))
print('first winner gap from bundle end', q([x['gap_bundle'] for a, x in first]))
print('first winner gap as share of block', [round(v, 3) for v in q([x['gap'] / a['n_tx'] for a, x in first])])
print('first winner bid', fmt(q([x['bid'] for a, x in first])))
print('first winner cu', q([x['cu'] for a, x in first if x['cu']]), 'cul', q([x['cul'] for a, x in first if x['cul']]))
print('first winner tx version', collections.Counter(str(x['v']) for a, x in first))
tp = collections.Counter()
for a, x in W0:
    tp['+'.join(sorted(x['tips'])) or 'none'] += 1
print('slot0 winner tip paths', tp.most_common(12))
tpl = collections.Counter()
for a, x in L0:
    if fail_class(x) == 'slippage': tpl['+'.join(sorted(x['tips'])) or 'none'] += 1
print('slot0 slippage-loser tip paths', tpl.most_common(8))
# does higher bid win? within launch, pairs winner vs slippage loser in slot0
pairs = hi = 0
for a in rows:
    xs = [x for x in a['r0'] if x['buy'] and not x['owner_linked']]
    ws = [x for x in xs if x['ok']]; ls = [x for x in xs if fail_class(x) == 'slippage']
    for w in ws:
        for l in ls:
            pairs += 1; hi += w['bid'] > l['bid']
print('winner bid > slippage-loser bid (same launch, slot0):', sh(hi, pairs))
# earlier-index vs bid among winners in slot0
pairs = hi = 0
for a in rows:
    ws = sorted([x for x in a['r0'] if x['buy'] and not x['owner_linked'] and x['ok']], key=lambda x: x['i'])
    for i in range(len(ws)):
        for j in range(i + 1, len(ws)):
            pairs += 1; hi += ws[i]['bid'] > ws[j]['bid']
print('earlier slot0 winner had higher bid:', sh(hi, pairs))
# failure codes by program for slot0
fc = collections.Counter((fail_class(x), x['err'], (x.get('err_prog') or '')[:6]) for a, x in L0)
print('slot0 failure classes', fc.most_common(15))
