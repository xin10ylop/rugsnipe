# Are the candidates still trading? Each wallet's newest signatures (up to 2,000): how many since Oct 25 18:00 UTC, the
# newest one's time, and the share that failed.
import sys, json, time, datetime
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month")
import tri
SINCE = 1792951200   # Oct 25 18:00 UTC
C = json.load(open(SP + "/wallets/cand1.json"))
def one(w):
    out = []; before = None
    for _ in range(2):
        s = tri.sigs(w, before=before)
        out += s
        if len(s) < 1000 or (s[-1].get("blockTime") or 0) < SINCE: break
        before = s[-1]["signature"]
    rec = [x for x in out if (x.get("blockTime") or 0) >= SINCE]
    newest = max((x.get("blockTime") or 0) for x in out) if out else 0
    return w, dict(n_since=len(rec), capped=len(rec) >= 1999, newest=newest, fail=sum(1 for x in rec if x.get("err")) / max(len(rec), 1))
with ThreadPoolExecutor(8) as ex: R = dict(ex.map(one, C))
json.dump(R, open(SP + "/wallets/active.json", "w"))
now = time.time()
for w, r in sorted(R.items(), key=lambda kv: -kv[1]["newest"]):
    print(f"{w[:8]} since Oct 25 18:00: {r['n_since']:5d}{'+' if r['capped'] else ' '} failed {r['fail']:4.0%}  newest {(now - r['newest']) / 60:7.0f} min ago")
