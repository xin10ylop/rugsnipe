"""Stage 1: every signature of the pump.fun mint authority over the last N days."""
import sys
import time
from common import OUT, jdump, log, rpc
from rugsnipe_analysis.events import PUMP_MINT_AUTHORITY

days = float(sys.argv[1]) if len(sys.argv) > 1 else 16
c = rpc(4.0)
anchor = int(time.time())
stop = anchor - int(days * 86400)
sigs = []
before = None
pages = 0
while True:
    opts = {"limit": 1000}
    if before:
        opts["before"] = before
    page = c.call("getSignaturesForAddress", [PUMP_MINT_AUTHORITY, opts],
                  cache_key=(f"sigs:{PUMP_MINT_AUTHORITY}:{before}:None:1000" if before else None),
                  endpoints=("https://api.mainnet-beta.solana.com",))
    pages += 1
    if not page:
        break
    for s in page:
        sigs.append([s["signature"], s["slot"], s.get("blockTime"), s.get("err") is not None])
    before = page[-1]["signature"]
    oldest = page[-1].get("blockTime") or anchor
    if pages % 25 == 0:
        log(f"{pages} pages {len(sigs)} sigs oldest {(anchor-oldest)/86400:.2f} d")
        jdump(OUT / "index_partial.json", {"anchor": anchor, "sigs": sigs})
    if oldest < stop:
        break
sigs = [s for s in sigs if s[2] and s[2] >= stop]
jdump(OUT / "index.json", {"anchor": anchor, "days": days, "pages": pages, "sigs": sigs})
log(f"done {len(sigs)} sigs, {pages} pages")
