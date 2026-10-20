#!/usr/bin/env python3
"""Build an UNSIGNED ArmSet JSON for paper testing of rugsnipe-hot.

Sign it afterwards with ``rugsnipe-ctl armset sign --key <dev key> --in <json>``
(the dev signing key stays outside git; its public key goes into
``config/paper.example.toml``). Paper ArmSets never carry ``gate_passed = true``.

Two kinds:

* ``operators`` (default): 20 repeat creators (>= 3 launches) from
  ``data/funding_creators.jsonl`` (``prior_coins_same_wallet >= 2``, funding
  chain resolved), topped up from the wallet operators of
  ``data/reports/gap1_operators.json``. Each operator gets its creator wallet
  as a ``signer`` watch key and, when its direct funder is known, a funding
  rule for that funder with the funded amount as a +-0.0005 SOL band
  (docs/06 section 9.2/9.6). Size 0.1 SOL, v_max 40 SOL.
* ``cex-broad``: a broad *test* watchlist so that paper decisions actually
  fire: one operator whose funding rules are the high-confidence CEX hot
  wallets of ``analysis/rugsnipe_analysis/labels.py`` that fund pump creators
  most often (``data/analytics.db`` ``funding`` table), any amount in
  [0.05, 50] SOL to a fresh wallet. This is NOT a strategy; it exercises the
  funding watcher -> dynamic watchlist -> decision path.
* ``class``: the lead policy's universe (docs/04 section 2) is a class of
  launches, not a list of operators: one ``shadow`` operator (0.5 SOL,
  ``v_max`` 115 SOL) and ``match_all`` set to it, so every create the
  watchlist does not know maps to it; ``[entry] filter = "multi_bundle"`` in
  ``config/paper-lead.example.toml`` keeps the multi-wallet bundled ones.

Usage::

    python -I analysis/scripts/make_paper_armset.py --kind operators --ver 1 --out /tmp/x/ops.json
    python -I analysis/scripts/make_paper_armset.py --kind cex-broad --ver 2 --out /tmp/x/cex.json
    python -I analysis/scripts/make_paper_armset.py --kind class --ver 3 --out /tmp/x/class.json
    python -I analysis/scripts/make_paper_armset.py --kind class --ver 4 --probe-allowed --probe-stage a --valid-hours 264 --out /tmp/x/probe-a.json

``--probe-allowed`` (class kind only) writes the probe ArmSet of docs/04 section 7 step 2 and
docs/08 section 9: ``probe_allowed = true``, ``probe_stage`` ``a`` (default; 0.02 SOL measurement) or
``b`` (0.25 SOL), the class operator in state ``probe`` at the stage's size. The hot box refuses a
probe whose ArmSet names a stage other than ``[probe] stage`` in its config, and stage ``b`` with an ArmSet
that names no stage, so moving to B takes a signed ArmSet and the local config. It never sets ``gate_passed``. A probe also needs every risk cap, keys, a post-execution feed
and a build with real senders (docs/06 section 6, docs/08).

``--valid-hours N`` (default 6) sets ``valid_until = created_at + N * 3600``. The hot box also refuses an ArmSet
older than ``[risk] armset_max_age_s`` (default 21,600 s = 6 h), and a running probe drops to paper once its ArmSet
is stale, so a probe stage is signed ONCE for the whole stage: stage A uses ``--valid-hours 264`` (11 days)
together with ``[risk] armset_max_age_s = 950_400`` in the probe config (config/probe-a.example.toml; readiness
audit 2026-10-20 M1, docs/08 section 9 step 1).
"""
from __future__ import annotations

import argparse
import json
import sqlite3
import sys
import time
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "analysis"))

LAMPORTS = 1_000_000_000
BAND = 500_000  # +-0.0005 SOL
SIZE = 100_000_000  # 0.1 SOL
V_MAX = 40 * LAMPORTS
CLASS_SIZE = 500_000_000  # 0.5 SOL (docs/04 section 4 cap)
CLASS_V_MAX = 115 * LAMPORTS  # no entry cap beyond slot-0 graduation (gap-3)
PROBE_SIZE = {"a": 20_000_000, "b": 250_000_000}  # 0.02 / 0.25 SOL per stage (docs/04 section 7)


def op_entry(op_id: int, family: str, stats: dict) -> dict:
    return {
        "op_id": op_id,
        "state": "shadow",
        "size_lamports": SIZE,
        "v_max_lamports": V_MAX,
        "ev_cap_lamports": 0,
        "ab_skip_p": 0.0,
        "family": family,
        "stats": stats,
    }


def funding_rule(funder: str, bands: list[list[int]], op_id: int, rule_id: int) -> dict:
    return {
        "funder": funder,
        "bands": bands[:5],
        "fresh_only": True,
        "ttl_s": 1800,
        "remove_if_no_launch_s": 3600,
        "last_transfer": True,
        "op_id": op_id,
        "rule_id": rule_id,
    }


