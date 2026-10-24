# Tweet data (fxtwitter, no key) for every tweet behind the universe's coins and his attempts -> univ/tweets/<id>.json
import json, os, time, urllib.request
from concurrent.futures import ThreadPoolExecutor
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
R = json.load(open(SP + "/univ/dataset.json"))
sids = sorted({r["sid"] for r in R if r["sid"] and (r["he"] or (r["dev"] >= 0.95 and r["delay"] is not None and r["delay"] < 3600))})
print("tweets to fetch:", len(sids), flush=True)
def get(sid):
    p = f"{SP}/univ/tweets/{sid}.json"
    if os.path.exists(p): return "have"
    for k in range(3):
        try:
            req = urllib.request.Request(f"https://api.fxtwitter.com/status/{sid}", headers={"User-Agent": "Mozilla/5.0 rugsnipe-research"})
            with urllib.request.urlopen(req, timeout=20) as r: j = json.load(r)
            t = j.get("tweet") or {}
            a = t.get("author") or {}
            q = t.get("quote") or {}
            qa = q.get("author") or {}
            keep = dict(code=j.get("code"), text=(t.get("text") or "")[:400], created=t.get("created_timestamp"), lang=t.get("lang"),
                        likes=t.get("likes"), retweets=t.get("retweets"), replies=t.get("replies"), views=t.get("views"),
                        replying_to=t.get("replying_to"), has_media=bool((t.get("media") or {}).get("all")),
                        author=dict(screen_name=a.get("screen_name"), followers=a.get("followers"), following=a.get("following"),
                                    tweets=a.get("tweets"), joined=a.get("joined"), verified=(a.get("verification") or {}).get("verified"),
                                    vtype=(a.get("verification") or {}).get("type")),
                        quote=dict(screen_name=qa.get("screen_name"), followers=qa.get("followers"), verified=(qa.get("verification") or {}).get("verified"),
                                   likes=q.get("likes"), views=q.get("views")) if q else None)
            json.dump(keep, open(p, "w")); return "ok"
        except urllib.error.HTTPError as e:
            if e.code == 404: json.dump(dict(code=404), open(p, "w")); return "404"
            time.sleep(2 * (k + 1))
        except Exception:
            time.sleep(2 * (k + 1))
    return "fail"
import collections
with ThreadPoolExecutor(3) as ex: res = list(ex.map(get, sids))
print(collections.Counter(res), flush=True)
