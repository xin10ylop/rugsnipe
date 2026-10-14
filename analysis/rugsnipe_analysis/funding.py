"""Funding tracer: who funded a wallet, and who else did that funder fund?

Core ideas (the guru's "simple / mother / exchange" schemes, made mechanical):

* :func:`first_funding` -- the earliest incoming SOL transfer of a wallet
  (paginate getSignaturesForAddress to the oldest page; capped).
* :func:`trace_funding` -- walk funder -> funder's funder up to ``max_depth``
  hops, stopping at a labeled CEX/bridge/DEX or at an "exchange-like" wallet
  (> ``exchange_tx_threshold`` signatures, i.e. a CEX we don't know or a
  mother wallet).
* :func:`siblings_scan` -- for a funder and an amount, list recent outgoing
  transfers within ``amount +/- tol`` to *fresh* wallets (first tx == that
  transfer).  This is the guru's "filter Binance 2 transfers between 2.955 and
  2.956" step.  Practical only for low/medium-volume funders over public RPC;
  for CEX hot wallets use :func:`siblings_from_index` over our own creator
  funding index (we see every pump.fun creator anyway -- the reverse lookup is
  orders of magnitude cheaper than scanning Binance).

All RPC goes through the shared :class:`rugsnipe_analysis.rpc.RpcClient`
(signature history is pinned to the archive endpoint there).
"""
from __future__ import annotations

import threading
from dataclasses import asdict, dataclass, field
from typing import Any, Iterable, Optional

from . import labels as L
from .rpc import RpcClient

LAMPORTS = 1_000_000_000
SYSTEM_TRANSFER_TYPES = {"transfer", "transferWithSeed", "createAccount", "createAccountWithSeed"}

_client: Optional[RpcClient] = None
_client_lock = threading.Lock()


def shared_client() -> RpcClient:
    """Process-wide RpcClient (8 req/s total, round-robin, disk cache)."""
    global _client
    with _client_lock:
        if _client is None:
            _client = RpcClient()
        return _client


# --------------------------------------------------------------------------
# transaction decoding
# --------------------------------------------------------------------------
@dataclass
class SolTransfer:
    source: str
    destination: str
    lamports: int
    kind: str          # system instruction type, or "balance_delta"
    inner: bool        # True if found in innerInstructions (CPI)

    @property
    def sol(self) -> float:
        return self.lamports / LAMPORTS


def _iter_instructions(tx: dict) -> Iterable[tuple[dict, bool]]:
    msg = (tx.get("transaction") or {}).get("message") or {}
    for ix in msg.get("instructions") or []:
        yield ix, False
    for grp in (tx.get("meta") or {}).get("innerInstructions") or []:
        for ix in grp.get("instructions") or []:
            yield ix, True


def program_ids(tx: dict) -> list[str]:
    """Distinct program ids invoked (top-level + CPI), in first-seen order."""
    seen: list[str] = []
    for ix, _ in _iter_instructions(tx):
        p = ix.get("programId")
        if p and p not in seen:
            seen.append(p)
    return seen


def account_keys(tx: dict) -> list[str]:
    keys = ((tx.get("transaction") or {}).get("message") or {}).get("accountKeys") or []
    return [k["pubkey"] if isinstance(k, dict) else k for k in keys]


def signers(tx: dict) -> list[str]:
    keys = ((tx.get("transaction") or {}).get("message") or {}).get("accountKeys") or []
    return [k["pubkey"] for k in keys if isinstance(k, dict) and k.get("signer")]


def sol_transfers(tx: dict) -> list[SolTransfer]:
    """Native SOL movements from parsed System-program instructions (incl. CPI)."""
    out: list[SolTransfer] = []
    for ix, inner in _iter_instructions(tx):
        if ix.get("program") != "system":
            continue
        parsed = ix.get("parsed")
        if not isinstance(parsed, dict) or parsed.get("type") not in SYSTEM_TRANSFER_TYPES:
            continue
        info = parsed.get("info") or {}
        dst = info.get("destination") or info.get("newAccount")
        lam = info.get("lamports")
        if info.get("source") and dst and lam:
            out.append(SolTransfer(info["source"], dst, int(lam), parsed["type"], inner))
    return out


def balance_delta(tx: dict, address: str) -> Optional[int]:
    """Lamport change of ``address`` in ``tx`` (None if not an account key)."""
    keys = account_keys(tx)
    if address not in keys:
        return None
    i = keys.index(address)
    meta = tx.get("meta") or {}
    try:
        return int(meta["postBalances"][i]) - int(meta["preBalances"][i])
    except (KeyError, IndexError, TypeError):
        return None