def operators(n: int) -> tuple[list, list, list]:
    from rugsnipe_analysis import labels

    rows = [json.loads(l) for l in open(ROOT / "data/funding_creators.jsonl")]
    seen: set[str] = set()
    picked = []
    for r in sorted(rows, key=lambda r: -int(r.get("prior_coins_same_wallet") or 0)):
        w = r["creator"]
        if w in seen or int(r.get("prior_coins_same_wallet") or 0) < 2:
            continue
        seen.add(w)
        f = r.get("funding") or {}
        funder = f.get("funder") if f.get("status") == "ok" else None
        amount = f.get("amount_sol")
        picked.append({
            "wallet": w,
            "launches": int(r["prior_coins_same_wallet"]) + 1,
            "funder": funder,
            "amount_lamports": int(round(amount * LAMPORTS)) if funder and amount else None,
            "funder_label": (labels.get_label(funder).name if funder and labels.get_label(funder) else None),
            "source": "funding_creators.jsonl",
        })
    gap1 = json.load(open(ROOT / "data/reports/gap1_operators.json"))
    for o in sorted(gap1["per_operator"], key=lambda o: -o["N"]):
        if len(picked) >= n:
            break
        if o["type"] not in ("wallet-expanded", "wallet-sample") or o["N"] < 3:
            continue
        w = o["op"].split(":", 1)[1]
        if w in seen:
            continue
        seen.add(w)
        picked.append({"wallet": w, "launches": o["N"], "funder": None, "amount_lamports": None,
                       "funder_label": None, "source": "gap1_operators.json " + o["type"]})
    picked = picked[:n]
    ops, watch, rules = [], [], []
    for i, p in enumerate(picked, start=1):
        ops.append(op_entry(i, "repeat-creator", {"launches": p["launches"], "source": p["source"],
                                                   "funder_label": p["funder_label"]}))
        watch.append({"key": p["wallet"], "kind": "signer", "op_id": i, "rule_id": 0})
        if p["funder"] and p["amount_lamports"]:
            a = p["amount_lamports"]
            rules.append(funding_rule(p["funder"], [[a - BAND, a + BAND]], i, i))
    return ops, watch, rules


def cex_broad(n_funders: int) -> tuple[list, list, list]:
    from rugsnipe_analysis import labels

    counts: Counter[str] = Counter()
    db = ROOT / "data/analytics.db"
    if db.exists():
        c = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
        for (funder, n) in c.execute(
                "SELECT funder, COUNT(*) FROM funding WHERE funder IS NOT NULL GROUP BY funder"):
            counts[funder] = n
    cex = [l for l in labels.all_labels() if l.category == "cex" and l.confidence == "high"]
    cex.sort(key=lambda l: (-counts.get(l.address, 0), l.name))
    chosen, entities = [], set()
    for l in cex:
        if len(chosen) >= n_funders:
            break
        if counts.get(l.address, 0) == 0 and len(chosen) >= n_funders // 2:
            continue
        if l.entity in entities and counts.get(l.address, 0) == 0:
            continue
        entities.add(l.entity)
        chosen.append(l)
    ops = [op_entry(1, "cex-fresh-broad-test", {"funders": [l.name for l in chosen],
                                                 "creators_funded_in_db": {l.name: counts.get(l.address, 0) for l in chosen}})]
    rules = [funding_rule(l.address, [[50_000_000, 50 * LAMPORTS]], 1, k + 1)
             for k, l in enumerate(chosen)]
    return ops, [], rules


def class_arm() -> tuple[list, list, list]:
    op = op_entry(1, "class-multi-bundle", {"policy": "docs/04 section 2 rank-1 micro-scalp",
                                            "entry_filter": "multi_bundle"})
    op["size_lamports"] = CLASS_SIZE
    op["v_max_lamports"] = CLASS_V_MAX
    return [op], [], []


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--kind", choices=["operators", "cex-broad", "class"], default="operators")
    ap.add_argument("--ver", type=int, required=True)
    ap.add_argument("--n", type=int, default=20, help="operators (operators) or funders (cex-broad)")
    ap.add_argument("--out", required=True)
    ap.add_argument("--probe-allowed", action="store_true",
                    help="class kind: probe_allowed = true, operator state probe at the --probe-stage size (docs/04 section 7)")
    ap.add_argument("--probe-stage", choices=["a", "b"], default="a",
                    help="with --probe-allowed: a = 0.02 SOL measurement, b = 0.25 SOL (docs/04 section 7)")
    ap.add_argument("--regime", default="paper", help="ArmSet regime (rugsnipe-hot --regime must match)")
    ap.add_argument("--valid-hours", type=int, default=6,
                    help="valid_until = created_at + N hours (default 6; probe stage A: 264 = 11 days, with "
                         "[risk] armset_max_age_s = 950_400 in the probe config)")
    a = ap.parse_args()
    if a.probe_allowed and a.kind != "class":
        ap.error("--probe-allowed applies to --kind class only")
    if a.valid_hours < 1:
        ap.error("--valid-hours must be >= 1")
    if a.kind == "operators":
        ops, watch, rules = operators(a.n)
    elif a.kind == "class":
        ops, watch, rules = class_arm()
    else:
        ops, watch, rules = cex_broad(min(a.n, 10))
    now = int(time.time())
    armset = {
        "ver": a.ver,
        "run_id": f"paper-{a.kind}-{time.strftime('%Y%m%dT%H%M%SZ', time.gmtime(now))}",
        "code_sha": "",
        "prereg_sha": "",
        "created_at": now,
        "valid_until": now + a.valid_hours * 3600,
        "regime": a.regime,
        "min_hot_version": "0.1.0",
        "gate_passed": False,
        "shared_fee_payers": [],
        "operators": ops,
        "watch": watch,
        "funding_rules": rules,
    }
    if a.kind == "class":
        armset["match_all"] = ops[0]["op_id"]
    if a.probe_allowed:
        armset["probe_allowed"] = True
        armset["probe_stage"] = a.probe_stage
        ops[0]["state"] = "probe"
        ops[0]["size_lamports"] = PROBE_SIZE[a.probe_stage]
    Path(a.out).parent.mkdir(parents=True, exist_ok=True)
    Path(a.out).write_text(json.dumps(armset, indent=1, sort_keys=True) + "\n")
    print(f"{a.out}: {len(ops)} operator(s), {len(watch)} watch key(s), {len(rules)} funding rule(s), "
          f"valid {a.valid_hours} h (until {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime(armset['valid_until']))})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
