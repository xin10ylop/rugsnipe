# Fetch the metadata JSON of every launch-tool create in the window (and every launch he attempted), store raw.
import json, sys, os, hashlib, time, urllib.request, calendar
from urllib.parse import urlparse
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
T0 = int(sys.argv[1])
U = json.load(open(f"{SP}/univ/creates_{T0}.json"))
LH = json.load(open(SP + "/today/legs_him.json")); h_att = {x["mint"] for x in LH if x["kind"] == "buy" and x["mint"]}
TOOLS = {"metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io"}
todo = [c for c in U if urlparse(c["uri"]).netloc in TOOLS or c["mint"] in h_att]
ok = 0
for c in todo:
    p = f"{SP}/univ/meta/{c['mint']}.json"
    if os.path.exists(p): ok += 1; continue
    try:
        req = urllib.request.Request(c["uri"], headers={"User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(req, timeout=15) as r: body = r.read(200_000)
        json.dump(dict(uri=c["uri"], body=body.decode("utf-8", "replace")), open(p, "w")); ok += 1
    except Exception as e:
        json.dump(dict(uri=c["uri"], error=str(e)[:120]), open(p, "w"))
    time.sleep(0.05)
print("fetched", ok, "of", len(todo))
