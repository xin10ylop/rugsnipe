# Every signature of a wallet back to T0 (landed and failed), oldest last -> h9long/<w8>_sigs.json
import sys, json, time
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month")
import tri
W = sys.argv[1]; T0 = int(sys.argv[2])
out, before = [], None
while True:
    o = {"limit": 1000}
    if before: o["before"] = before
    page = tri.rpc("getSignaturesForAddress", [W, o], cache=False) or []
    out += [[x["signature"], x["slot"], x.get("blockTime") or 0, x.get("err") is not None] for x in page]
    if not page: break
    print(time.strftime("%H:%M:%S"), len(out), page[-1].get("blockTime"), flush=True)
    if len(page) < 1000 or (page[-1].get("blockTime") or 0) < T0: break
    before = page[-1]["signature"]
out = [x for x in out if x[2] >= T0]
json.dump(out, open(f"{SP}/h9long/{W[:8]}_sigs.json", "w"))
print("total", len(out), "landed", sum(1 for x in out if not x[3]), "oldest", min(x[2] for x in out))
