# The seed each copy_more wallet uses for its token account (createAccountWithSeed: address = sha256(base, seed, owner)),
# over its recent pump buys: is it a fixed-length prefix of the mint (predictable) or random?
import sys, json, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month")
import tri
W = {"JBokzLFR": "JBokzLFRg8CEcThrjweWa9WoZoZPYitRSVffXuxdHHVu", "3yrJE1A8": "3yrJE1A8dgSuy5NLimG5GSYxA6wMkvnAYLD9Y4xR2fjJ",
     "H9kuqGKN": "H9kuqGKNYgPFYCA17uUTkaqXGum7MK8y1GwbS7cxsjDg", "CxyKYmQo": "CxyKYmQoaYLCds6puxSbWndk6ktqax8PwAeq8enRzph8",
     "D5BDDkBt": "D5BDDkBtjjva35nwu45WfmZgytsog9GST2tHQaZUYuFX"}
for k, w in W.items():
    sigs = tri.rpc("getSignaturesForAddress", [w, {"limit": 150}], cache=False)
    kinds = collections.Counter(); ex = []
    for s in sigs:
        if s.get("err") or len(ex) >= 12: continue
        tx = tri.rpc("getTransaction", [s["signature"], {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}], cache=False)
        if not tx or "Instruction: Buy" not in " ".join(tx["meta"]["logMessages"] or []): continue
        allix = tx["transaction"]["message"]["instructions"] + [i for g in tx["meta"].get("innerInstructions") or [] for i in g["instructions"]]
        mint = next((b["mint"] for b in tx["meta"].get("postTokenBalances") or [] if b.get("owner") == w and b["mint"].endswith("pump")), None)
        if not mint: continue
        cs = [i["parsed"]["info"] for i in allix if isinstance(i.get("parsed"), dict) and i["parsed"].get("type") == "createAccountWithSeed"]
        ata = [i for i in allix if isinstance(i.get("parsed"), dict) and i["parsed"].get("type") in ("create", "createIdempotent") and i["parsed"]["info"].get("mint") == mint]
        if cs:
            sd = cs[0]["seed"]; kinds["seed = mint[:%d]" % len(sd) if mint.startswith(sd) else "seed random (%d)" % len(sd)] += 1
        elif ata: kinds["ATA"] += 1
        else: kinds["existing account / other"] += 1
        ex.append(mint)
    print(k, dict(kinds))
