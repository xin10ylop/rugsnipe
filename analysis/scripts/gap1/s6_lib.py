"""Cached access to /coins?creator= pages written by s6_expand.py."""
from rugsnipe_analysis.cache import DEFAULT_CACHE, DiskCache

_c = DiskCache(DEFAULT_CACHE, "pumpapi")


def coins(w, max_pages=30):
    out = []
    for p in range(max_pages):
        hit = _c.get(f"creatorcoins50:{w}:{p}")
        if not hit:
            break
        out += hit["v"] or []
        if len(hit["v"] or []) < 50:
            break
    return out


def daily_counts(w, t0, t1):
    return [int((c["created_timestamp"] / 1000 - t0) // 86400) for c in coins(w)
            if t0 <= c["created_timestamp"] / 1000 <= t1 and c.get("creator") == w]
