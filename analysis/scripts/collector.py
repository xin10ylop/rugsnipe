"""Run the forward collector (PumpPortal + logsSubscribe -> analytics.db).

Examples
--------
    # here (container): 30 minutes into data/analytics.db, relay socket in /tmp
    .venv/bin/python -I analysis/scripts/collector.py --local --duration 1800 --relay /tmp/pp.sock

    # Ohio (systemd): config from /etc/rugsnipe/rugsnipe.toml, db in /var/lib/rugsnipe
    python3 -I analysis/scripts/collector.py --config /etc/rugsnipe/rugsnipe.toml

Prints a one-line status every minute and a JSON summary at the end (also stored as a
``box_events`` row of kind ``collector_summary``).
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from rugsnipe_analysis import store  # noqa: E402
from rugsnipe_analysis.collector import DEFAULT_RELAY, CollectorConfig, config_from_toml, run_collector  # noqa: E402
from rugsnipe_analysis.rpcbudget import load_config, validate  # noqa: E402


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--local", action="store_true", help="write to data/analytics.db (default: /var/lib/rugsnipe)")
    ap.add_argument("--db", help="explicit analytics.db path")
    ap.add_argument("--duration", type=float, default=None, help="stop after N seconds (default: run forever)")
    ap.add_argument("--config", help="rugsnipe TOML ([rpc_budget], [feeds.relay], optional [collector])")
    ap.add_argument("--relay", nargs="?", const=DEFAULT_RELAY, default=None,
                    help=f"relay raw PumpPortal frames on a Unix socket (default path {DEFAULT_RELAY})")
    ap.add_argument("--box", default=None, help="box id for box_events (default: local / config [box].id)")
    ap.add_argument("--funding-hops", type=int, default=None, help="funding trace depth for fresh signers")
    ap.add_argument("--tape-delay", type=float, default=None, help="seconds after the create (default 2100)")
    ap.add_argument("--cache-tx", action="store_true", help="also cache fetched create txs in data/cache/rpc")
    ap.add_argument("--summary-json", help="also write the final summary to this path")
    a = ap.parse_args()

    toml = load_config(a.config) if a.config else load_config()
    errs = validate(toml)
    if errs:
        sys.exit("config error: " + "; ".join(errs))
    db = a.db or (str(store.LOCAL_DB) if a.local else str(store.OHIO_DB))
    cfg = CollectorConfig(db_path=db, duration_s=a.duration, relay_socket=a.relay, config_path=a.config,
                          box=a.box or ((toml.get("box") or {}).get("id") if not a.local else None) or "local",
                          cache_tx=a.cache_tx)
    cfg = config_from_toml(cfg, toml)
    if a.funding_hops is not None:
        cfg.funding_hops = a.funding_hops
    if a.tape_delay is not None:
        cfg.tape_delay_s = a.tape_delay
    summary = run_collector(cfg)
    out = json.dumps(summary, indent=1, default=str)
    print(out)
    if a.summary_json:
        Path(a.summary_json).write_text(out)


if __name__ == "__main__":
    main()
