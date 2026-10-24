#!/usr/bin/env python3
"""Join the TweetStream log (ts_logger.py) with the bot's tweet-check journal lines: how fast posts reach this box
from their publish time, and whether each post was here before the bot saw the creates of the coins on it.

    sudo journalctl -u rugsnipe-hot -o short-unix --since "2026-10-24 19:00" | grep " tweet " > ~/tweetlog/verdicts.txt
    python3 -I ts_lead.py ~/tweetlog/tweets.jsonl ~/tweetlog/verdicts.txt

A verdict line: "<unix.us> <host> rugsnipe-hot[pid]: tweet <mint8> <kind> rank R age As sid S fetch Fms verdict Vms
slot+K ..."; the bot saw the create at (journal time - V ms). Lead = create seen - post received (> 0: the post was
here first, so a feed could have told the bot about the tweet before the coin existed).
"""
import collections, json, re, statistics, sys

LINE = re.compile(r"^(\d+\.\d+) \S+ \S+: tweet (\S{8}) (\w+) rank (\d+) age (-?\d+)s sid (\d+) fetch (\d+)ms verdict (\d+)ms slot\+(\d+)")


def load_posts(path):
    posts = {}
    for line in open(path):
        try:
            r = json.loads(line)
        except ValueError:
            continue
        if r.get("op") == "content" and r["id"] not in posts:
            posts[r["id"]] = r
    return posts


def load_verdicts(path):
    out = []
    for line in open(path):
        m = LINE.match(line.strip())
        if m:
            ts, k8, kind, rank, age, sid, fetch, vms, slot = m.groups()
            out.append(dict(seen=float(ts) * 1000 - int(vms), mint8=k8, kind=kind, rank=int(rank), sid=sid))
    return out


def pct(v, p):
    v = sorted(v)
    return v[min(len(v) - 1, int(p * len(v)))] if v else None


def report(posts, verdicts):
    lat = collections.defaultdict(list)
    for r in posts.values():
        lat[r["author"]].append(r["rx"] - r["pub"])
    print(f"posts logged: {len(posts)}")
    for a, v in sorted(lat.items(), key=lambda kv: -len(kv[1])):
        print(f"   @{a:18s} {len(v):4d} posts  publish -> here p50 {pct(v, .5)} ms  p95 {pct(v, .95)} ms")
    on = [dict(v, lead=v["seen"] - posts[v["sid"]]["rx"], author=posts[v["sid"]]["author"]) for v in verdicts if v["sid"] in posts]
    print(f"bot verdicts: {len(verdicts)}; coins on a logged post: {len(on)}")
    for lab, g in (("first on their tweet (rank 1)", [x for x in on if x["rank"] == 1]), ("copies (rank 2+)", [x for x in on if x["rank"] > 1])):
        if not g:
            continue
        leads = [x["lead"] for x in g]
        print(f"   {lab:30s} {len(g):4d} coins  post here before the create: {100 * sum(l > 0 for l in leads) / len(leads):3.0f}%  "
              f"lead p10 {pct(leads, .1):.0f} ms  p50 {pct(leads, .5):.0f} ms")
    return on


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    report(load_posts(sys.argv[1]), load_verdicts(sys.argv[2]))
