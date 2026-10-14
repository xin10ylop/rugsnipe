"""Create transaction -> analytics.db rows (``creates`` + ``create_buys``).

Two inputs:

* :func:`rows_from_tx` -- a confirmed jsonParsed ``getTransaction`` result.  Reuses
  :func:`rugsnipe_analysis.dataset.creation_record` (events, launcher, dev buys) and
  adds the transaction-level fields of section 5.2: every signer, fee payer, tx
  version and index, priority fee and Jito-style tips, CU used, the ``create_v2``
  arguments (``is_cashback``), and the **owner of the token account credited by
  each in-tx buy** (which may differ from the buy's signer).
* :func:`rows_from_logs` -- a ``logsSubscribe`` notification (``Program data:``
  CreateEvent + TradeEvents, decoded by :mod:`events`).  Fee payer, signers and
  owners are unknown there (free mode, section 7.1).

All amounts are integer lamports / raw token units.
"""
from __future__ import annotations

import struct
import time
from typing import Any

from . import events as E
from . import labels as L
from .dataset import creation_record
from .funding import account_keys, signers, sol_transfers
from .store import launcher_of, regime_for, uri_host

DISC_CREATE_V2 = bytes.fromhex("d6904cec5f8b31b4")
DISC_CREATE = bytes.fromhex("181ec828051c0777")
BASE_FEE_PER_SIG = 5_000


class _Prefetched:
    """Adapter so dataset.creation_record() can decode an already fetched transaction."""

    def __init__(self, tx: dict | None) -> None:
        self.tx = tx

    def get_transaction(self, sig: str) -> dict | None:  # noqa: ARG002 - interface
        return self.tx


def _iter_ix(tx: dict):
    msg = (tx.get("transaction") or {}).get("message") or {}
    for ix in msg.get("instructions") or []:
        yield ix
    for grp in (tx.get("meta") or {}).get("innerInstructions") or []:
        for ix in grp.get("instructions") or []:
            yield ix


def parse_create_args(data: bytes) -> dict[str, Any] | None:
    """Arguments of pump ``create_v2`` / legacy ``create`` instruction data (None otherwise).

    create_v2: name, symbol, uri: string; creator: pubkey; is_mayhem_mode: bool;
    [is_cashback_enabled: bool, ...] (trailing fields are optional on chain).
    legacy create: name, symbol, uri, creator.
    """
    disc = data[:8]
    if disc not in (DISC_CREATE_V2, DISC_CREATE):
        return None
    o = 8
    out: dict[str, Any] = {"ix_variant": "create_v2" if disc == DISC_CREATE_V2 else "create"}
    try:
        for k in ("name", "symbol", "uri"):
            n = struct.unpack_from("<I", data, o)[0]
            o += 4
            out[k] = data[o:o + n].decode("utf-8", "replace")
            o += n
        if len(data) >= o + 32:
            out["creator"] = E.b58encode(data[o:o + 32])
            o += 32
        if disc == DISC_CREATE_V2:
            out["is_mayhem"] = bool(data[o]) if len(data) > o else None
            o += 1
            out["is_cashback"] = bool(data[o]) if len(data) > o else None
    except (struct.error, IndexError):
        return out
    return out


def create_args(tx: dict) -> dict[str, Any] | None:
    """Decoded arguments of the first pump create instruction in ``tx`` (top level or CPI)."""
    for ix in _iter_ix(tx):
        if ix.get("programId") != E.PUMP_PROGRAM or "data" not in ix:
            continue
        try:
            args = parse_create_args(E.b58decode(ix["data"]))
        except KeyError:
            continue
        if args:
            return args
    return None


def priority_lamports(tx: dict) -> int | None:
    """Fee above the base fee (5000 lamports per signature); valid for legacy, v0 and v1."""
    meta = tx.get("meta") or {}
    sigs = (tx.get("transaction") or {}).get("signatures") or []
    if meta.get("fee") is None or not sigs:
        return None
    return max(0, int(meta["fee"]) - BASE_FEE_PER_SIG * len(sigs))


def tips(tx: dict) -> tuple[int, str | None]:
    """(lamports, recipient) of SOL transfers to MEV tip accounts (labels category 'mev')."""
    total, rcpt = 0, None
    for t in sol_transfers(tx):
        lab = L.get_label(t.destination)
        if lab is not None and lab.category == "mev":
            total += t.lamports
            rcpt = rcpt or t.destination
    return total, rcpt


def token_credits(tx: dict, mint: str) -> list[tuple[str, int]]:
    """[(owner, raw increase)] of token accounts of ``mint`` whose balance grew in ``tx``."""
    meta = tx.get("meta") or {}
    pre = {(b.get("accountIndex")): int(b["uiTokenAmount"]["amount"])
           for b in meta.get("preTokenBalances") or [] if b.get("mint") == mint}
    out = []
    for b in meta.get("postTokenBalances") or []:
        if b.get("mint") != mint:
            continue
        d = int(b["uiTokenAmount"]["amount"]) - pre.get(b.get("accountIndex"), 0)
        if d > 0 and b.get("owner"):
            out.append((b["owner"], d))
    return out


def match_owners(buys: list[dict], credits: list[tuple[str, int]], bonding_curve: str | None) -> None:
    """Fill ``token_owner`` of each buy row: exact amount match first, then the signer itself."""
    pool = [c for c in credits if c[0] != bonding_curve]
    for b in buys:
        hit = next((c for c in pool if c[1] == b["tokens_out"]), None)
        if hit is None:
            hit = next((c for c in pool if c[0] == b["signer"]), None)
        if hit is not None:
            b["token_owner"] = hit[0]
            pool.remove(hit)


