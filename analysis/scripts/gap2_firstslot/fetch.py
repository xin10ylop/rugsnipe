import json, sys, time
from concurrent.futures import ThreadPoolExecutor, as_completed
import blocks
smp = json.load(open('sample.json'))['sample']
slots = sorted({s['slot'] for s in smp} | {s['slot'] + 1 for s in smp}, reverse=True)
W = int(sys.argv[1]) if len(sys.argv) > 1 else 8
print(len(slots), 'slots', flush=True)
t0 = time.time(); done = 0; fails = 0
with ThreadPoolExecutor(W) as ex:
    futs = {ex.submit(blocks.get, s): s for s in slots}
    for f in as_completed(futs):
        try: f.result()
        except Exception as e: fails += 1; print('FAIL', futs[f], e, flush=True)
        done += 1
        if done % 200 == 0: print(done, round(time.time() - t0), 'fails', fails, flush=True)
print('done', done, 'fails', fails, round(time.time() - t0))
