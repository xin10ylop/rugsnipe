#!/usr/bin/env python3
"""Verify every curated label on-chain and write labels_verified.json.

For each address:
  * getMultipleAccounts -> exists?, owner program, executable, lamports, data len
  * getSignaturesForAddress (archive endpoint; limit 1000 for CEX/gambling, 200
    otherwise) -> sample size, time span, tx rate per hour, last activity.
Verdict per category (see rugsnipe_analysis.labels module doc):
  confirmed | inconclusive | rejected   (+ "hot"/"cold" activity class for CEX)

Usage:  python3 -I scripts/verify_labels.py [--only cex]
"""
from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from rugsnipe_analysis import labels as L  # noqa: E402
from rugsnipe_analysis.funding import shared_client  # noqa: E402

TOKEN_PROGRAMS = {"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"}
JITO_TIP_PROGRAM = "T1pyyaTNZsKv2WcRAB8oVnk93mLJw2XzjtVYqCsaHqt"
WALLET_CATS = {"cex", "gambling", "bot_fee", "mev", "swap_service", "onramp", "institution"}
CUSTODIAN_CATS = {"cex", "gambling", "swap_service", "onramp", "institution"}


def fetch_accounts(addrs: list[str]) -> dict[str, dict | None]:
    rpc = shared_client()
    out: dict[str, dict | None] = {}
    for i in range(0, len(addrs), 10):  # publicnode answers "Request blocked" (-32602) for >10 keys
        chunk = addrs[i:i + 10]
        res = rpc.call("getMultipleAccounts", [chunk, {"encoding": "base64", "dataSlice": {"offset": 0, "length": 0}}])
        for a, acc in zip(chunk, (res or {}).get("value", [])):
            out[a] = acc
    return out


def data_len(acc: dict) -> int | None:
    # with dataSlice length 0 we don't get size; use 'space' if provided
    return acc.get("space")


def activity(addr: str, limit: int) -> dict:
    rpc = shared_client()
    try:
        sigs = rpc.get_signatures(addr, limit=limit)
    except Exception as e:  # noqa: BLE001
        return {"error": str(e)[:200]}
    if not sigs:
        return {"sig_sample": 0}
    newest, oldest = sigs[0].get("blockTime") or 0, sigs[-1].get("blockTime") or 0
    span = max(newest - oldest, 1)
    return {
        "sig_sample": len(sigs),
        "sample_capped": len(sigs) >= limit,
        "sample_span_hours": round(span / 3600, 2),
        "tx_per_hour": round(len(sigs) / (span / 3600), 1) if len(sigs) > 1 else 0.0,
        "last_activity_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(newest)) if newest else None,
        "hours_since_last": round((time.time() - newest) / 3600, 1) if newest else None,
        "failed_share": round(sum(1 for s in sigs if s.get("err")) / len(sigs), 3),
    }


def verdict(lab: L.Label, acc: dict | None, act: dict) -> tuple[str, str]:
    if acc is None:
        if act.get("sig_sample"):
            return "inconclusive", (f"account closed/drained to 0 lamports but has history "
                                    f"(last {act.get('last_activity_utc')})")
        return "rejected", "no account and no signature history (invalid / never used)"
    owner, exe = acc.get("owner"), acc.get("executable")
    bal = acc.get("lamports", 0) / 1e9
    if lab.category in WALLET_CATS:
        if owner in TOKEN_PROGRAMS:
            return "rejected", "account is a token mint/token account, not a wallet"
        if lab.category == "mev":
            return ("confirmed", "owned by Jito tip-payment program") if owner == JITO_TIP_PROGRAM else (
                "inconclusive", f"owner {owner}")
        if owner != L.SYSTEM_PROGRAM:
            return "inconclusive", f"not system-owned (owner {owner})"
        rate = act.get("tx_per_hour", 0) or 0
        hsl = act.get("hours_since_last")
        recent = hsl is not None and hsl < 24 * 7
        if lab.category in CUSTODIAN_CATS:
            if bal >= 500 or (rate >= 30 and recent and bal >= 5):
                return "confirmed", f"exchange-like: balance {bal:,.0f} SOL, {rate} tx/h"
            if act.get("sig_sample", 0) >= 200 and bal >= 50:
                return "inconclusive", f"moderate wallet: balance {bal:,.1f} SOL, {rate} tx/h"
            return "inconclusive", f"dormant/small: balance {bal:,.2f} SOL, {rate} tx/h, last {act.get('last_activity_utc')}"
        # bot_fee
        if rate >= 5 and recent:
            return "confirmed", f"active fee collector: {rate} tx/h"
        return "inconclusive", f"low activity: {rate} tx/h, last {act.get('last_activity_utc')}"
    # programs and protocol accounts
    if exe:
        return "confirmed", "executable program"
    if (act.get("tx_per_hour") or 0) >= 5:
        return "confirmed", f"active protocol account (owner {owner}, {act.get('tx_per_hour')} tx/h)"
    return "inconclusive", f"non-executable, owner {owner}, {act.get('tx_per_hour')} tx/h"


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--only", help="only this category")
    ap.add_argument("--only-new", action="store_true", help="skip addresses already in labels_verified.json")
    args = ap.parse_args()
    labs = [l for l in L.all_labels(include_rejected=True) if not args.only or l.category == args.only]
    if args.only_new:
        done = set(L._load_verified())
        labs = [l for l in labs if l.address not in done]
    print(f"verifying {len(labs)} labels", flush=True)
    accs = fetch_accounts([l.address for l in labs])
    out_path = Path(L.__file__).with_name("labels_verified.json")
    try:
        existing = json.loads(out_path.read_text()).get("addresses", {})
    except (OSError, json.JSONDecodeError):
        existing = {}
    for i, lab in enumerate(labs, 1):
        acc = accs.get(lab.address)
        is_prog = bool(acc and acc.get("executable"))
        act = {} if is_prog else activity(lab.address, 1000 if lab.category in CUSTODIAN_CATS else 200)
        v, why = verdict(lab, acc, act)
        existing[lab.address] = {
            "checked_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "exists": acc is not None,
            "owner": acc.get("owner") if acc else None,
            "executable": acc.get("executable") if acc else None,
            "balance_sol": round(acc.get("lamports", 0) / 1e9, 3) if acc else None,
            **act,
            "activity_class": ("hot" if (act.get("tx_per_hour") or 0) >= 60 else "warm" if (act.get("tx_per_hour") or 0) >= 1 else "cold")
            if lab.category in CUSTODIAN_CATS and acc else None,
            "verdict": v, "why": why,
        }
        print(f"[{i}/{len(labs)}] {lab.name:<32} {v:<12} {why}", flush=True)
    out_path.write_text(json.dumps({
        "generated_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "getMultipleAccounts + getSignaturesForAddress(limit 1000 CEX / 200 other) via api.mainnet-beta.solana.com",
        "addresses": existing}, indent=1, sort_keys=True))
    print("wrote", out_path)


if __name__ == "__main__":
    main()
