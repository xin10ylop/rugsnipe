#!/usr/bin/env python3
"""Log every TweetStream post with the millisecond it reached this host (research; it never trades).

One JSON line per post event (tweet/content and tweet/update) to --out:
  {"rx": receipt unix ms, "op": "content"|"update", "id": tweet id, "pub": publish unix ms (from the id),
   "author": handle, "text": ..., "kind": ..., "ref": {"type", "author", "text"} or null}
The key comes from TWEETSTREAM_API_KEY or the --key-name line (default TWEETSTREAM_KEY) of --api-keys and is never
printed; a second account's key runs a second logger (--key-name TWEETSTREAM_KEY2, its own --out).

    python3 -m venv ~/tsvenv && ~/tsvenv/bin/pip install 'websockets>=14'
    sudo systemd-run --unit tweetlog --property=Restart=always \\
         ~/tsvenv/bin/python -I ts_logger.py --api-keys /etc/rugsnipe/keys/api-keys.toml --out ~/tweetlog/tweets.jsonl
"""
import argparse, asyncio, json, os, random, re, sys, time

WS_URL = "wss://ws-global.tweetstream.io/ws"   # outside the USA (docs: connection-auth-heartbeat)
TWITTER_EPOCH_MS = 1288834974657


def read_key(path, name="TWEETSTREAM_KEY"):
    key = os.environ.get("TWEETSTREAM_API_KEY", "").strip()
    if key or not path:
        return key
    for line in open(path):
        m = re.match(r'\s*' + re.escape(name) + r'\s*=\s*"([^"]+)"', line)
        if m:
            return m.group(1).strip()
    return ""


def handle(a):
    a = a if isinstance(a, dict) else {}
    return str(a.get("handle") or a.get("username") or "").lstrip("@").lower()


def record(event, rx_ms):
    """The log line for a post event, or None for anything else."""
    if not isinstance(event, dict) or event.get("v") != 1 or event.get("t") != "tweet":
        return None
    op = event.get("op")
    if op not in ("content", "update"):
        return None
    d = event.get("d") or {}
    tid = str(d.get("tweetId", ""))
    if not tid.isdigit():
        return None
    ref = d.get("ref") if isinstance(d.get("ref"), dict) else None
    return {"rx": round(rx_ms), "op": op, "id": tid, "pub": (int(tid) >> 22) + TWITTER_EPOCH_MS,
            "author": handle(d.get("author")), "text": str(d.get("text") or "")[:600], "kind": d.get("kind"),
            "ref": {"type": ref.get("type"), "author": handle(ref.get("author")), "text": str(ref.get("text") or "")[:400]} if ref else None}


def say(*a):
    print(time.strftime("%H:%M:%S"), *a, file=sys.stderr, flush=True)


async def run(key, out_path, url):
    import websockets
    protocols = ["tweetstream.v1", f"tweetstream.auth.token.{key}"]
    attempt = 0
    with open(out_path, "a") as out:
        while True:
            opened = None
            try:
                async with websockets.connect(url, subprotocols=protocols) as ws:
                    opened = time.monotonic(); say("connected", url)
                    await ws.send(json.dumps({"op": "auth_ping"}))
                    async for raw in ws:
                        rx = time.time() * 1000          # before parsing: our own work is not in the latency
                        try:
                            ev = json.loads(raw)
                        except ValueError:
                            continue
                        rec = record(ev, rx)
                        if rec:
                            out.write(json.dumps(rec, ensure_ascii=False) + "\n"); out.flush()
                            if rec["op"] == "content":
                                say(f"post @{rec['author']} {rec['id']} {rec['rx'] - rec['pub']} ms")
                        elif isinstance(ev, dict) and ev.get("t") == "control":
                            say("control", ev.get("op"), json.dumps(ev.get("d") or {})[:200])
            except Exception as e:
                status = getattr(getattr(e, "response", None), "status_code", None)
                if status in (401, 403):
                    sys.exit(f"TweetStream refused the key (HTTP {status}): check the dashboard")
                say("closed:", type(e).__name__, str(e).replace(key, "<key>")[:160])
            if opened is not None and time.monotonic() - opened >= 30:
                attempt = 0
            delay = min(30, 2 ** attempt) + random.random() / 4; attempt = min(attempt + 1, 5)
            await asyncio.sleep(delay)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--api-keys", default="/etc/rugsnipe/keys/api-keys.toml")
    ap.add_argument("--out", required=True)
    ap.add_argument("--url", default=WS_URL)
    ap.add_argument("--key-name", default="TWEETSTREAM_KEY")
    a = ap.parse_args()
    key = read_key(a.api_keys, a.key_name)
    if not key:
        sys.exit(f"no key: set TWEETSTREAM_API_KEY or add {a.key_name} = \"...\" to {a.api_keys}")
    os.makedirs(os.path.dirname(os.path.abspath(a.out)), exist_ok=True)
    asyncio.run(run(key, a.out, a.url))


if __name__ == "__main__":
    main()