def incoming_transfers(tx: dict, wallet: str) -> list[SolTransfer]:
    """SOL transfers *into* wallet; falls back to balance delta attribution."""
    ins = [t for t in sol_transfers(tx) if t.destination == wallet and t.source != wallet]
    if ins:
        return ins
    d = balance_delta(tx, wallet)
    if d and d > 0:
        # attribute to the account with the largest lamport decrease
        keys = account_keys(tx)
        meta = tx.get("meta") or {}
        pre, post = meta.get("preBalances") or [], meta.get("postBalances") or []
        best, best_d = None, 0
        for k, a, b in zip(keys, pre, post):
            if k != wallet and b - a < best_d:
                best, best_d = k, b - a
        if best:
            return [SolTransfer(best, wallet, d, "balance_delta", True)]
    return []


# --------------------------------------------------------------------------
# signature history helpers
# --------------------------------------------------------------------------
@dataclass
class History:
    address: str
    sigs: list[dict]          # newest first
    complete: bool            # False => older history beyond the page cap

    @property
    def n(self) -> int:
        return len(self.sigs)

    @property
    def oldest(self) -> Optional[dict]:
        return self.sigs[-1] if self.sigs else None

    def rate_per_hour(self) -> Optional[float]:
        """Tx rate over the fetched window (newest..oldest fetched)."""
        if len(self.sigs) < 2:
            return None
        t0, t1 = self.sigs[-1].get("blockTime"), self.sigs[0].get("blockTime")
        if not t0 or not t1:
            return None
        return round(len(self.sigs) / max((t1 - t0) / 3600, 1 / 3600), 1)


def history(address: str, max_pages: int, rpc: Optional[RpcClient] = None) -> History:
    """Signature history (newest first) up to ``max_pages`` x 1000."""
    rpc = rpc or shared_client()
    sigs, complete = rpc.get_all_signatures(address, max_pages=max_pages)
    return History(address, sigs, complete)


# --------------------------------------------------------------------------
# first funding
# --------------------------------------------------------------------------
@dataclass
class Funding:
    """Funding facts of one wallet.

    Top-level fields describe the FIRST incoming SOL transfer (what Solscan's
    "funded by" shows).  ``main`` describes the LARGEST incoming SOL transfer
    among the scanned transactions before ``until_time`` (e.g. before the token
    create) -- the economically relevant source when a wallet is first
    "touched" with dust and funded properly afterwards.
    """
    wallet: str
    status: str                      # ok | older_than_cap | no_history | no_incoming_sol
    funder: Optional[str] = None
    amount_sol: Optional[float] = None
    signature: Optional[str] = None
    slot: Optional[int] = None
    block_time: Optional[int] = None
    kind: Optional[str] = None       # transfer | createAccount | balance_delta ...
    via_programs: list[str] = field(default_factory=list)   # labeled bridge/dex programs in that tx
    tx_signers: list[str] = field(default_factory=list)
    main: Optional[dict] = None      # same keys for the largest pre-``until_time`` incoming transfer
    n_incoming: int = 0              # incoming SOL transfers seen in the scanned window
    incoming_total_sol: float = 0.0
    scanned_txs: int = 0
    wallet_sig_count: Optional[int] = None
    wallet_history_complete: Optional[bool] = None
    first_sig_time: Optional[int] = None
    note: str = ""

    def to_dict(self) -> dict:
        return asdict(self)

    def chosen(self, follow: str = "main") -> dict:
        """Funding dict to follow up the chain (``main`` or ``first``)."""
        first = {"funder": self.funder, "amount_sol": self.amount_sol, "signature": self.signature,
                 "slot": self.slot, "block_time": self.block_time, "kind": self.kind,
                 "via_programs": self.via_programs}
        if follow == "main" and self.main:
            return self.main
        return first


def _incoming_record(tx: dict, s: dict, wallet: str) -> Optional[dict]:
    ins = incoming_transfers(tx, wallet)
    if not ins:
        return None
    t = max(ins, key=lambda x: x.lamports)
    sg = signers(tx)
    via: list[str] = []
    if t.kind == "balance_delta" or t.source not in sg:
        # source is a PDA / token account / relayer: attribute to the program
        via = [lab.address for lab in L.label_programs(program_ids(tx))]
    return {"funder": t.source, "amount_sol": round(t.sol, 9), "signature": s["signature"],
            "slot": s.get("slot"), "block_time": s.get("blockTime"), "kind": t.kind,
            "via_programs": via, "tx_signers": sg,
            "total_in_sol": round(sum(x.sol for x in ins), 9)}


