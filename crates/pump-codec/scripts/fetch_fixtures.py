#!/usr/bin/env python3
"""Fetch the offline test fixtures for the pump-codec crate.

Run with an isolated interpreter from anywhere:

    python3 -I crates/pump-codec/scripts/fetch_fixtures.py            # fetch what is missing
    python3 -I crates/pump-codec/scripts/fetch_fixtures.py --force    # re-fetch everything
    python3 -I crates/pump-codec/scripts/fetch_fixtures.py --discover # print fresh candidates per kind

Standard library only. Public endpoints, polite: <= ~3 req/s, retry with backoff
on HTTP 429 / JSON-RPC 429. Transactions are fetched with
`encoding: "base64"` and `maxSupportedTransactionVersion: 1` so the raw wire
bytes (legacy, v0 and v1) are stored verbatim. The tests never touch the
network; they only read the JSON written here.
"""
import base64
import datetime
import json
import os
import sys
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
FIX = os.path.normpath(os.path.join(HERE, "..", "fixtures"))
RPCS = ["https://solana-rpc.publicnode.com", "https://api.mainnet-beta.solana.com"]
PUMP_API = "https://frontend-api-v3.pump.fun"
SOLANA_CHAIN = "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp"
PUMP = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
AMM = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"

# (name, signature, kind/notes). Kinds were discovered with --discover on
# 2026-10-14 and checked by hand (top-level pump instructions per tx).
TXS = [
    ("v1_create_v2_buy", "Mz4aKhSrFkoayenCzy7zwaTnZBACoNDP1XVq3qiVkDQpHH9wMxqt1GwnymGDQByzaPjCZGgd6tK7wE6nuvRWgT8",
     "tx v1: create_v2(16) + legacy buy(18) + buy_exact_quote_in_v3(17)"),
    ("v1_create_v2_buy_b", "4GvSxH2p7vZs6TnZjHH9Bu3VA8ppuTm5khaeDJ7eEpGrjqSX9ynP7UCHBLjRtTNbzsVhd5dYpSxJmiuZZraX5toA",
     "tx v1: create_v2(16) + legacy buy(18)"),
    ("v1_create_v2_extend_bexq_v3", "2iWZxKJAMz7fpdBLeW8eKz8KdRXtzM9nPzeiFK6VkLJ9xCct1rfWaPGCtrwryHcQCDLSe7BfshkUp9FyW28pCAEH",
     "tx v1: create_v2(16) + extend_account + buy_exact_quote_in_v3(17)"),
    ("v0_create_v2_buy_v3", "3HM6gFxFgGZuSggQXmTav1kEnQSZAuncQXq5kTCtMvZAR7KehkcHZyGo615Sq28ubVWn4KGwCjvREapDbZbo78Xu",
     "tx v0 (no lookups): create_v2(16) + buy_v3(17)"),
    ("v0_create_v2_quote_tail", "3NjDvfvTAfbhGFP2Pr8jY6HGp7hZjNxNxiWUTyVxz3u4QEboBinDEPG9L4sLCRMkbWL23htpQFtZengZ4F9qiAZZ",
     "tx v0 (1 ALT lookup): create_v2(20 = 16 + 4-account quote tail) + buy_v2(27), non-SOL quote"),
    ("worked_dev_buy_v2", "RwnaexcirWm231nopvZy6NiKiPi9wrhyGZskCqs6HNPNjpxaeP6RbouRsxqvJNtoRDfXdTUboqTsdiZZWx2iuaa",
     "tx v0 (1 ALT lookup): create_v2 + dev buy_v2 of 26,170,731,707,316 for 750,000,000 (mechanics doc section 4)"),
    ("worked_buy", "52REYvzrShCsc8wFKX1m1ZsPCWNnBjmRLnpFSKr7RrReHE88y8fgNegdEmqmyKCoiUVaibwv5CdMSRehvGNXtXWA",
     "legacy tx: router CPI (program GMgnVF...) into pump buy(18), 923,456,789 lamports -> 30,520,874,344,997 tokens (mechanics doc section 4)"),
    ("worked_sell", "54rjuNFq9AVoRQWdqFFbP2rJGpuvrn3NrQLMEQXfGe15CK1y6rtEeVhJeJcH7DJZE9dVc57fRhuNUmv7p51MzKDX",
     "legacy tx: durable-nonce advance + router CPI (program 6HSS4x...) into pump sell(16), sol_amount 2,412,711,917 (mechanics doc section 3/4)"),
    ("legacy_buy18", "2CyZ538P1vY1ak4psnaGAZp9WKQyahWZNDJBumqdj3YVrhvWutkhuJPMGeJiZgobBBbQeWbwGqikJd9UjkzumtUx",
     "legacy tx: legacy buy with 16 + 2 remaining accounts"),
    ("legacy_buy19", "nakbpohNMXhxCnMv4WbZzr1rdJH9PUkJd8AxtVTnUVoD9eJijMaqN5dbRnCiJwWQnKe5XX97tvCsd1uLKQQSdrd",
     "legacy tx: legacy buy with 19 accounts"),
    ("legacy_buy_exact_sol_in", "5JP3X7QndzxRFAw1VbheA7rQ1wDGENKb5vGP4L73w7QBWVsT26A2FqGeKvzT2sFeqozQiviNLyw7DWLnjWdDHwwq",
     "legacy tx: buy_exact_sol_in(18)"),
    ("legacy_sell16", "2v9BRCaA9qpQic8wLZHobSyYXVaxrE59t5AhKefgpSvapkLVWSrCDTD86srkKpjf84WbjdjyrPfUeQnkL7YwHQof",
     "legacy tx: legacy sell with 14 + 2 remaining accounts"),
    ("v1_sell16", "4oP7DLGj9RUbpcnj2jen1qhEKZet45zw3g7QgP7Y3QNfcE7CEp2bAnpbG9mdzxUCqbP2SAtx8C8twG6D7HzDLpzd",
     "tx v1: legacy sell(16)"),
    ("v1_buy_exact_quote_in_v3", "4HvEBDRtTnKteXsCFPbfdisLPwmsEu3cXcJsev4WdZUnPY6FjQSKpwHKowFa9kGJWcdFUCGU4wMNktiMehV8rA3q",
     "tx v1: buy_exact_quote_in_v3(17)"),
    ("v1_router_inner_trade", "4ETCP1WWt9RkEBzXfRnsYR7Ug3DVXXmYEN9F5JKpiTuDDYy1LWwwMq1sR7yiZA4rb4xPbB8RWXngML6QVtaYj3gE",
     "tx v1: no top-level pump ix, pump trade via router CPI (TradeEvent in inner ixs)"),
    ("legacy_create_spl", "2wMWZkGPr53DhL31YrLz1q8CJ6HWc4pnKVYwBxSuuCqsKQcoz6jzE5mtY9Wm6n9QWLDhDThDGZ5BLT5vMQXS32aA",
     "tx v0 (no lookups): legacy `create` (14 accounts, SPL Token mint) + legacy buy(18)"),
    ("create_buy_complete", "5vGT6VUYJEsMYDgZisbK5pX44RfNiQ6G78TDRM8ZHdGpePR3Yw5bnRJmjKpJqqjXG53jZYsfJheHBRy43gwRUYsW",
     "create + buy that completes the curve in one tx: CreateEvent, TradeEvent, CompleteEvent"),
    ("migrate", "2WdpuGxZAep26YsM7gcGWXMRmg71GouooHzwGP1irRk62gD2X2WAb8HgoBWzR1Mf4epv7JuoLMnkiuPMCaKMpFGS",
     "migration of the same mint: PumpSwap CreatePoolEvent + pump CompletePumpAmmMigrationEvent"),
    ("pumpswap_buy", "26Ds1VryRcoT43AdRcYsf4aA477ShXY3xZkdjgxQtoqcGRNjyJq8v1DTY34tVMA9V3cdq56VPnCYrY9Bm4nVRsRr",
     "PumpSwap buy (BuyEvent) on the freshly migrated pool"),
    ("pumpswap_sell", "2u8p5Z1vC8dcnY8jbpKg88rEipmh7ZkcWEpRCuvVEJBbGpYSSX2T1pmZPUCQ1YvCXz3W1bgx4ekSW9fYtPy5C8zo",
     "PumpSwap sell (SellEvent)"),
]

