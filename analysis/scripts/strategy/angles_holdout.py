"""G4 skeptic: a second fresh holdout (creates of the last 6 h that are >= 45 min old at index time).

Stages (cached and resumable under data/cache/strategy_holdout2/):
  index    getSignaturesForAddress on the pump mint authority, newest first, back to anchor - 6.2 h.
           Frame: anchor - 6 h <= blockTime <= anchor - 45 min. No outcome data.
  decode   creates in the frame via getTransaction + dataset.creation_record (random order, seeded).
           api.mainnet-beta.solana.com at <= 3.5 req/s.
  screen   getBlock(signatures) of each decoded create's slot is NOT used (too heavy); instead the
           multi-wallet screen is done on the first trades page (see pull).
  pull     first seconds of trades (swap-api, <= 0.6 req/s = 36/min, hard cap 500 requests incl.
           retries) for a seeded random sample of non-mayhem SOL-quoted single-create launches not
           already in any earlier cache. Pulled only after angles_prereg.json was written.
Run: .venv/bin/python -I analysis/scripts/strategy/angles_holdout.py <stage> [args]
"""
from __future__ import annotations

import json
import random
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "analysis"))

HD = REPO / "data" / "cache" / "strategy_holdout2"
HD.mkdir(parents=True, exist_ok=True)
MINT_AUTH = "TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM"
RPC = ("https://api.mainnet-beta.solana.com",)
REQ_CAP = 500
PREREG = REPO / "data" / "reports" / "strategy" / "angles_prereg.json"


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def index():
    from rugsnipe_analysis.rpc import RpcClient
    p = HD / "index.json"
    if p.exists():
        log("index exists", p)
        return
    rpc = RpcClient(endpoints=RPC, max_rps=2.0, cache_root=str(HD))
    anchor = int(time.time())
    sigs, before, pages = [], None, 0
    while True:
        page = rpc.get_signatures(MINT_AUTH, before=before)
        pages += 1
        if not page:
            break
        sigs += [{"sig": s["signature"], "slot": s["slot"], "bt": s.get("blockTime"), "err": s.get("err") is not None}
                 for s in page]
        before = page[-1]["signature"]
        oldest = page[-1].get("blockTime") or anchor
        log("page", pages, len(sigs), "oldest h ago", round((anchor - oldest) / 3600, 2))
        if oldest < anchor - int(6.2 * 3600):
            break
    json.dump({"anchor": anchor, "pages": pages, "sigs": sigs}, open(p, "w"))
    log("index written", len(sigs), "sigs, rpc", rpc.stats)


def frame():
    idx = json.load(open(HD / "index.json"))
    a = idx["anchor"]
    pool = [s for s in idx["sigs"] if not s["err"] and s["bt"] and a - 6 * 3600 <= s["bt"] <= a - 45 * 60]
    seen, out = set(), []
    for s in pool:
        if s["sig"] not in seen:
            seen.add(s["sig"])
            out.append(s)
    return idx, out


def decode():
    """Decode the creates of the dense block [anchor - H h - 45 min, anchor - 45 min] (argv[2] = H, default 2),
    newest first, so the block is complete back to wherever the run stops."""
    from rugsnipe_analysis.dataset import creation_record
    from rugsnipe_analysis.rpc import RpcClient
    hours = float(sys.argv[2]) if len(sys.argv) > 2 else 2.0
    rps = float(sys.argv[3]) if len(sys.argv) > 3 else 3.5
    idx, pool = frame()
    a = idx["anchor"]
    rpc = RpcClient(endpoints=RPC, max_rps=rps, cache_root=str(HD))
    cp = HD / "creates.jsonl"
    have = set()
    if cp.exists():
        have = {json.loads(l)["q_sig"] for l in open(cp)}
    order = sorted((s for s in pool if s["bt"] >= a - 45 * 60 - int(hours * 3600)), key=lambda s: (-s["bt"], s["sig"]))
    todo = [s for s in order if s["sig"] not in have]
    log("frame", len(pool), "to decode", len(todo), "have", len(have))

    def dec(s):
        try:
            return {"q_sig": s["sig"], "rec": creation_record(rpc, s["sig"])}
        except Exception as e:  # noqa: BLE001
            return {"q_sig": s["sig"], "err": str(e)[:120]}

    n = 0
    with ThreadPoolExecutor(4) as ex, open(cp, "a") as fo:
        for r in ex.map(dec, todo):
            fo.write(json.dumps(r, separators=(",", ":")) + "\n")
            n += 1
            if n % 250 == 0:
                fo.flush()
                log("decoded", n, rpc.stats)
    log("decode done", n, rpc.stats)


