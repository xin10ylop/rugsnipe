#!/usr/bin/env python3
"""How right the tweet feed's copy call is, and how right it would be if a copy needed a link (2026-10-25).

The feed calls a coin a copy when every post its ticker or name matches was matched by an earlier coin's text; on
Oct 25 only 44 of 116 such calls were copies by the tweet check. The tighter call: the matched post must be a tweet an
earlier coin already LINKS in its metadata (a tweet check verdict with that sid came before this coin). Reads the
hot unit's journal and prints both against the tweet check's own verdict on the same coin.

    sudo journalctl -u rugsnipe-hot -o short-unix --since today | python3 -I feed_linked.py
"""
import collections
import re
import sys

FEED = re.compile(r"^(\d+\.\d+) \S+ \S+: tweetfeed (\S{8}) (\w+) posts (\d+) post (\d+) age (-?\d+)ms")
TWEET = re.compile(r"^(\d+\.\d+) \S+ \S+: tweet (\S{8}) (\w+) rank (\d+) age (-?\d+)s sid (\d+)")


def read(lines):
    feed, verdict = {}, {}
    book = []  # (time, sid) of every verdict that names a tweet
    for line in lines:
        m = FEED.match(line)
        if m:
            t, k8, call, n, post, age = m.groups()
            feed.setdefault(k8, dict(t=float(t), call=call, n=int(n), post=int(post)))
            continue
        m = TWEET.match(line)
        if m:
            t, k8, kind, rank, age, sid = m.groups()
            verdict.setdefault(k8, dict(t=float(t), kind=kind, rank=int(rank), sid=int(sid)))
            if int(sid):
                book.append((float(t), int(sid)))
    return feed, verdict, sorted(book)


def linked(f, book):
    """The feed's post is a tweet an earlier coin links (its verdict came before this coin's feed call)."""
    return any(sid == f["post"] and t < f["t"] for t, sid in book)


def main():
    feed, verdict, book = read(sys.stdin)
    rows = [(k8, f, verdict.get(k8)) for k8, f in feed.items() if f["n"] > 0]
    print(f"coins with a matched post: {len(rows)} (feed lines {len(feed)}, tweet verdicts {len(verdict)})")
    truth = lambda v: "no verdict" if not v else v["kind"]
    for lab, sel in (("text copy call (now)", lambda k8, f, v: f["call"] == "copy"),
                     ("linked copy (the matched post is an earlier coin's tweet)", lambda k8, f, v: linked(f, book)),
                     ("text copy AND linked", lambda k8, f, v: f["call"] == "copy" and linked(f, book))):
        g = [(k8, f, v) for k8, f, v in rows if sel(k8, f, v)]
        c = collections.Counter(truth(v) for _, _, v in g)
        right = c.get("copy", 0)
        print(f"   {lab:58s}: {len(g):4d} calls, tweet check says {dict(c)}; right {100 * right / max(1, len(g)):.0f}%")
    copies = sum(1 for _, _, v in rows if v and v["kind"] == "copy")
    print(f"   copies by the tweet check among the matched coins: {copies}")


if __name__ == "__main__":
    main()
