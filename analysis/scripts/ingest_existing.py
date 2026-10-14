"""Load the pre-M0 datasets into analytics.db (idempotent; safe to re-run).

    .venv/bin/python -I analysis/scripts/ingest_existing.py            # data/analytics.db
    .venv/bin/python -I analysis/scripts/ingest_existing.py --db /var/lib/rugsnipe/analytics.db

Loads data/tokens_light.jsonl (D1), data/tokens_sample.jsonl (D2 + firstslot metrics),
data/funding_creators.jsonl (funding traces) and the labels.py address book.
"""
from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from rugsnipe_analysis import store  # noqa: E402
from rugsnipe_analysis.cache import REPO_ROOT  # noqa: E402


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--db", default=str(store.LOCAL_DB))
    ap.add_argument("--data-dir", default=str(REPO_ROOT / "data"))
    a = ap.parse_args()
    t0 = time.time()
    con = store.connect(a.db)
    loaded = store.ingest_existing(con, Path(a.data_dir))
    out = {"db": a.db, "schema_version": store.schema_version(con), "loaded": loaded,
           "row_counts": {k: v for k, v in store.counts(con).items() if v}, "seconds": round(time.time() - t0, 1)}
    con.close()
    print(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
