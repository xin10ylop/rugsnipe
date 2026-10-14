import json, random
idx = json.load(open('/home/user/rugsnipe/data/cache/create_index.json'))
A = idx['anchor_time']
sig = [s for s in idx['sigs'] if not s['err'] and A - 86400 - 600 <= s['bt'] <= A - 600]
print(len(sig), min(s['slot'] for s in sig), max(s['slot'] for s in sig))
random.seed(20261014)
smp = random.sample(sig, 3600)
json.dump({"window": [A - 86400 - 600, A - 600], "population": len(sig), "sample": smp}, open('sample.json', 'w'))