ACCOUNTS = [
    ("global", "4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf", "pump Global"),
    ("fee_config", "8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt", "pump-fees FeeConfig for the pump program"),
    ("bonding_curve_complete", "3PfeaUhTnEiwA9aS141sNLfttVboQM39dNedWicTVPUm",
     "BondingCurve of Cc2QsnFfxFYXaTvdXgJkUctM9ZsNBKfgaaaSihjepump (completed + migrated)"),
    ("bonding_curve_worked", "", "BondingCurve of 2mFxK8mUNr4MExKZ79t4wL3Z62yj8S3hDPTPZLbFpump (address filled from the API)"),
]

API = [
    ("coins_latest", "/coins?offset=0&limit=50&sort=created_timestamp&order=DESC&includeNsfw=true"),
    ("coins_completed", "/coins?offset=0&limit=10&sort=created_timestamp&order=DESC&includeNsfw=true&complete=true"),
    ("coin_worked", "/coins-v2/2mFxK8mUNr4MExKZ79t4wL3Z62yj8S3hDPTPZLbFpump"),
    ("trades_worked", "/trades/%s/2mFxK8mUNr4MExKZ79t4wL3Z62yj8S3hDPTPZLbFpump?limit=100" % SOLANA_CHAIN),
]

_last = [0.0]