def seen_mints() -> set:
    """Mints already in any earlier cache (so the new holdout is disjoint from every set used so far)."""
    out = set()
    for p, key in ((REPO / "data/cache/strategy_holdout/tapes.jsonl", "mint"), (REPO / "data/cache/gap3/trades.jsonl", "mint"),
                   (REPO / "data/cache/window/creates.jsonl", "mint"), (REPO / "data/tokens_light.jsonl", "mint"),
                   (REPO / "data/tokens_sample.jsonl", "mint"), (REPO / "data/cache/strategy/holdout_trades.jsonl", "mint"),
                   (REPO / "data/cache/predictor/table.jsonl", "mint"), (REPO / "data/cache/strategy/op_tapes.jsonl", "mint")):
        if p.exists():
            for line in open(p):
                try:
                    out.add(json.loads(line)[key])
                except (ValueError, KeyError):
                    pass
    return out


def _block_recs():
    idx, pool = frame()
    a = idx["anchor"]
    recs = {}
    for line in open(HD / "creates.jsonl"):
        r = json.loads(line)
        if r.get("rec"):
            recs[r["q_sig"]] = r["rec"]
    return idx, recs


def eligible(r) -> bool:
    return (not r["is_mayhem_mode"] and r["sol_quoted"] and r["n_creates_in_tx"] == 1
            and sum(x["sol"] for x in r["create_tx_trades"] if x["buy"]) < 84.9)


def prescreen():
    """Multi-wallet screen from the chain only (no outcome data): getSignaturesForAddress on the bonding curve with
    a later create's signature as the `before` cursor (the first ~5 s of curve activity), then getTransaction for the
    successful slot-0 transactions right after the create, applying tapes._raw_bundle's rule (contiguous buys at
    tx index <= create + 5, gaps <= 2). Output prescreen.jsonl: multi True/False/None (unknown -> pull)."""
    import bisect
    from rugsnipe_analysis import events as E
    from rugsnipe_analysis.rpc import RpcClient
    rps = float(sys.argv[2]) if len(sys.argv) > 2 else 1.2
    rpc = RpcClient(endpoints=RPC, max_rps=rps, cache_root=str(HD))
    idx, recs = _block_recs()
    allsig = sorted((s for s in idx["sigs"] if not s["err"]), key=lambda s: (s["slot"], s["sig"]))
    slots = [s["slot"] for s in allsig]
    seen = seen_mints()
    out_p = HD / "prescreen.jsonl"
    done = set()
    if out_p.exists():
        done = {json.loads(l)["sig"] for l in open(out_p)}
    todo = sorted((r for r in recs.values() if eligible(r) and r["mint"] not in seen and r["create_sig"] not in done),
                  key=lambda r: (-r["create_time"], r["create_sig"]))
    log("decoded", len(recs), "eligible todo", len(todo), "done", len(done))
    fo = open(out_p, "a")
    for n, r in enumerate(todo):
        S0, csig = r["create_slot"], r["create_sig"]
        C = {r["creator"], r["create_user"], r["fee_payer"]}
        j = bisect.bisect_left(slots, S0 + 25)
        if j >= len(allsig):
            continue
        before = allsig[j]["sig"]
        res = rpc.call("getSignaturesForAddress", [r["bonding_curve"], {"limit": 1000, "before": before}],
                       cache_key=f"pre:{r['bonding_curve']}:{before}") or []
        reached = any(x["signature"] == csig for x in res)
        s0 = [x for x in res if x["slot"] == S0]
        s0_ok = [x["signature"] for x in reversed(s0) if x.get("err") is None and x["signature"] != csig]
        s0_fail = sum(1 for x in s0 if x.get("err") is not None)
        s1 = [x for x in res if x["slot"] == S0 + 1]
        s1_fail = sum(1 for x in s1 if x.get("err") is not None)
        intx_other = any(x["buy"] and x["user"] not in C for x in r["create_tx_trades"])
        multi, ctx, run = None, None, []
        if reached and not s0_ok and not intx_other:
            multi = False
        elif reached:
            ctx_tx = rpc.get_transaction(csig)
            ctx = (ctx_tx or {}).get("transactionIndex")
            B = {x["user"] for x in r["create_tx_trades"] if x["buy"] and x["user"] not in C}
            prev = ctx
            for sg in s0_ok[:8]:
                if ctx is None:
                    break
                tx = rpc.get_transaction(sg)
                if not tx:
                    continue
                ti = tx.get("transactionIndex")
                if ti is None or ti <= ctx:
                    continue
                evs = [e for e in E.extract_events(tx) if isinstance(e, E.TradeEvent) and e.mint == r["mint"]]
                buy = any(e.is_buy for e in evs)
                if ti - prev <= 2 and ti - ctx <= 5 and buy:
                    run.append(ti)
                    prev = ti
                    B |= {e.user for e in evs if e.is_buy and e.user not in C}
                elif ti - prev > 2 or ti - ctx > 5:
                    break
            multi = bool(B) if ctx is not None else None
        fo.write(json.dumps(dict(sig=csig, mint=r["mint"], slot=S0, bt=r["create_time"], reached=reached, n_s0_ok=len(s0_ok),
                                 n_s0_fail=s0_fail, n_s1_fail=s1_fail, n_sigs=len(res), ctx=ctx, run=run, multi=multi),
                            separators=(",", ":")) + "\n")
        fo.flush()
        if (n + 1) % 100 == 0:
            log("prescreened", n + 1, rpc.stats)
    log("prescreen done", rpc.stats)


