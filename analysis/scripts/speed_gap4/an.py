import json,collections,statistics as st
rows=[json.loads(l) for l in open('scan/rows.jsonl')]
slots=sorted({r['slot'] for r in rows}); print('blocks',len(slots),'slots',slots[0],'-',slots[-1],'nonvote txs',len(rows))
V=collections.Counter(r['v'] for r in rows); print('all nonvote by version',V)
P=collections.Counter(r['v'] for r in rows if r['pump']); print('pump by version',P, 'v1 share %.1f%%'%(100*P['1']/sum(P.values())))
prov=collections.defaultdict(collections.Counter)
for r in rows:
    ps=r['provs'] or ['none']
    for p in ps:
        c=prov[p]; c['n']+=1; c['v'+r['v']]+=1
        if r['nonce']: c['nonce_'+r['v']]+=1
        if r['err']: c['err_'+r['v']]+=1
        if r['pump']: c['pump_'+r['v']]+=1
        if r['pump'] and r['nonce']: c['pump_nonce_'+r['v']]+=1
        if r['provs_alt']: c['tip_in_alt']+=1
print(f"{'prov':12s} {'n':>6s} {'leg':>5s} {'v0':>5s} {'v1':>5s} | {'nonce leg/v0/v1':>16s} | {'pump leg/v0/v1':>15s} | {'pump+nonce v1':>6s} | err v1")
for p,c in sorted(prov.items(),key=lambda x:-x[1]['n']):
    print(f"{p:12s} {c['n']:6d} {c['vlegacy']:5d} {c['v0']:5d} {c['v1']:5d} | {c['nonce_legacy']:4d}/{c['nonce_0']:4d}/{c['nonce_1']:4d}     | {c['pump_legacy']:4d}/{c['pump_0']:4d}/{c['pump_1']:4d} | {c['pump_nonce_1']:5d} | {c['err_1']}")
multi=collections.Counter(tuple(r['provs']) for r in rows if len(r['provs'])>1); print('multi-provider tips',multi.most_common(8))
