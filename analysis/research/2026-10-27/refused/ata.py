# Does he buy into his associated token account (Token-2022)? Check his recent buys' account lists.
import sys, json
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month")
import tri
sys.path.insert(0, '/home/user/rugsnipe/analysis/scripts/strategy')
import operators_refresh as orf, hashlib
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"
def pda(seeds, pid):
    for bump in range(255, -1, -1):
        h = hashlib.sha256(b"".join(seeds) + bytes([bump]) + orf.b58d(pid) + b"ProgramDerivedAddress").digest()
        if not orf._on_curve(h): return orf.b58e(h)
AT = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
for w in (A, "H9kuqGKNYgPFYCA17uUTkaqXGum7MK8y1GwbS7cxsjDg", "3yrJE1A8dgSuy5NLimG5GSYxA6wMkvnAYLD9Y4xR2fjJ", "JBokzLFRg8CEcThrjweWa9WoZoZPYitRSVffXuxdHHVu", "CxyKYmQoaYLCds6puxSbWndk6ktqax8PwAeq8enRzph8", "D5BDDkBtjjva35nwu45WfmZgytsog9GST2tHQaZUYuFX"):
    sigs = tri.rpc("getSignaturesForAddress", [w, {"limit": 40}], cache=False)
    n = ok = 0
    for s in sigs:
        if s.get("err"): continue
        tx = tri.rpc("getTransaction", [s["signature"], {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}], cache=False)
        if not tx: continue
        pb = [b for b in tx["meta"].get("postTokenBalances") or [] if b.get("owner") == w and b["mint"].endswith("pump")]
        for b in pb:
            keys = tx["transaction"]["message"]["accountKeys"]
            acc = keys[b["accountIndex"]]["pubkey"]
            ata = pda([orf.b58d(w), orf.b58d(b["programId"]), orf.b58d(b["mint"])], AT)
            n += 1; ok += (str(ata) == acc)
            if n <= 2: print(w[:8], b["mint"][:8], b["programId"][:6], "ata" if str(ata) == acc else "NOT ata", b["uiTokenAmount"]["amount"])
    print(w[:8], "token balances", n, "in the ATA", ok)