def pull():
    """HB tapes (swap-api, <= 0.6 req/s, hard cap REQ_CAP requests incl. retries) for every prescreened launch with
    multi True or None, strictly newest first, so HB is a dense, time-contiguous block. Waits for the prescreen
    when it catches up with it. Windows: 10 s x 1 page; if incomplete 10 s x 3 pages; then 4 s x 3 pages."""
    from rugsnipe_analysis.pumpapi import PumpApi, normalize_trade
    if not PREREG.exists():
        raise SystemExit("pre-registration missing: run angles_prereg.py first")
    api = PumpApi(max_rps=0.6, cache_root=str(HD))
    tp = HD / "tapes.jsonl"
    done, calls0 = set(), 0
    if tp.exists():
        for line in open(tp):
            r = json.loads(line)
            done.add(r["mint"])
            calls0 += r.get("calls", 0)
    _, recs = _block_recs()
    by_sig = {r["create_sig"]: r for r in recs.values()}
    log("pull start, done", len(done), "calls so far", calls0)
    fo = open(tp, "a")
    idle_rounds = 0
    last_bt = None
    while True:
        pre = [json.loads(l) for l in open(HD / "prescreen.jsonl")]
        pre.sort(key=lambda x: (-x["bt"], x["sig"]))
        todo = [x for x in pre if x["multi"] is not False and x["mint"] not in done]
        if last_bt is not None:
            todo = [x for x in todo if x["bt"] <= last_bt]   # keep the block contiguous
        if not todo:
            idle_rounds += 1
            if idle_rounds > 30:
                break
            time.sleep(20)
            continue
        idle_rounds = 0
        for x in todo:
            if calls0 + api.stats["calls"] + 4 > REQ_CAP:
                log("request cap reached", calls0 + api.stats["calls"])
                log("pull done", len(done), api.stats, "oldest bt", last_bt)
                return
            r = by_sig.get(x["sig"])
            if r is None:
                continue
            c0 = api.stats["calls"]
            bt, slot = r["create_time"], r["create_slot"]
            seen, complete, hor = {}, False, None
            for W_, mp in ((10, 1), (10, 3), (4, 3)):
                raw, comp = api.trades_before(r["mint"], (bt + W_) * 1000 + 999, max_pages=mp)
                for t in raw:
                    n = normalize_trade(t)
                    seen[(n["sig"], n["sub"], n["user"])] = n
                if comp:
                    complete, hor = True, W_
                    break
            tr = sorted(seen.values(), key=lambda q: (q["slot"], q["txi"], q["sub"]))
            tr = [t for t in tr if t["slot"] >= slot and t["ts"] <= bt + (hor or 4)]
            out = {"mint": r["mint"], "complete": complete, "hor": hor, "slot": slot, "bt": bt, "sig": r["create_sig"],
                   "creator": r["creator"], "user": r["create_user"], "fee_payer": r["fee_payer"], "pre": x,
                   "calls": api.stats["calls"] - c0,
                   "trades": [[t["slot"], t["txi"], t["sub"], t["ts"], t["user"], int(t["buy"]), round(t["sol"], 9),
                               round(t["tok"], 6), t["mcap"], t["sig"], t["prog"]] for t in tr]}
            fo.write(json.dumps(out, separators=(",", ":")) + "\n")
            fo.flush()
            done.add(r["mint"])
            last_bt = bt
            if len(done) % 25 == 0:
                log("tapes", len(done), "calls", calls0 + api.stats["calls"], api.stats,
                    time.strftime("%H:%M:%S", time.gmtime(bt)))
            break      # re-read the prescreen after each launch (it may have added newer-ordered rows? no: newest first)
    log("pull done", len(done), api.stats, "oldest bt", last_bt)


if __name__ == "__main__":
    {"index": index, "decode": decode, "prescreen": prescreen, "pull": pull}[sys.argv[1]]()
