"""S4 critic: fresh holdout for the surviving S1/S3 policies.

Stages (run in order; every stage is cached and resumable under data/cache/strategy_holdout/):
  index    getSignaturesForAddress on the pump mint authority, newest first, back to anchor - 6.2 h.
           The anchor (the time of this call) is stored; the frame is creates with
           anchor - 6 h <= blockTime <= anchor - 60 min.
  decode   every create in the frame via getTransaction + dataset.creation_record (no outcome data).
           api.mainnet-beta.solana.com at <= 3 req/s.
  pull     first seconds of trades (swap-api, <= 1.2 req/s = 72/min, hard cap 800 requests incl.
           retries) for: (1) every launch whose creator is in the PRE-REGISTERED armed list
           (data/reports/strategy/critic_preregistration.json, written before this stage), then
           (2) a seeded random sample of the remaining non-mayhem SOL-quoted creates.
Run: .venv/bin/python -I analysis/scripts/strategy/critic_holdout.py <stage> [args]
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

HD = REPO / "data" / "cache" / "strategy_holdout"
HD.mkdir(parents=True, exist_ok=True)
MINT_AUTH = "TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM"
RPC = ("https://api.mainnet-beta.solana.com",)
REQ_CAP = 800
PREREG = REPO / "data" / "reports" / "strategy" / "critic_preregistration.json"


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
    pool = [s for s in idx["sigs"] if not s["err"] and s["bt"] and a - 6 * 3600 <= s["bt"] <= a - 3600]
    seen, out = set(), []
    for s in pool:
        if s["sig"] not in seen:
            seen.add(s["sig"])
            out.append(s)
    return idx, out


def decode():
    from rugsnipe_analysis.dataset import creation_record
    from rugsnipe_analysis.rpc import RpcClient
    idx, pool = frame()
    rpc = RpcClient(endpoints=RPC, max_rps=float(sys.argv[2]) if len(sys.argv) > 2 else 3.0, cache_root=str(HD))
    cp = HD / "creates.jsonl"
    have = set()
    if cp.exists():
        have = {json.loads(l)["q_sig"] for l in open(cp)}
    todo = [s for s in pool if s["sig"] not in have]
    random.Random(20261014).shuffle(todo)
    log("frame", len(pool), "to decode", len(todo))

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


def _decoded():
    out = []
    for line in open(HD / "creates.jsonl"):
        r = json.loads(line)
        if r.get("rec"):
            out.append(r["rec"])
    return out


def pull():
    """Tapes for armed-creator launches first, then the random (decode-order) sample. Hard cap REQ_CAP
    swap-api requests (retries included), <= 1.2 req/s. Only non-mayhem, SOL-quoted, single-create txs;
    slot-0 graduations (>= 84.9 SOL bought in the create tx) are skipped before pulling."""
    from rugsnipe_analysis.pumpapi import PumpApi, normalize_trade
    mode = sys.argv[2] if len(sys.argv) > 2 else "both"
    cap = min(REQ_CAP, int(sys.argv[3])) if len(sys.argv) > 3 else REQ_CAP   # per-run budget (total so far + this run)
    pre = json.load(open(PREREG))
    armed = set().union(*[set(v) for v in pre["armed"].values()])
    tp = HD / "tapes.jsonl"
    done, calls0 = set(), 0
    if tp.exists():
        for line in open(tp):
            r = json.loads(line)
            done.add(r["mint"])
            calls0 += r.get("calls", 0)
    api = PumpApi(max_rps=1.2, cache_root=str(HD))
    recs = _decoded()
    elig = [r for r in recs if not r["is_mayhem_mode"] and r["sol_quoted"] and r["n_creates_in_tx"] == 1
            and sum(x["sol"] for x in r["create_tx_trades"] if x["buy"]) < 84.9]
    arm = [r for r in elig if r["create_user"] in armed or r["creator"] in armed]
    rnd = [r for r in elig if not (r["create_user"] in armed or r["creator"] in armed)]
    queue = ([(r, "armed") for r in arm] if mode in ("both", "armed") else []) + \
            ([(r, "random") for r in rnd] if mode in ("both", "random") else [])
    log("decoded", len(recs), "eligible", len(elig), "armed", len(arm), "random", len(rnd), "done", len(done), "calls so far", calls0)
    fo = open(tp, "a")
    for r, grp in queue:
        if r["mint"] in done:
            continue
        if calls0 + api.stats["calls"] + 6 > cap:
            log("request cap reached")
            break
        c0 = api.stats["calls"]
        bt, slot = r["create_time"], r["create_slot"]
        seen, complete, hor = {}, False, None
        for W, mp in ((25, 1), (4, 1), (4, 4)):
            raw, comp = api.trades_before(r["mint"], (bt + W) * 1000 + 999, max_pages=mp)
            for t in raw:
                n = normalize_trade(t)
                seen[(n["sig"], n["sub"], n["user"])] = n
            if comp:
                complete, hor = True, W
                break
        tr = sorted(seen.values(), key=lambda x: (x["slot"], x["txi"], x["sub"]))
        tr = [t for t in tr if t["slot"] >= slot and t["ts"] <= bt + (hor or 4)]
        out = {"mint": r["mint"], "group": grp, "complete": complete, "hor": hor, "slot": slot, "bt": bt,
               "sig": r["create_sig"], "creator": r["creator"], "user": r["create_user"], "fee_payer": r["fee_payer"],
               "calls": api.stats["calls"] - c0,
               "trades": [[t["slot"], t["txi"], t["sub"], t["ts"], t["user"], int(t["buy"]), round(t["sol"], 9),
                           round(t["tok"], 6), t["mcap"], t["sig"], t["prog"]] for t in tr]}
        fo.write(json.dumps(out, separators=(",", ":")) + "\n")
        fo.flush()
        done.add(r["mint"])
        if len(done) % 25 == 0:
            log("tapes", len(done), "calls", calls0 + api.stats["calls"], api.stats)
    log("pull done", len(done), "calls", calls0 + api.stats["calls"], api.stats)


if __name__ == "__main__":
    {"index": index, "decode": decode, "pull": pull}[sys.argv[1]]()
