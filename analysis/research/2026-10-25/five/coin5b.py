# Per coin, anchored on his / our own transactions (no full history walk): every transaction of the create slot and the
# next slot (the create, the bundle run, who bought first behind it, his and our attempts), and the curve state sampled
# at create + 1..12 (the last trade at or before each slot, inside the window up to the last sell by him or us).
# -> five/coin/<mint>.json
import sys, json, os, struct, time, collections, threading
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl"); sys.path.insert(0, "/home/user/rugsnipe/analysis")
import tri, pumpparse as PP, common as C
from rugsnipe_analysis import events as E
tri.MIN_GAP = 0.01
OUT = SP + "/dec/five"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
SAMPLE = (1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12)
def log(*a): print(time.strftime("%H:%M:%S"), *a, flush=True)
def rstr(b, o):
    n = struct.unpack_from("<I", b, o)[0]; return b[o + 4:o + 4 + n], o + 4 + n
def page(bc, before):
    return tri.rpc("getSignaturesForAddress", [bc, {"limit": 1000, "before": before}], cache=True) or []
def decode(t, m, out, rows, failed):
    ks = PP.keys_of(t); idx = t.get("transactionIndex") or 0
    if (t.get("meta") or {}).get("err") is not None:
        failed.append([t["slot"], idx, ks[0]]); return
    for prog, accs, data, parsed, oi, inner in PP._ixs(t, ks):
        if prog != C.PUMP or not data: continue
        if data[:8] in PP.IXR and PP.IXR[data[:8]].startswith("create") and accs and accs[0] == m and "create" not in out:
            try:
                o = 8; name, o = rstr(data, o); sym, o = rstr(data, o); uri, o = rstr(data, o)
                creator = C.b58e(data[o:o + 32]); tail = data[o + 32:]
            except Exception:
                name = sym = uri = b""; creator = None; tail = b""
            u = uri.decode(errors="replace")
            host = u.split("//")[-1].split("/")[0].lower() if "//" in u else None
            hdr = t["transaction"]["message"]["header"]
            out["create"] = dict(slot=t["slot"], idx=idx, signer=ks[0], signers=ks[:hdr["numRequiredSignatures"]], creator=creator,
                                 ix=PP.IXR[data[:8]], inner=bool(inner), uri=u[:200], host=host, name=name.decode(errors="replace")[:40],
                                 sym=sym.decode(errors="replace")[:20], tail=tail.hex(), fee=t["meta"].get("fee"),
                                 progs=sorted({k for k in ks if len(k) > 30 and not k.startswith("1111")})[:16])
        if data[:8] == E.EVENT_IX_TAG and data[8:16] == E.DISC_TRADE:
            try: e = E.decode_trade(data[8:])
            except Exception: continue
            if e.mint == m:
                rows.append([t["slot"], idx, e.user, int(e.is_buy), e.sol_amount, e.token_amount, e.virtual_sol_reserves, e.virtual_token_reserves, (e.fee or 0) + (e.creator_fee or 0), ks[0]])
def coin(m, first_sig, last_sig):
    p = f"{OUT}/coin/{m}.json"
    if os.path.exists(p): return "have"
    bc = C.bonding_curve(m)
    older = page(bc, first_sig)            # everything before the first attempt (create slot run)
    if len(older) >= 1000: older = older[:1000]
    win = page(bc, last_sig) if last_sig and last_sig != first_sig else []
    sigs = {x["signature"]: x for x in older + win}
    me = tri.tx(first_sig)
    if me: sigs.setdefault(first_sig, dict(signature=first_sig, slot=me["slot"], err=(me.get("meta") or {}).get("err")))
    if not sigs:
        json.dump(dict(mint=m, ok=False), open(p, "w")); return "empty"
    reached = len(older) < 1000
    cs = min(x["slot"] for x in sigs.values())
    out = dict(mint=m, cs=cs, reached=reached, n_older=len(older), n_win=len(win))
    rows = []; failed = []
    # the create slot and the next: every transaction (failed ones too: who tried)
    for s, x in sigs.items():
        if x["slot"] <= cs + 1:
            t = tri.tx(s)
            if t: decode(t, m, out, rows, failed)
    # later slots: one sample per slot (the last successful transaction at that slot)
    by = collections.defaultdict(list)
    for s, x in sigs.items():
        if cs + 1 < x["slot"] <= cs + 12 and not x.get("err"): by[x["slot"]].append(s)
    for sl in sorted(by):
        # the listing is newest first within a slot: the first one listed is the last executed
        s = next(s for s in (y["signature"] for y in (older + win) if y["slot"] == sl and not y.get("err")))
        t = tri.tx(s)
        if t: decode(t, m, out, rows, failed)
    rows.sort(key=lambda r: (r[0], r[1]))
    out["rows"] = rows; out["failed"] = sorted(failed)
    out["slots_seen"] = sorted({x["slot"] - cs for x in sigs.values() if x["slot"] <= cs + 12})
    json.dump(out, open(p + ".tmp", "w")); os.replace(p + ".tmp", p)
    return "ok" if reached and "create" in out else "partial"
if __name__ == "__main__":
    W = {w: json.load(open(f"{OUT}/walk_{w[:6]}.json"))["rows"] for w in (A, US)}
    first = {}; last = {}; prio = collections.Counter()
    for w, rows in W.items():
        for r in rows:
            m = r["m"]
            if r["kind"] == "buy":
                if m not in first or (r["slot"], r["idx"]) < first[m][0]: first[m] = ((r["slot"], r["idx"]), r["sig"])
                if r["ok"]: prio[m] += 2 if w == A else 3
                else: prio[m] += 1 if w == US else 0
            elif r["ok"]:
                if m not in last or (r["slot"], r["idx"]) > last[m][0]: last[m] = ((r["slot"], r["idx"]), r["sig"])
    mints = sorted(first, key=lambda m: -prio[m])
    if len(sys.argv) > 1: mints = mints[:int(sys.argv[1])]
    log("coins", len(mints), "with a landed buy", sum(1 for m in mints if prio[m] >= 2))
    cnt = collections.Counter(); lock = threading.Lock()
    def safe(m):
        try: r = coin(m, first[m][1], last.get(m, (None, None))[1])
        except Exception as e: r = "error"
        with lock:
            cnt[r] += 1
            if sum(cnt.values()) % 250 == 0: log(dict(cnt))
    with ThreadPoolExecutor(12) as ex: list(ex.map(safe, mints))
    log("done", dict(cnt))
