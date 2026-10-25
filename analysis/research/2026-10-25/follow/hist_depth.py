# How far back the copied wallet goes: its signatures before the month already pulled (before Sep 24), counted per day,
# kept with slot and error flag. -> hist/older_sigs.json [[slot, blockTime, failed, sig], ...]
import sys, json, time, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); import tri
tri.MIN_GAP = 0.02
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"
rows = [json.loads(l) for l in open(SP + "/month/AHwJSnFN.jsonl")]
oldest = min(rows, key=lambda r: r["s"])["sig"]
out = []; before = oldest; pages = 0
while pages < 400:
    s = tri.rpc("getSignaturesForAddress", [A, {"limit": 1000, "before": before}], cache=False) or []
    pages += 1
    out += [[x["slot"], x.get("blockTime"), x.get("err") is not None, x["signature"]] for x in s]
    if len(s) < 1000: break
    before = s[-1]["signature"]
    if pages % 20 == 0: print(pages, time.strftime("%Y-%m-%d", time.gmtime(s[-1]["blockTime"])), flush=True)
json.dump(out, open(SP + "/dec/hist/older_sigs.json", "w"))
d = collections.Counter(time.strftime("%Y-%m", time.gmtime(x[1])) for x in out if x[1])
print("pages", pages, "sigs", len(out), "by month", dict(sorted(d.items())), "first", time.strftime("%Y-%m-%d", time.gmtime(out[-1][1])) if out else None)
