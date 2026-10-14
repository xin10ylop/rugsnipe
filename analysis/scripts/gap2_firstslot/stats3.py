import json, collections
from stats import q, sh, fail_class
rows = [a for a in json.load(open('rows.json'))]
def w0(a): return sum(1 for x in a['r0'] if x['buy'] and not x['owner_linked'] and x['ok']) > 0
def at0(a): return sum(1 for x in a['r0'] if x['buy'] and not x['owner_linked']) > 0
print('n', len(rows))
print('create position p10..p90', [round(v, 3) for v in q([a['pos'] for a in rows])])
for lo, hi in ((0, .25), (.25, .5), (.5, .75), (.75, .85), (.85, .95), (.95, 1.01)):
    G = [a for a in rows if lo <= a['pos'] < hi]
    print(f' pos [{lo},{hi}): n={len(G)} share={len(G)/len(rows):.3f} slot0 winner {sh(sum(w0(a) for a in G), len(G))} slot0 attempt {sh(sum(at0(a) for a in G), len(G))}')
lc = collections.Counter(a['lclass'] for a in rows)
print('leader classes', lc)
for c in lc:
    G = [a for a in rows if a['lclass'] == c]
    print(f' {c}: n={len(G)} slot0 winner {sh(sum(w0(a) for a in G), len(G))}; pos>=0.85 {sh(sum(a["pos"]>=.85 for a in G), len(G))}')
# per leader: leaders with >=15 creates and zero slot0 winners
byl = collections.defaultdict(list)
for a in rows: byl[a['leader']].append(a)
big = {l: G for l, G in byl.items() if len(G) >= 10}
zero = {l: G for l, G in big.items() if sum(w0(a) for a in G) == 0}
print('leaders with >=10 creates:', len(big), 'covering', sum(len(G) for G in big.values()), 'creates; with zero slot0 winners:', len(zero), 'covering', sum(len(G) for G in zero.values()))
rates = sorted([(sum(w0(a) for a in G) / len(G), len(G), l[:8], rows and G[0]['lclass']) for l, G in big.items()])
print('lowest', rates[:10]); print('highest', rates[-6:])
# expected zero-count under pooled rate
import math
p = sum(w0(a) for a in rows) / len(rows)
exp0 = sum((1 - p) ** len(G) for G in big.values())
print('pooled p', round(p, 3), 'expected #leaders with zero winners under homogeneity', round(exp0, 1))
print('pre-create touching txs: launches', sh(sum(1 for a in rows if a['pre_create_attempts'] > 0), len(rows)))
print('slot+1 skipped', sh(sum(1 for a in rows if a['b1_skipped']), len(rows)), 'slot+1 same leader', sh(sum(1 for a in rows if a['same_leader_s1']), len(rows)))
print('creates in jito bundle', sh(sum(a['jito_create'] for a in rows), len(rows)), 'jito bundle size', collections.Counter(a['jito_n'] for a in rows if a['jito_create']))
print('bundle_n_tx dist', collections.Counter(min(a['bundle_n_tx'], 5) for a in rows))
print('bundle detected via jito membership', sum(a['bundle_jito_n'] > 0 for a in rows))
print('create bid p10..p90 SOL', [round(v/1e9, 5) for v in q([a['create_bid'] for a in rows])])