def rows_from_record(rec: dict, now: int | None = None) -> tuple[dict, list[dict]]:
    """creation_record() dict (with raw TradeEvents in ``_trades``) -> (creates row, create_buys rows)."""
    trades = rec.get("_trades") or []
    buys = []
    for i, t in enumerate(trades):
        if not t.is_buy:
            continue
        buys.append({"sig": rec["create_sig"], "ev_index": i, "mint": rec["mint"], "signer": t.user,
                     "token_owner": None, "ix_name": t.ix_name, "sol_in": t.sol_amount,
                     "tokens_out": t.token_amount, "vsol_post": t.virtual_sol_reserves,
                     "vtok_post": t.virtual_token_reserves, "fee_bps": t.fee_bps,
                     "creator_fee_bps": t.creator_fee_bps})
    progs = rec.get("launcher_programs_ordered") or rec.get("launcher_programs") or []
    row = {
        "mint": rec["mint"], "slot": rec.get("create_slot"), "block_time": rec.get("create_time"),
        "sig": rec["create_sig"], "signer": rec.get("create_user"), "fee_payer": rec.get("fee_payer"),
        "creator_arg": rec.get("creator"), "bonding_curve": rec.get("bonding_curve"),
        "launcher": launcher_of(progs), "launcher_programs_json": sorted(set(progs)) or None,
        "quote_mint": rec.get("quote_mint"), "token_program": rec.get("token_program"),
        "is_mayhem": rec.get("is_mayhem_mode"), "name": rec.get("name"), "symbol": rec.get("symbol"),
        "uri_host": uri_host(rec.get("uri")),
        "intx_sol_in": sum(b["sol_in"] for b in buys), "intx_tokens_out": sum(b["tokens_out"] for b in buys),
        "intx_n_buys": len(buys),
        "v_after_intx": trades[-1].virtual_sol_reserves if trades else rec.get("init_vsol"),
        "init_vsol": rec.get("init_vsol"), "init_vtok": rec.get("init_vtok"),
        "n_creates_in_tx": rec.get("n_creates_in_tx"),
        "regime": regime_for(rec.get("create_slot"), rec.get("create_time")),
        "updated_at": now or int(time.time()),
    }
    return row, buys


def rows_from_tx(tx: dict | None, sig: str, src: str = "tx") -> tuple[dict, list[dict]] | None:
    """Confirmed jsonParsed transaction -> (creates row, create_buys rows); None if not a successful create."""
    rec = creation_record(_Prefetched(tx), sig)  # type: ignore[arg-type]
    if rec is None or tx is None:
        return None
    evs = E.extract_events(tx)
    rec["_trades"] = [e for e in evs if isinstance(e, E.TradeEvent) and e.mint == rec["mint"]]
    rec["launcher_programs_ordered"] = E.top_level_programs(tx)
    ce = next((e for e in evs if isinstance(e, E.CreateEvent)), None)
    rec["uri"] = ce.uri if ce else None
    row, buys = rows_from_record(rec)
    match_owners(buys, token_credits(tx, rec["mint"]), rec.get("bonding_curve"))
    args = create_args(tx) or {}
    tip, _ = tips(tx)
    meta = tx.get("meta") or {}
    ver = tx.get("version")
    row.update({
        "tx_index": tx.get("transactionIndex"), "signers_json": signers(tx),
        "tx_version": None if ver is None else str(ver), "ix_variant": args.get("ix_variant"),
        "is_cashback": args.get("is_cashback"), "fee_lamports": meta.get("fee"),
        "priority_lamports": priority_lamports(tx), "tip_lamports": tip,
        "cu_used": meta.get("computeUnitsConsumed"), "decoded_from": "tx", "src": src,
    })
    return row, buys


def rows_from_logs(logs: list[str], sig: str, slot: int | None, src: str = "logs") -> tuple[dict, list[dict]] | None:
    """logsSubscribe notification logs -> partial (creates row, create_buys rows); None without a CreateEvent."""
    evs = E.extract_events({"meta": {"logMessages": logs}})
    ce = next((e for e in evs if isinstance(e, E.CreateEvent)), None)
    if ce is None:
        return None
    trades = [e for e in evs if isinstance(e, E.TradeEvent) and e.mint == ce.mint]
    rec = {"create_sig": sig, "create_slot": slot, "create_time": None, "mint": ce.mint,
           "bonding_curve": ce.bonding_curve, "creator": ce.creator, "create_user": ce.user,
           "fee_payer": None, "name": ce.name[:64], "symbol": ce.symbol[:32], "uri": ce.uri,
           "token_program": ce.token_program, "is_mayhem_mode": ce.is_mayhem_mode,
           "quote_mint": ce.quote_mint, "init_vsol": ce.virtual_sol_reserves,
           "init_vtok": ce.virtual_token_reserves,
           "n_creates_in_tx": sum(1 for e in evs if isinstance(e, E.CreateEvent)), "_trades": trades}
    row, buys = rows_from_record(rec)
    row.pop("launcher", None)
    row.pop("launcher_programs_json", None)
    row.update({"decoded_from": "logs", "src": src, "block_time": None})
    if logs_truncated(logs):
        # later TradeEvents may be missing: in-tx totals are unknown until getTransaction
        for k in ("intx_sol_in", "intx_tokens_out", "intx_n_buys", "v_after_intx"):
            row[k] = None
    return row, buys


def logs_truncated(logs: list[str]) -> bool:
    return any(line.startswith("Log truncated") for line in logs[-3:])
