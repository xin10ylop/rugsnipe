"""Tiny JSON disk cache (one file per key, sharded by hash prefix)."""
from __future__ import annotations

import hashlib
import json
import os
import tempfile
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_CACHE = REPO_ROOT / "data" / "cache"


class DiskCache:
    """Key -> JSON value store. Writes are atomic (tmp file + rename)."""

    def __init__(self, root: Path | str, namespace: str) -> None:
        self.root = Path(root) / namespace
        self.root.mkdir(parents=True, exist_ok=True)

    def _path(self, key: str) -> Path:
        h = hashlib.sha1(key.encode()).hexdigest()
        safe = "".join(c if c.isalnum() else "_" for c in key)[:80]
        return self.root / h[:2] / f"{safe}.{h[:10]}.json"

    def get(self, key: str) -> Any | None:
        p = self._path(key)
        if not p.exists():
            return None
        try:
            with p.open() as f:
                return json.load(f)
        except (OSError, json.JSONDecodeError):
            return None

    def has(self, key: str) -> bool:
        return self._path(key).exists()

    def put(self, key: str, value: Any) -> None:
        p = self._path(key)
        p.parent.mkdir(parents=True, exist_ok=True)
        fd, tmp = tempfile.mkstemp(dir=p.parent, suffix=".tmp")
        with os.fdopen(fd, "w") as f:
            json.dump(value, f, separators=(",", ":"))
        os.replace(tmp, p)
