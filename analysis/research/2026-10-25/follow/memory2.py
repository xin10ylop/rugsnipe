# Long memory, unbounded: his attempts Aug 2026 -> Oct 23 (his wallet starts in Aug), each bundle wallet's whole history
# (up to 40 pages). For the bundle wallets of his rule picks the keyed follow missed (Oct 23 12-24 + Oct 24 00-12), and of a
# control sample of coins he did not take, how many of the wallet's launches did he attempt BEFORE Oct 23 (his month)?
# A wallet's create-slot buy at slot s and his buy at s or s + 1 on the same mint = one earlier attempt.
import sys, os, json, collections, random, threading, time
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl")
import tri, pumpparse as PP
tri.MIN_GAP = 0.025
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; PUMP = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
D = {}
for day in (23, 24):
    for r in json.load(open(SP + f"/pm/dataset_{day}.json")):
        r["per"] = "23a" if day == 23 and r["hour"] < 12 else "23b" if day == 23 else ("24a" if r["hour"] < 12 else "24b"); D[r["mint"]] = r
BW = json.load(open(SP + "/dec/bwall.json")); cat = json.load(open(SP + "/dec/cell/cat.json"))
TOOLS = ("metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io")
TEMPLATE = lambda d: (0.95 <= d < 1.05) or (2.95 <= d < 4.05) or (4.95 <= d < 5.05) or d >= 7
isrule = lambda r: r["host"] in TOOLS and TEMPLATE(r["dev"])
W0 = min(r["slot"] for r in D.values())  # Oct 23 00:00
miss = [m for m, c in cat.items() if D[m]["per"] in ("23b", "24a") and isrule(D[m]) and D[m]["he"] and c not in ("followed", "no bundle wallets")]
random.seed(7)
ctrl_pool = [m for m, c in cat.items() if D[m]["per"] in ("23b", "24a") and isrule(D[m]) and not D[m]["he"] and c in ("known, he never took", "new wallets")]
ctrl = random.sample(ctrl_pool, min(len(miss), len(ctrl_pool)))
# his buy transactions before Oct 23 by slot
his = collections.defaultdict(list)
for l in open(SP + "/month/AHwJSnFN.jsonl"):
    r = json.loads(l)
    if "buy" in (r.get("pix") or []) and r["s"] < W0: his[r["s"]].append(r)
for slot, mint, failed in json.load(open(SP + "/dec/hist/older_att.json")):
    his[slot].append(dict(sig=None, mints=[mint], s=slot))
print("missed picks", len(miss), "control", len(ctrl), "his earlier buy slots", len(his), flush=True)
def mint_bought(t, who):
    ks = PP.keys_of(t)
    for prog, accs, data, parsed, oi, inner in PP._ixs(t, ks):
        if prog == PUMP and data[:8] in PP.IXR and PP.IXR[data[:8]].startswith("buy") and len(accs) > 6:
            return accs[2]
    return None
OUT = SP + "/dec/cell/memory2.json"
S = json.load(open(OUT)) if os.path.exists(OUT) else {}
lock = threading.Lock()
def one(w):
    if w in S: return
    before = None; n = 0; hits = set(); pages = 0
    oldest = None
    while pages < 40:
        ss = tri.sigs(w, before=before); pages += 1
        if not ss: break
        for x in ss:
            s = x["slot"]
            if s >= W0 or x.get("err") is not None: continue
            if s in his or s + 1 in his:
                t = tri.tx(x["signature"])
                if not t: continue
                m = mint_bought(t, w)
                if not m: continue
                for hr in his.get(s, []) + his.get(s + 1, []):
                    hm = (hr.get("mints") or [None])[0]
                    if hm is None and hr.get("sig"):
                        ht = tri.tx(hr["sig"]); hm = mint_bought(ht, A) if ht else None
                    if hm == m: hits.add(m)
        n += len(ss); before = ss[-1]["signature"]; oldest = ss[-1]["slot"]
        if len(ss) < 1000: break
    with lock: S[w] = dict(n=n, pages=pages, oldest=oldest, full=pages < 40, earlier=sorted(hits))
ws = sorted({w for m in miss + ctrl for w in BW[m]["bw"]})
print("wallets", len(ws), flush=True)
t0 = time.time()
def run(w):
    try: one(w)
    except Exception as e: print("err", w[:8], str(e)[:80], flush=True)
with ThreadPoolExecutor(6) as ex:
    for i, _ in enumerate(ex.map(run, ws)):
        if i % 50 == 0:
            with lock: json.dump(S, open(OUT, "w"))
            print(f"{i}/{len(ws)} {time.time()-t0:.0f}s", flush=True)
json.dump(S, open(OUT, "w"))
def mem(m): return max((len(S.get(w, {}).get("earlier", [])) for w in BW[m]["bw"]), default=0)
for lab, ms in (("his missed rule picks", miss), ("control: rule coins he skipped", ctrl)):
    c = collections.Counter(min(mem(m), 5) for m in ms)
    print(lab, len(ms), "-> max earlier attempts on a bundle wallet (5 = 5+):", dict(sorted(c.items())))