def first_funding(wallet: str, max_pages: int = 5, scan_max: int = 12,
                  until_time: Optional[int] = None, hist: Optional[History] = None,
                  rpc: Optional[RpcClient] = None) -> Funding:
    """First (and largest) incoming SOL transfer of ``wallet``.

    Paginates the wallet's signatures to the oldest page (``max_pages`` cap:
    a wallet with more history is reported as ``older_than_cap``), then scans
    up to ``scan_max`` of the earliest successful transactions that happened
    before ``until_time`` (all, if None).
    """
    rpc = rpc or shared_client()
    h = hist or history(wallet, max_pages, rpc)
    f = Funding(wallet=wallet, status="ok", wallet_sig_count=h.n, wallet_history_complete=h.complete)
    if not h.sigs:
        f.status = "no_history"
        return f
    f.first_sig_time = h.oldest.get("blockTime") if h.oldest else None
    if not h.complete:
        f.status = "older_than_cap"
        f.note = f">{h.n} signatures; oldest page not reached (cap {max_pages} pages)"
        return f
    chron = [s for s in reversed(h.sigs) if s.get("err") is None
             and (until_time is None or (s.get("blockTime") or 0) <= until_time)][:scan_max]
    recs: list[dict] = []
    for s in chron:
        tx = rpc.get_transaction(s["signature"])
        if not tx:
            continue
        rec = _incoming_record(tx, s, wallet)
        if rec:
            recs.append(rec)
    f.scanned_txs = len(chron)
    if not recs:
        f.status = "no_incoming_sol"
        f.note = f"no incoming SOL in the {len(chron)} oldest successful txs"
        return f
    first = recs[0]
    f.funder, f.amount_sol, f.kind = first["funder"], first["amount_sol"], first["kind"]
    f.signature, f.slot, f.block_time = first["signature"], first["slot"], first["block_time"]
    f.via_programs, f.tx_signers = first["via_programs"], first["tx_signers"]
    f.n_incoming = len(recs)
    f.incoming_total_sol = round(sum(r["total_in_sol"] for r in recs), 9)
    main = max(recs, key=lambda r: r["amount_sol"])
    f.main = {k: main[k] for k in ("funder", "amount_sol", "signature", "slot", "block_time", "kind",
                                   "via_programs")}
    if len(chron) >= scan_max and h.n > scan_max:
        f.note = f"scanned only the {scan_max} oldest txs before until_time"
    return f


# --------------------------------------------------------------------------
# recursive trace
# --------------------------------------------------------------------------
@dataclass
class Hop:
    depth: int                     # 0 = the traced wallet itself
    address: str
    kind: str                      # see classify_wallet()
    label: Optional[dict] = None   # Label.to_dict() subset when labeled
    sig_count: Optional[int] = None
    sig_count_capped: Optional[bool] = None
    tx_per_hour: Optional[float] = None
    first_seen: Optional[int] = None
    funding: Optional[dict] = None  # Funding.to_dict() of this hop (None for terminal)

    def to_dict(self) -> dict:
        return asdict(self)


def _label_brief(lab: L.Label) -> dict:
    return {"entity": lab.entity, "name": lab.name, "category": lab.category,
            "confidence": lab.confidence, "solscan": lab.solscan}


def classify_wallet(h: History, funding_time: Optional[int], exchange_tx_threshold: int) -> str:
    """Heuristic class of an unlabeled funder.

    * ``exchange_like``     -- more than ``exchange_tx_threshold`` signatures
                               (unknown CEX/instant-exchange or a busy mother).
    * ``fresh_intermediary`` -- <= 10 txs total and first tx <= 48 h before it
                               funded the downstream wallet.
    * ``wallet``            -- anything else (regular wallet / mother candidate).
    """
    if not h.complete or h.n > exchange_tx_threshold:
        return "exchange_like"
    first = h.oldest.get("blockTime") if h.oldest else None
    if h.n <= 10 and first and funding_time and 0 <= funding_time - first <= 48 * 3600:
        return "fresh_intermediary"
    return "wallet"


