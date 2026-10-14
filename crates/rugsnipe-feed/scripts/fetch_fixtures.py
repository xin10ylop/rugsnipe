#!/usr/bin/env python3
"""Build the offline fixtures of the rugsnipe-feed crate (same pattern as
crates/pump-codec/scripts/fetch_fixtures.py: standard library, polite, the exact
query stored next to the verbatim result; the tests never touch the network).

    python3 -I crates/rugsnipe-feed/scripts/fetch_fixtures.py funding [--force]
    python3 -I crates/rugsnipe-feed/scripts/fetch_fixtures.py frames <recorder.jsonl> [...]
    python3 -I crates/rugsnipe-feed/scripts/fetch_fixtures.py relay

* ``funding``: a ``getTransaction(jsonParsed)`` of a recent system transfer sent by the
  Binance hot wallet ``5tzFki...`` (the funder named in docs/06 section 9.6) ->
  ``fixtures/funding_get_transaction.json``.
* ``frames``: picks a varied subset of live ``logsSubscribe`` frames recorded by
  ``examples/tail.rs --record`` (creates with and without in-tx buys, mayhem,
  non-SOL quote, failed transactions) -> ``fixtures/logs_frames.jsonl``.
* ``relay``: the PumpPortal frames recorded by the collector selftest
  (``analysis/rugsnipe_analysis/testdata/collector_frames.json``) in the collector's
  relay line format -> ``fixtures/relay_pp.txt``.
"""
import base64
import json
import os
import sys
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
FIX = os.path.normpath(os.path.join(HERE, "..", "fixtures"))
ROOT = os.path.normpath(os.path.join(HERE, "..", "..", ".."))
RPC = "https://api.mainnet-beta.solana.com"
BINANCE = "5tzFkiKscXHK5ZXCGbXZxdw7gTjjD1mBwuoFbhUvuAi9"
PUMP = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"


def rpc(method, params, tries=6):
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    delay = 1.0
    for _ in range(tries):
        req = urllib.request.Request(RPC, data=body, headers={"content-type": "application/json",
                                                               "user-agent": "rugsnipe-fixtures"})
        try:
            with urllib.request.urlopen(req, timeout=30) as r:
                d = json.load(r)
            if "error" in d and d["error"].get("code") in (429, -32429):
                raise urllib.error.URLError("rate limited")
            time.sleep(0.4)
            return d
        except (urllib.error.URLError, TimeoutError):
            time.sleep(delay)
            delay *= 2
    raise RuntimeError(f"{method} failed")


def funding(force):
    path = os.path.join(FIX, "funding_get_transaction.json")
    if os.path.exists(path) and not force:
        print(f"exists: {path}")
        return
    sigs = rpc("getSignaturesForAddress", [BINANCE, {"limit": 40}])["result"]
    for s in sigs:
        if s.get("err") is not None:
            continue
        params = [s["signature"], {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 1,
                                   "commitment": "confirmed"}]
        d = rpc("getTransaction", params)
        r = d.get("result")
        if not r:
            continue
        ixs = r["transaction"]["message"]["instructions"]
        hit = [i for i in ixs if i.get("program") == "system" and i.get("parsed", {}).get("type") == "transfer"
               and i["parsed"]["info"].get("source") == BINANCE]
        if hit:
            out = {"query": {"endpoint": RPC, "method": "getTransaction", "params": params},
                   "funder": BINANCE, "result": r}
            os.makedirs(FIX, exist_ok=True)
            with open(path, "w") as f:
                json.dump(out, f, indent=1)
                f.write("\n")
            info = hit[0]["parsed"]["info"]
            print(f"wrote {path}: {s['signature']} {info['lamports']} lamports -> {info['destination']}")
            return
    raise SystemExit("no system transfer by the funder in the last 40 signatures")


def frames(paths):
    rows = []
    for p in paths:
        with open(p) as f:
            rows += [json.loads(line) for line in f if line.strip()]
    picked, kinds = [], {}

    def kind(r):
        fr = r.get("frame") or {}
        if fr.get("method") != "logsNotification":
            return "ack" if "result" in fr else None
        v = fr["params"]["result"]["value"]
        logs = v.get("logs") or []
        if v.get("err") is not None:
            return "failed"
        text = "\n".join(logs)
        if "Log truncated" in text:
            return "truncated"
        n_buys = text.count("Instruction: Buy") + text.count("Instruction: BuyExactSolIn") + \
            text.count("Instruction: BuyV3") + text.count("Instruction: BuyExactQuoteInV3") + \
            text.count("Instruction: BuyV2") + text.count("Instruction: BuyExactQuoteInV2")
        legacy = "Program log: Instruction: Create\n" in text + "\n"
        return f"create_buys{min(n_buys, 2)}" + ("_legacy" if legacy else "")

    for r in rows:
        k = kind(r)
        if k is None:
            continue
        cap = 2 if k in ("ack", "failed", "truncated") else 6
        if kinds.get(k, 0) < cap:
            kinds[k] = kinds.get(k, 0) + 1
            picked.append(r)
    picked.sort(key=lambda r: r["recv_ns"])
    path = os.path.join(FIX, "logs_frames.jsonl")
    with open(path, "w") as f:
        for r in picked:
            f.write(json.dumps(r, separators=(",", ":")) + "\n")
    print(f"wrote {path}: {len(picked)} frames {kinds}")


def relay():
    src = os.path.join(ROOT, "analysis", "rugsnipe_analysis", "testdata", "collector_frames.json")
    d = json.load(open(src))
    path = os.path.join(FIX, "relay_pp.txt")
    with open(path, "w") as f:
        t = 1791818209000000000
        for i, k in enumerate(("pp_ack", "pp_create", "pp_create_mayhem", "pp_create")):
            f.write(f"{t + i * 1000000}\t{d[k]}\n")
    print(f"wrote {path} (the second pp_create is a duplicate on purpose)")


def main():
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    cmd = sys.argv[1]
    if cmd == "funding":
        funding("--force" in sys.argv)
    elif cmd == "frames":
        frames(sys.argv[2:])
    elif cmd == "relay":
        relay()
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main()
