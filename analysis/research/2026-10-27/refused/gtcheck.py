# Ghost-target survivors, stage 2a: each one's own recent transactions. How many pump launch buys a day, how its token
# account is opened (ATA, a seed from the mint = predictable; random = not), and how it sends (tip routes).
#   python3 -I gtcheck.py WALLET...
import sys, json, collections, time
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month")
import tri
TIPS = {"jito": {"96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5", "HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe", "Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY",
                 "ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49", "DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh", "ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt",
                 "DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL", "3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT"}}
for w in sys.argv[1:]:
    sigs = []; before = None; lo = time.time() - 86400
    while True:
        o = {"limit": 1000}
        if before: o["before"] = before
        pg = tri.rpc("getSignaturesForAddress", [w, o], cache=False) or []
        sigs += [x for x in pg if (x.get("blockTime") or 0) >= lo]
        if len(pg) < 1000 or (pg[-1].get("blockTime") or 0) < lo: break
        before = pg[-1]["signature"]
    ok = [x for x in sigs if not x.get("err")]
    kinds = collections.Counter(); route = collections.Counter(); n = 0; sizes = []
    for x in ok[:60]:
        tx = tri.rpc("getTransaction", [x["signature"], {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}], cache=False)
        if not tx or "Instruction: Buy" not in " ".join(tx["meta"]["logMessages"] or []): continue
        allix = tx["transaction"]["message"]["instructions"] + [i for g in tx["meta"].get("innerInstructions") or [] for i in g["instructions"]]
        mint = next((b["mint"] for b in tx["meta"].get("postTokenBalances") or [] if b.get("owner") == w), None)
        if not mint: continue
        n += 1
        cs = [i["parsed"]["info"] for i in allix if isinstance(i.get("parsed"), dict) and i["parsed"].get("type") == "createAccountWithSeed"]
        ata = [i for i in allix if isinstance(i.get("parsed"), dict) and i["parsed"].get("type") in ("create", "createIdempotent")]
        if cs: kinds["seed = mint[:%d]" % len(cs[0]["seed"]) if mint.startswith(cs[0]["seed"]) else "random seed"] += 1
        elif ata: kinds["ATA"] += 1
        else: kinds["account already open / other"] += 1
        for i in allix:
            p = i.get("parsed")
            if isinstance(p, dict) and p.get("type") == "transfer" and p["info"].get("source") == w:
                d = p["info"]["destination"]; lam = p["info"]["lamports"]
                if lam < 50_000_000: route["jito" if d in TIPS["jito"] else d[:6]] += 1
    print(f"{w[:8]}: {len(sigs)} txs in 24 h ({len(ok)} landed); of 60 read, {n} pump buys; account {dict(kinds)}; tips to {route.most_common(4)}")
