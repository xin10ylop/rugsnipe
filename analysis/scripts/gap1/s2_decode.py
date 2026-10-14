"""Stage 2: decode a uniform random sample of create txs from the 16-day index.

Output: data/cache/gap1/creates.jsonl (one row per decoded pump.fun create).
The sample order is a seeded shuffle, so any prefix is itself a uniform sample.
"""
import json
import random
import sys
import threading
from concurrent.futures import ThreadPoolExecutor
from urllib.parse import urlparse

from common import OUT, log, rpc
from rugsnipe_analysis.events import (CreateEvent, TradeEvent, b58decode, extract_events,
                                      fee_payer, top_level_programs)
from rugsnipe_analysis.funding import signers, sol_transfers

N = int(sys.argv[1]) if len(sys.argv) > 1 else 16000
MIN_AGE_H = 6
CB = "ComputeBudget111111111111111111111111111111"
c = rpc(13.0)
idx = json.load(open(OUT / "index.json"))
anchor = idx["anchor"]
pool = [s for s in idx["sigs"] if not s[3] and s[2] <= anchor - MIN_AGE_H * 3600]
random.Random(20261014).shuffle(pool)
log(f"pool {len(pool)} successful sigs older than {MIN_AGE_H}h; decoding first {N}")
out_path = OUT / "creates.jsonl"
done = set()
if out_path.exists():
    for line in open(out_path):
        done.add(json.loads(line)["sig"])
lock = threading.Lock()
fo = open(out_path, "a")


def host(uri):
    try:
        h = urlparse(uri).netloc.lower()
        return h or (uri.split(":")[0] if uri else "")
    except Exception:
        return ""


def decode(item):
    sig, slot, bt, _ = item
    if sig in done:
        return
    tx = c.get_transaction(sig)
    row = {"sig": sig, "slot": slot, "bt": bt, "rank": rank[sig]}
    if not tx or (tx.get("meta") or {}).get("err"):
        row["kind"] = "missing_or_failed"
    else:
        evs = extract_events(tx)
        cr = [e for e in evs if isinstance(e, CreateEvent)]
        if not cr:
            row["kind"] = "not_create"
        else:
            e = cr[0]
            fp = fee_payer(tx)
            trades = [t for t in evs if isinstance(t, TradeEvent) and t.mint == e.mint]
            msg = tx["transaction"]["message"]
            cu_lim = cu_px = None
            for ix in msg.get("instructions", []):
                if ix.get("programId") == CB and ix.get("data"):
                    try:
                        d = b58decode(ix["data"])
                    except Exception:
                        continue
                    if d[:1] == b"\x02" and len(d) >= 5:
                        cu_lim = int.from_bytes(d[1:5], "little")
                    elif d[:1] == b"\x03" and len(d) >= 9:
                        cu_px = int.from_bytes(d[1:9], "little")
            tr = [(t.destination, t.lamports) for t in sol_transfers(tx)
                  if t.source == fp and not t.inner]
            meta = tx["meta"]
            row.update({
                "kind": "create", "mint": e.mint, "creator": e.creator, "user": e.user,
                "fee_payer": fp, "signers": signers(tx), "name": e.name[:40], "symbol": e.symbol[:16],
                "uri": e.uri[:160] if hasattr(e, "uri") else None,
                "host": host(getattr(e, "uri", "") or ""),
                "sol_quoted": e.sol_quoted, "quote_mint": e.quote_mint, "mayhem": e.is_mayhem_mode,
                "token_program": e.token_program, "n_creates": len(cr),
                "progs": sorted(set(top_level_programs(tx))),
                "dev_buys": [[t.user, t.sol_amount / 1e9, t.token_amount / 1e6] for t in trades if t.is_buy],
                "cu_limit": cu_lim, "cu_price": cu_px, "fee": meta.get("fee"),
                "cu_used": meta.get("computeUnitsConsumed"),
                "payer_pre": (meta.get("preBalances") or [None])[0],
                "top_transfers": tr, "version": tx.get("version"),
            })
    with lock:
        fo.write(json.dumps(row, separators=(",", ":")) + "\n")
        fo.flush()
        done.add(sig)
        if len(done) % 500 == 0:
            log(f"{len(done)} decoded; rpc {c.stats}")


todo = pool[:N]
rank = {s[0]: i for i, s in enumerate(todo)}
with ThreadPoolExecutor(14) as ex:
    for f in [ex.submit(decode, it) for it in todo]:
        try:
            f.result()
        except Exception as err:  # noqa: BLE001
            log("fail", type(err).__name__, str(err)[:150])
log("done", len(done), c.stats)
