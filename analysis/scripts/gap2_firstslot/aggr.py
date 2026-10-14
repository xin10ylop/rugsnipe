import json, math, statistics, collections, sys
import core
smp = json.load(open('sample.json'))['sample']
jito = {}
for l in open('jito.jsonl'):
    r = json.loads(l); jito[r['sig']] = r
labs = {}
for l in open('labels.jsonl'):
    r = json.loads(l); labs[r['sig']] = r
bam = {v['validator_pubkey']: v for v in json.load(open('../bam_validators.json'))}
sw = {v['identity']: v for v in json.load(open('../stakewiz.json'))}
SLIP = {"C6002", "C6042", "C6003"}

def leader_class(l):
    v = sw.get(l, {}); fd = str(v.get('version', '')).startswith('26.')
    return ('BAM' if l in bam else 'nonBAM') + ('-FD' if fd else '-Agave')

rows = []
for s in smp:
    jb = jito.get(s['sig'], {})
    a = core.analyze(s, bundle_sigs=jb.get('txs'))
    if not a or a.get('status') != 'ok': continue
    lb = labs.get(s['sig'], {})
    pr = max((lb.get('api') or {}).get('prior', 0) or 0, (lb.get('api_signer') or {}).get('prior', 0) or 0)
    f = lb.get('fund') or {}
    cexf = False
    labcat = (f.get('label') or [None, None])[1]; mlabcat = (f.get('main_label') or [None, None])[1]
    if (labcat == 'cex' or mlabcat == 'cex') and f.get('first_sig_time') and f['first_sig_time'] >= a['bt'] - 72 * 3600:
        cexf = True
    a['labeled'] = bool(lb.get('api'))
    a['prior'] = pr; a['repeat'] = pr >= 3; a['farm'] = pr >= 50; a['cex_fresh'] = cexf
    a['tracked'] = a['repeat'] or cexf
    a['jito_create'] = bool(jb.get('bundle_id')); a['jito_n'] = len(jb.get('txs') or []); a['jito_tip'] = jb.get('tip')
    a['bundled'] = a['bundle_sol'] >= 1 or len(set(a['bundle_wallets']) - {a['creator'], a['signer']}) >= 2
    a['lclass'] = leader_class(a['leader'])
    a['same_leader_s1'] = (a['b1_leader'] == a['leader']) if a['b1_leader'] else None
    rows.append(a)
json.dump(rows, open('rows.json', 'w'))
print('rows', len(rows), 'labeled', sum(r['labeled'] for r in rows))