def trace_funding(wallet: str, max_depth: int = 3, exchange_tx_threshold: int = 2000,
                  wallet_max_pages: int = 5, until_time: Optional[int] = None, follow: str = "main",
                  rpc: Optional[RpcClient] = None, hist: Optional[History] = None) -> list[Hop]:
    """Funding chain wallet -> funder -> funder's funder ... (<= max_depth hops).

    ``until_time`` (e.g. the token create time) bounds which incoming transfers
    count for the traced wallet; each upper hop is bounded by the time it sent
    funds downstream.  ``follow`` = "main" (largest incoming, default) or
    "first" (first incoming, = Solscan's "funded by").

    Stops at a labeled terminal (CEX, bridge, DEX, launchpad, bot fee...), at an
    ``exchange_like`` wallet (> threshold sigs; history capped), or when a
    funding transfer cannot be decoded.
    """
    rpc = rpc or shared_client()
    chain: list[Hop] = []
    h0 = hist or history(wallet, wallet_max_pages, rpc)
    f = first_funding(wallet, max_pages=wallet_max_pages, until_time=until_time, hist=h0, rpc=rpc)
    chain.append(Hop(0, wallet, "traced", None, h0.n, not h0.complete, h0.rate_per_hour(),
                     f.first_sig_time, f.to_dict()))
    visited = {wallet}
    pages_for_threshold = max(1, -(-exchange_tx_threshold // 1000))  # ceil
    for depth in range(1, max_depth + 1):
        if f.status != "ok":
            break
        c = f.chosen(follow)
        addr = c.get("funder")
        if not addr or addr in visited:
            break
        visited.add(addr)
        lab = L.get_label(addr)
        if lab is not None:
            chain.append(Hop(depth, addr, lab.category, _label_brief(lab)))
            break
        if c.get("via_programs"):
            # funds arrived through a bridge/DEX program (relayer/PDA source)
            plab = L.get_label(c["via_programs"][0])
            chain.append(Hop(depth, addr, f"via_{plab.category}" if plab else "via_program",
                             _label_brief(plab) if plab else None))
            break
        h = history(addr, pages_for_threshold, rpc)
        kind = classify_wallet(h, c.get("block_time"), exchange_tx_threshold)
        if kind == "exchange_like":
            chain.append(Hop(depth, addr, kind, None, h.n, not h.complete, h.rate_per_hour(), None))
            break
        nf = first_funding(addr, max_pages=pages_for_threshold, until_time=c.get("block_time"),
                           scan_max=8, hist=h, rpc=rpc)
        chain.append(Hop(depth, addr, kind, None, h.n, not h.complete, h.rate_per_hour(),
                         nf.first_sig_time, nf.to_dict()))
        f = nf
    return chain


def summarize_chain(chain: list[Hop]) -> dict[str, Any]:
    """Compact classification of a funding chain for reporting.

    ``origin`` is the category where the trace stopped:
      cex | swap_service | onramp | institution | bridge | dex | launchpad | bot_fee | mev |
      gambling | via_bridge | via_dex |
      exchange_like | wallet (depth exhausted at a normal wallet) | unknown.
    ``pattern`` maps it onto the guru's taxonomy:
      exchange (CEX / instant-exchange / onramp / casino funded, directly or via
      fresh hops), mother (an unlabeled,
      non-fresh wallet funds the creator directly or via fresh hops),
      simple (creator funded by a wallet that is itself funded by a regular wallet),
      bridge/dex, unknown.
    """
    hops = chain[1:]
    root = chain[0]
    out: dict[str, Any] = {
        "depth": len(hops),
        "direct_funder": hops[0].address if hops else None,
        "direct_funder_kind": hops[0].kind if hops else None,
        "origin": None, "origin_address": None, "origin_label": None,
        "n_fresh_intermediaries": sum(1 for h in hops if h.kind == "fresh_intermediary"),
        "pattern": "unknown",
    }
    if not hops:
        out["origin"] = (root.funding or {}).get("status", "unknown")
        return out
    last = hops[-1]
    out["origin"], out["origin_address"] = last.kind, last.address
    out["origin_label"] = (last.label or {}).get("name")
    kinds = [h.kind for h in hops]
    first_non_fresh = next((h for h in hops if h.kind != "fresh_intermediary"), None)
    if first_non_fresh is None:
        out["pattern"] = "fresh_chain_unresolved"
    elif first_non_fresh.kind in L.EXCHANGE_CATEGORIES:
        out["pattern"] = "exchange"
    elif first_non_fresh.kind in {"via_bridge", "bridge"}:
        out["pattern"] = "bridge"
    elif first_non_fresh.kind in {"via_dex", "dex", "via_launchpad", "launchpad"}:
        out["pattern"] = "dex_or_launchpad"
    elif first_non_fresh.kind == "exchange_like":
        out["pattern"] = "exchange_like_or_mother"
    elif first_non_fresh.kind == "wallet":
        out["pattern"] = "mother_or_simple"
    out["kinds"] = kinds
    return out


# --------------------------------------------------------------------------
# siblings ("exchange pattern")
# --------------------------------------------------------------------------
@dataclass
class Sibling:
    destination: str
    amount_sol: float
    signature: str
    slot: Optional[int]
    block_time: Optional[int]
    fresh: Optional[bool]               # destination's first-ever tx is this transfer
    destination_sig_count: Optional[int]

    def to_dict(self) -> dict:
        return asdict(self)


def is_fresh_recipient(dest: str, sig: str, rpc: Optional[RpcClient] = None) -> tuple[Optional[bool], int]:
    """True if ``sig`` is the oldest signature of ``dest`` (it was born by this transfer)."""
    rpc = rpc or shared_client()
    h = history(dest, 1, rpc)
    if not h.complete:
        return False, h.n          # > 1000 txs: definitely not fresh
    if not h.sigs:
        return None, 0
    return h.sigs[-1]["signature"] == sig, h.n


def siblings_scan(funder: str, amount_sol: float, tol_sol: float = 0.001, max_pages: int = 1,
                  max_txs: int = 400, check_fresh: bool = True, workers: int = 6,
                  rpc: Optional[RpcClient] = None) -> dict[str, Any]:
    """Outgoing SOL transfers from ``funder`` within ``amount +/- tol`` (recent first).

    Cost: ``max_pages`` signature pages + up to ``max_txs`` getTransaction calls
    (+1 history call per matching destination when ``check_fresh``).  At ~8
    req/s, 400 txs ~ 1 min.  For a CEX hot wallet doing ~1000 tx/h this covers
    only ~25 minutes -- use :func:`siblings_from_index` there.
    """
    from concurrent.futures import ThreadPoolExecutor

    rpc = rpc or shared_client()
    h = history(funder, max_pages, rpc)
    sigs = [s for s in h.sigs if s.get("err") is None][:max_txs]
    lo, hi = int((amount_sol - tol_sol) * LAMPORTS), int((amount_sol + tol_sol) * LAMPORTS)

    def _scan(s: dict) -> list[Sibling]:
        tx = rpc.get_transaction(s["signature"])
        if not tx:
            return []
        res = []
        for t in sol_transfers(tx):
            if t.source == funder and lo <= t.lamports <= hi:
                res.append(Sibling(t.destination, round(t.sol, 9), s["signature"], s.get("slot"),
                                   s.get("blockTime"), None, None))
        return res

    with ThreadPoolExecutor(max_workers=workers) as ex:
        found = [sib for lst in ex.map(_scan, sigs) for sib in lst]
    if check_fresh and found:
        with ThreadPoolExecutor(max_workers=workers) as ex:
            fr = list(ex.map(lambda sb: is_fresh_recipient(sb.destination, sb.signature, rpc), found))
        for sb, (fresh, n) in zip(found, fr):
            sb.fresh, sb.destination_sig_count = fresh, n
    span = None
    if len(sigs) >= 2 and sigs[0].get("blockTime") and sigs[-1].get("blockTime"):
        span = sigs[0]["blockTime"] - sigs[-1]["blockTime"]
    return {
        "funder": funder, "amount_sol": amount_sol, "tol_sol": tol_sol,
        "scanned_txs": len(sigs), "window_seconds": span,
        "funder_history_complete": h.complete,
        "matches": [s.to_dict() for s in found],
        "fresh_matches": sum(1 for s in found if s.fresh),
    }


def siblings_from_index(records: Iterable[dict], funder: str, amount_sol: float,
                        tol_sol: float = 0.001) -> list[dict]:
    """Reverse lookup over our own creator-funding records.

    ``records`` are rows produced by ``scripts/trace_funding.py`` (each has
    ``creator`` and ``funding`` = :meth:`Funding.to_dict`).  Returns rows whose
    first OR main funding came from ``funder`` within ``amount +/- tol``.  This
    is how a live bot should implement the "exchange pattern": index every new
    creator's funding at creation time, then match (funder, amount) clusters.
    """
    out = []
    for r in records:
        fd = r.get("funding") or {}
        cands = [(fd.get("funder"), fd.get("amount_sol"))]
        if fd.get("main"):
            cands.append((fd["main"].get("funder"), fd["main"].get("amount_sol")))
        if any(f == funder and a is not None and abs(a - amount_sol) <= tol_sol + 1e-12 for f, a in cands):
            out.append(r)
    return out


def amount_signature(amount_sol: Optional[float]) -> Optional[str]:
    """Bucket an amount for cluster matching (3 decimals, like the guru's filter)."""
    if amount_sol is None:
        return None
    return f"{amount_sol:.3f}"


def is_round_amount(amount_sol: Optional[float]) -> Optional[bool]:
    """True for 'human' amounts (multiples of 0.1 SOL); odd amounts like 2.955 -> False."""
    if amount_sol is None:
        return None
    return abs(amount_sol * 10 - round(amount_sol * 10)) < 1e-6