def _throttle():
    dt = time.time() - _last[0]
    if dt < 0.34:
        time.sleep(0.34 - dt)
    _last[0] = time.time()


def rpc(method, params, tries=8):
    err = None
    for i in range(tries):
        url = RPCS[0] if i < 3 else RPCS[i % 2]
        _throttle()
        body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
        req = urllib.request.Request(url, data=body, headers={
            "content-type": "application/json", "user-agent": "rugsnipe-pump-codec-fixtures/0.1"})
        try:
            with urllib.request.urlopen(req, timeout=30) as r:
                d = json.load(r)
            if "error" in d:
                if d["error"].get("code") in (429, -32429, -32005):
                    time.sleep(2 * (i + 1))
                    continue
                raise RuntimeError(d["error"])
            return url, d["result"]
        except urllib.error.HTTPError as e:
            err = e
            if e.code in (429, 502, 503):
                time.sleep(2 * (i + 1))
                continue
            raise
        except (urllib.error.URLError, TimeoutError) as e:
            err = e
            time.sleep(1 + i)
    raise RuntimeError("rpc %s failed: %r" % (method, err))


def http_json(path, tries=6):
    for i in range(tries):
        _throttle()
        req = urllib.request.Request(PUMP_API + path, headers={
            "user-agent": "Mozilla/5.0 (rugsnipe fixtures)", "accept": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=30) as r:
                return json.load(r)
        except urllib.error.HTTPError as e:
            if e.code in (429, 503, 530):
                time.sleep(3 * (i + 1))
                continue
            raise
    raise RuntimeError("GET %s failed" % path)


def now():
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def write(path, obj):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        json.dump(obj, f, indent=1, sort_keys=True)
        f.write("\n")


def fetch_tx(name, sig, kind, force):
    path = os.path.join(FIX, "tx", name + ".json")
    if os.path.exists(path) and not force:
        return json.load(open(path))
    params = [sig, {"encoding": "base64", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}]
    url, res = rpc("getTransaction", params)
    if res is None:
        raise RuntimeError("transaction not found: " + sig)
    obj = {"name": name, "kind": kind, "signature": sig, "fetched_at": now(),
           "query": {"endpoint": url, "method": "getTransaction", "params": params}, "result": res}
    write(path, obj)
    print("tx", name, res["slot"], res.get("version"))
    return obj


def fetch_tx_json(name, sig, force):
    """Same transaction with `encoding: "json"`: RPC-decoded signatures,
    accountKeys, header, instructions and lookups, used as an independent
    oracle for the wire parser."""
    path = os.path.join(FIX, "tx_json", name + ".json")
    if os.path.exists(path) and not force:
        return json.load(open(path))
    params = [sig, {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}]
    url, res = rpc("getTransaction", params)
    if res is None:
        raise RuntimeError("transaction not found: " + sig)
    # Keep only the decoded transaction (meta is already in tx/<name>.json).
    obj = {"name": name, "signature": sig, "fetched_at": now(),
           "query": {"endpoint": url, "method": "getTransaction", "params": params},
           "result": {"slot": res["slot"], "version": res.get("version"), "transaction": res["transaction"]}}
    write(path, obj)
    return obj


def fetch_jito(force):
    path = os.path.join(FIX, "api", "jito_tip_accounts.json")
    if os.path.exists(path) and not force:
        return json.load(open(path))
    url = "https://mainnet.block-engine.jito.wtf/api/v1/getTipAccounts"
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "getTipAccounts", "params": []}).encode()
    _throttle()
    req = urllib.request.Request(url, data=body, headers={"content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=30) as r:
        res = json.load(r)["result"]
    obj = {"name": "jito_tip_accounts", "fetched_at": now(), "query": {"url": url, "method": "getTipAccounts"}, "result": res}
    write(path, obj)
    return obj


def fetch_account(name, address, note, force):
    path = os.path.join(FIX, "accounts", name + ".json")
    if os.path.exists(path) and not force:
        return json.load(open(path))
    params = [address, {"encoding": "base64", "commitment": "confirmed"}]
    url, res = rpc("getAccountInfo", params)
    obj = {"name": name, "address": address, "note": note, "fetched_at": now(),
           "query": {"endpoint": url, "method": "getAccountInfo", "params": params}, "result": res}
    write(path, obj)
    print("account", name, res["context"]["slot"])
    return obj


def fetch_api(name, path_q, force):
    path = os.path.join(FIX, "api", name + ".json")
    if os.path.exists(path) and not force:
        return json.load(open(path))
    res = http_json(path_q)
    obj = {"name": name, "fetched_at": now(), "query": {"url": PUMP_API + path_q}, "result": res}
    write(path, obj)
    print("api", name)
    return obj


def readme(txs, accts, apis):
    lines = [
        "# pump-codec fixtures",
        "",
        "Raw mainnet data used by the offline tests. Regenerate (network) with",
        "`python3 -I crates/pump-codec/scripts/fetch_fixtures.py [--force]`. Tests only read these files.",
        "",
        "Every transaction was fetched with JSON-RPC `getTransaction(sig, {\"encoding\":\"base64\",",
        "\"maxSupportedTransactionVersion\":1, \"commitment\":\"confirmed\"})`. The exact query and the",
        "endpoint that answered are stored in each file under `query`; the RPC result is stored verbatim",
        "under `result` (`result.transaction[0]` is the base64 wire transaction).",
        "`tx_json/<name>.json` holds the same transaction fetched with `\"encoding\":\"json\"` (RPC-decoded",
        "signatures, account keys, header, instructions, lookups): an independent oracle for the wire parser.",
        "`api/jito_tip_accounts.json` is the Jito block engine `getTipAccounts` response.",
        "",
        "## Transactions (`tx/`)",
        "",
        "| file | signature | slot | tx version | kind |",
        "|---|---|---|---|---|",
    ]
    for name, sig, kind in TXS:
        o = txs[name]
        v = o["result"].get("version")
        lines.append("| `tx/%s.json` | `%s` | %d | %s | %s |" % (name, sig, o["result"]["slot"], v, kind))
    lines += ["", "## Accounts (`accounts/`)", "",
              "Fetched with `getAccountInfo(address, {\"encoding\":\"base64\",\"commitment\":\"confirmed\"})`.", "",
              "| file | address | context slot | note |", "|---|---|---|---|"]
    for name, _addr, note in ACCOUNTS:
        o = accts[name]
        lines.append("| `accounts/%s.json` | `%s` | %d | %s |" % (name, o["address"], o["result"]["context"]["slot"], note))
    lines += ["", "## pump.fun API responses (`api/`)", "", "| file | URL | fetched |", "|---|---|---|"]
    for name, _p in API:
        o = apis[name]
        lines.append("| `api/%s.json` | `%s` | %s |" % (name, o["query"]["url"], o["fetched_at"]))
    lines.append("")
    with open(os.path.join(FIX, "README.md"), "w") as f:
        f.write("\n".join(lines))


# --- discovery (prints candidates; does not change the fixture list) -------
B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
DISC = {"181ec828051c0777": "create", "d6904cec5f8b31b4": "create_v2", "66063d1201daebea": "buy",
        "38fc74089edfcd5f": "buy_exact_sol_in", "33e685a4017f83ad": "sell", "b817ee6167c5d33d": "buy_v2",
        "5df6823ce7e940b2": "sell_v2", "c2ab1c46684d5b2f": "buy_exact_quote_in_v2", "07051dc4f5176550": "buy_v3",
        "1c92de7726c469d5": "sell_v3", "e1f7501ed5b38488": "buy_exact_quote_in_v3"}


def b58e(b):
    n = int.from_bytes(b, "big")
    s = ""
    while n:
        n, r = divmod(n, 58)
        s = B58[r] + s
    return "1" * (len(b) - len(b.lstrip(b"\0"))) + s


def _sv(b, o):
    v = s = 0
    while True:
        x = b[o]
        o += 1
        v |= (x & 0x7F) << s
        s += 7
        if not x & 0x80:
            return v, o


def top_level(raw):
    """(version, [(program, n_accounts, data)]) without resolving ALTs."""
    if raw[0] == 0x81:
        o = 4
        mask = int.from_bytes(raw[o:o + 4], "little")
        o += 36
        nix, nadd = raw[o], raw[o + 1]
        o += 2
        keys = [raw[o + 32 * i:o + 32 * i + 32] for i in range(nadd)]
        o += 32 * nadd + 4 * bin(mask).count("1")
        hdr = [(raw[o + 4 * i], raw[o + 4 * i + 1], int.from_bytes(raw[o + 4 * i + 2:o + 4 * i + 4], "little")) for i in range(nix)]
        o += 4 * nix
        out = []
        for p, na, nd in hdr:
            out.append((b58e(keys[p]), na, raw[o + na:o + na + nd]))
            o += na + nd
        return 1, out
    ns, o = _sv(raw, 0)
    o += 64 * ns
    ver = "legacy"
    if raw[o] & 0x80:
        ver = raw[o] & 0x7F
        o += 1
    o += 3
    nk, o = _sv(raw, o)
    keys = [raw[o + 32 * i:o + 32 * i + 32] for i in range(nk)]
    o += 32 * nk + 32
    nix, o = _sv(raw, o)
    out = []
    for _ in range(nix):
        p = raw[o]
        o += 1
        na, o = _sv(raw, o)
        o += na
        nd, o = _sv(raw, o)
        out.append((b58e(keys[p]), na, raw[o:o + nd]))
        o += nd
    return ver, out


def discover():
    coins = http_json(API[0][1])
    for c in coins[:20]:
        _, sigs = rpc("getSignaturesForAddress", [c["mint"], {"limit": 1000, "commitment": "confirmed"}])
        ok = [s for s in sigs if not s["err"]]
        if not ok:
            continue
        sig = ok[-1]["signature"]
        _, r = rpc("getTransaction", [sig, {"encoding": "base64", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}])
        ver, ixs = top_level(base64.b64decode(r["transaction"][0]))
        names = ["%s/%d" % (DISC.get(d[:8].hex(), d[:8].hex()), na) for p, na, d in ixs if p == PUMP]
        print("create", c["mint"], sig, r["slot"], "v=%s" % ver, names)
    _, sigs = rpc("getSignaturesForAddress", [PUMP, {"limit": 60, "commitment": "confirmed"}])
    for s in sigs:
        if s["err"]:
            continue
        _, r = rpc("getTransaction", [s["signature"], {"encoding": "base64", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}])
        if r is None:
            continue
        ver, ixs = top_level(base64.b64decode(r["transaction"][0]))
        names = ["%s/%d" % (DISC.get(d[:8].hex(), d[:8].hex()), na) for p, na, d in ixs if p == PUMP]
        print("trade", s["signature"], r["slot"], "v=%s" % ver, names or "(no top-level pump ix)")


def main():
    if "--discover" in sys.argv:
        discover()
        return
    force = "--force" in sys.argv
    apis = {n: fetch_api(n, p, force) for n, p in API}
    accts = {}
    for name, addr, note in ACCOUNTS:
        if not addr:
            addr = apis["coin_worked"]["result"]["bonding_curve"]
        accts[name] = fetch_account(name, addr, note, force)
    txs = {n: fetch_tx(n, s, k, force) for n, s, k in TXS}
    for n, s, _k in TXS:
        fetch_tx_json(n, s, force)
    fetch_jito(force)
    readme(txs, accts, apis)
    print("fixtures in", FIX)


if __name__ == "__main__":
    main()
