"""G1: slot-0 activity predictor for the LEAD micro-scalp (docs/research/strategy-predictor.md).

One table of every multi-wallet bundled launch on disk (gap-3 + deep + fresh + dense window, deduped by
mint), with

* the LEAD return per launch from the shared simulator (0.5 SOL, +1.1 % premium, 0.004 SOL, 1-slot exit
  latency) plus stress variants, the exit reason and the "busy" label;
* ONLY features observable at the decision instant in slot 0 (the create tx and the bundle that sits
  right in front of our buy, the signer's funding, point-in-time operator history, time and create rate).

Nothing here touches the network: create transactions come from the RPC disk cache, tapes from the
swap-api disk cache, metadata JSON from ``data/cache/predictor/meta`` (filled by
``analysis/scripts/strategy/predictor_meta.py``).
"""
from __future__ import annotations

import bisect
import collections
import json
import math
import re
import unicodedata
from pathlib import Path

import numpy as np

from . import sim
from .tapes import SPLIT_SLOT, Tape, build_tape, load_deep, load_gap3

REPO = Path(__file__).resolve().parents[3]
CACHE = REPO / "data" / "cache" / "predictor"
LEAD = [("tp", 0.15), ("ins", "bundle"), ("idle", 1), ("psl", 1, 0.15), ("slotexit", 5)]
REASONS = ("tp", "bundle_sell", "idle", "psl", "slot5")
OCT12 = 1791763200          # 2026-10-12T00:00:00Z
SRC_ORDER = ("gap3", "deep", "fresh", "window")
COMPUTE_BUDGET = "ComputeBudget111111111111111111111111111111"
BUSY_N = 100                # busy = more than one API page (100 trades) within 15 min of the create


# ----------------------------------------------------------------------------- tapes
class _CacheOnlyApi:
    def __init__(self):
        from ..pumpapi import PumpApi

        class CacheOnly(PumpApi):
            def get_json(self, url, params=None, cache_key=None):
                if cache_key:
                    hit = self.cache.get(cache_key)
                    if hit is not None:
                        return hit["v"]
                raise KeyError("not cached")

        self.api = CacheOnly()


def load_window() -> list[Tape]:
    """Dense window (workflow C, Oct 14 02:05-08:05 UTC), exactly as orchestrator_checks.window()."""
    from .. import window as win
    api = _CacheOnlyApi().api
    creates = {}
    for line in open(REPO / "data/cache/window/creates.jsonl"):
        r = json.loads(line)
        creates[r["mint"]] = r
    meta = {}
    for p in ("trades_meta_run1.jsonl", "trades_meta.jsonl"):
        for line in open(REPO / "data/cache/window" / p):
            m = json.loads(line)
            meta[m["mint"]] = m
    out = []
    for m in meta.values():
        r = creates.get(m["mint"])
        if not r or r.get("is_mayhem_mode"):
            continue
        qm = r.get("quote_mint") or ""
        if qm and not qm.startswith("1111"):
            continue
        t0 = r["create_time"]
        try:
            if m.get("mode") == "full_1page":
                raw, comp = api.trades_before(m["mint"], (t0 + 900) * 1000 + 999, max_pages=1)
                res = win._as_result(raw, t0 + 900 if comp else None, 900, "full_1page")
            elif m.get("mode") == "full":
                res = win.early_trades(api, m["mint"], t0, 900, 15)
            else:
                raw, comp = api.trades_before(m["mint"], (t0 + 60) * 1000 + 999, max_pages=2)
                res = win._as_result(raw, t0 + 60 if comp else None, 900, "prefix60")
        except KeyError:
            continue
        if not res["complete"]:
            continue
        tr = [[t["slot"], t["txi"], t["sub"], t["ts"], t["user"], int(t["buy"]), t["sol"], t["tok"], t["mcap"],
               t["sig"], t["prog"]] for t in res["trades"]]
        tr = [t for t in tr if t[0] >= r["create_slot"]]
        C = {r["creator"], r["create_user"], r["fee_payer"]}
        t = build_tape(m["mint"], "window", 1.0, r["create_slot"], t0, r["create_sig"], C, tr,
                       res["covered_until"] - t0, True)
        if t is not None:
            t.wmode = m.get("mode")          # type: ignore[attr-defined]
            out.append(t)
    out.sort(key=lambda t: (t.bt, t.mint))
    return out


def load_fresh() -> list[Tape]:
    out = []
    for line in open(REPO / "data/cache/strategy/holdout_trades.jsonl"):
        r = json.loads(line)
        if not r["complete"] or not r["trades"]:
            continue
        C = {r["creator"], r["user"], r["fee_payer"]}
        t = build_tape(r["mint"], "fresh", 1.0, r["slot"], r["bt"], r["sig"], C, r["trades"], r["hor"], True)
        if t is not None:
            out.append(t)
    out.sort(key=lambda t: (t.bt, t.mint))
    return out


def gap3_meta() -> dict:
    """mint -> (create sig, signer, truncation flag) for gap-3 tapes."""
    sel = {s["mint"]: s for s in json.load(open(REPO / "data/cache/gap3/sel.json"))}
    trunc = {}
    for line in open(REPO / "data/cache/gap3/trades.jsonl"):
        r = json.loads(line)
        trunc[r["mint"]] = bool(r["trunc"])
    return {m: dict(sig=s["sig"], signer=s["user"], creator=s["creator"], fee_payer=s["fee_payer"],
                    trunc=trunc.get(m, False)) for m, s in sel.items()}


def load_all() -> list[Tape]:
    """All multi-wallet bundled tapes, deduped by mint (longest horizon wins; ties by SRC_ORDER)."""
    sets = {"gap3": load_gap3(), "deep": load_deep(), "fresh": load_fresh(), "window": load_window()}
    best: dict[str, Tape] = {}
    for src in SRC_ORDER:
        for t in sets[src]:
            if t.kind != "multi":
                continue
            o = best.get(t.mint)
            if o is None or t.hor > o.hor:
                best[t.mint] = t
    out = sorted(best.values(), key=lambda t: (t.bt, t.mint))
    return out


# ----------------------------------------------------------------------------- LEAD return
def lead_eval(t: Tape, S=0.5, prem=0.011, cost=0.004, lat=1) -> dict:
    e, se = t.pb, t.S0
    if e >= t.hend:
        return {}
    m_e = float(t.mc[e])
    q = sim.tokens_for(S, m_e) / (1 + prem)
    f = sim.fire_slots(t, e, se, q, S, LEAD)
    fi = sim.fill_index(t, e, f, lat)
    k = int(np.argmin(f))
    fill = int(fi[k])
    r = float(sim.value(float(t.mc[fill]), q) / S - 1 - cost / S)
    hold_slots = float(f[k] - se) if np.isfinite(f[k]) else float("nan")
    return dict(ret=r, reason=REASONS[k], hold_s=hold_slots * t.ms_slot / 1000)


# ----------------------------------------------------------------------------- create tx
def _rpc_cache():
    from ..cache import DEFAULT_CACHE, DiskCache
    return DiskCache(DEFAULT_CACHE, "rpc")


def _cu_budget(tx: dict) -> tuple[int | None, int | None]:
    from .. import events as E
    lim = price = None
    msg = (tx.get("transaction") or {}).get("message") or {}
    for ix in msg.get("instructions") or []:
        if ix.get("programId") != COMPUTE_BUDGET or "data" not in ix:
            continue
        try:
            d = E.b58decode(ix["data"])
        except Exception:  # noqa: BLE001
            continue
        if d[:1] == b"\x02" and len(d) >= 5:
            lim = int.from_bytes(d[1:5], "little")
        elif d[:1] == b"\x03" and len(d) >= 9:
            price = int.from_bytes(d[1:9], "little")
    return lim, price


def create_tx_features(sig: str, rc=None) -> dict:
    from ..createtx import rows_from_tx
    from ..store import launcher_of
    rc = rc or _rpc_cache()
    hit = rc.get(f"tx:{sig}")
    tx = hit.get("v") if hit else None
    if not tx:
        return {"tx_ok": 0}
    res = rows_from_tx(tx, sig)
    if res is None:
        return {"tx_ok": 0}
    row, buys = res
    from .. import events as E
    lim, price = _cu_budget(tx)
    progs = E.top_level_programs(tx)
    evs = E.extract_events(tx)
    ce = next((e for e in evs if isinstance(e, E.CreateEvent)), None)
    signers = row.get("signers_json") or []
    return dict(
        tx_ok=1, tx_index=tx.get("transactionIndex"), tip_lamports=row.get("tip_lamports") or 0,
        priority_lamports=row.get("priority_lamports"), cu_used=row.get("cu_used"), cu_limit=lim, cu_price=price,
        launcher=launcher_of(progs), n_top_ix=len(progs), tx_version=row.get("tx_version"),
        ix_variant=row.get("ix_variant"), is_cashback=row.get("is_cashback"),
        signer=row.get("signer"), creator_arg=row.get("creator_arg"), fee_payer=row.get("fee_payer"),
        n_signers=len(signers), token_program=row.get("token_program"), is_mayhem=row.get("is_mayhem"),
        uri=ce.uri if ce else None, name=row.get("name"), symbol=row.get("symbol"),
        intx_n_buys=row.get("intx_n_buys"),
        creator_fee_bps=max((b.get("creator_fee_bps") or 0 for b in buys), default=None),
        fee_bps=max((b.get("fee_bps") or 0 for b in buys), default=None),
    )


# ----------------------------------------------------------------------------- name / symbol / uri
_EMOJI = re.compile("[\U0001F000-\U0001FAFF☀-➿⬀-⯿]")


def text_features(name: str | None, symbol: str | None, uri: str | None) -> dict:
    name, symbol = name or "", symbol or ""
    letters = [c for c in name if c.isalpha()]
    sl = [c for c in symbol if c.isalpha()]
    host = None
    if uri:
        m = re.match(r"https?://([^/]+)/", uri + "/")
        host = m.group(1).lower() if m else None
    return dict(
        name_len=len(name), sym_len=len(symbol), name_words=len(name.split()),
        name_caps=(sum(c.isupper() for c in letters) / len(letters)) if letters else 0.0,
        sym_caps=(sum(c.isupper() for c in sl) / len(sl)) if sl else 0.0,
        emoji=int(bool(_EMOJI.search(name + symbol))),
        non_ascii=int(any(ord(c) > 127 for c in name + symbol)),
        cjk=int(any("CJK" in unicodedata.name(c, "") or "HIRAGANA" in unicodedata.name(c, "")
                    or "KATAKANA" in unicodedata.name(c, "") for c in name + symbol)),
        sym_ticker=int(bool(re.fullmatch(r"\$?[A-Z0-9]{2,6}", symbol))),
        sym_dollar=int(symbol.startswith("$")),
        name_eq_sym=int(name.strip().lower().replace(" ", "") == symbol.strip().lower().lstrip("$")),
        name_digits=int(any(c.isdigit() for c in name)),
        uri_host=host or "none",
        uri_ipfs=int(bool(uri and "ipfs" in uri)),
    )


def meta_features(mint: str) -> dict:
    p = CACHE / "meta" / f"{mint}.json"
    if not p.exists():
        return dict(meta_ok=-1)
    try:
        d = json.load(open(p))
    except (OSError, json.JSONDecodeError):
        return dict(meta_ok=-1)
    j = d.get("json")
    if not isinstance(j, dict):
        return dict(meta_ok=0, meta_ms=d.get("ms"))
    ms = d.get("gw_ms") or d.get("ms")
    blob = json.dumps(j).lower()

    def has(k):
        v = j.get(k)
        if v is None and isinstance(j.get("extensions"), dict):
            v = j["extensions"].get(k)
        return int(bool(v and str(v).strip()))

    tw = has("twitter")
    return dict(
        meta_ok=1, meta_ms=ms, meta_gw=int(bool(d.get("gw"))), m_twitter=tw, m_telegram=has("telegram"), m_website=has("website"),
        m_n_social=tw + has("telegram") + has("website"),
        m_tw_status=int("/status/" in str(j.get("twitter") or "")),
        m_x_com=int("x.com" in blob or "twitter.com" in blob),
        m_desc_len=len(str(j.get("description") or "")),
        m_show_name=int(bool(j.get("showName"))),
        m_created_on_pump=int("pump.fun" in str(j.get("createdOn") or "")),
        m_created_on=str(j.get("createdOn") or "none")[:40],
    )


# ----------------------------------------------------------------------------- funding
def _label_cat(addr: str | None) -> str | None:
    if not addr:
        return None
    from .. import labels as L
    lab = L.get_label(addr)
    return lab.category if lab is not None else None


class Funding:
    """Signer funding as of the create (all three caches; window by wallet, gap-1 by create sig)."""

    def __init__(self):
        self.by_sig = {}
        for line in open(REPO / "data/cache/gap1/funding.jsonl"):
            r = json.loads(line)
            self.by_sig[r["sig"]] = r
        self.by_wallet = {}
        for line in open(REPO / "data/cache/window/funding.jsonl"):
            r = json.loads(line)
            self.by_wallet[r["wallet"]] = r
        extra = CACHE / "funding_extra.jsonl"     # deep / fresh signers (predictor_funding.py)
        if extra.exists():
            for line in open(extra):
                r = json.loads(line)
                self.by_wallet.setdefault(r["wallet"], r)
        self.by_asof = {}
        pool = CACHE / "funding_pool.jsonl"       # operator-pool launches, as of each create (angle B)
        if pool.exists():
            for line in open(pool):
                r = json.loads(line)
                self.by_asof[r["as_of_sig"]] = r
        import sqlite3
        db = sqlite3.connect(f"file:{REPO}/data/analytics.db?mode=ro", uri=True)
        self.db = {}
        for row in db.execute("select wallet, status, funder, amount, block_time, n_prior, fresh, as_of_mint, "
                              "funder_category, main_funder from funding"):
            self.db[row[0]] = row
        self.fc = {}
        p = REPO / "data/funding_creators.jsonl"
        if p.exists():
            for line in open(p):
                r = json.loads(line)
                k = r.get("wallet") or r.get("creator")
                if k:
                    self.fc[k] = r

    def get(self, sig: str, signer: str, bt: int) -> dict:
        n_prior = funder = amount = ftime = None
        status = "unknown"
        src = None
        r = self.by_sig.get(sig)
        if r is not None:
            src = "gap1"
            n_prior = r.get("n_pre")
            status = r["status"]
            f = r.get("first") or {}
            funder, amount, ftime = f.get("funder"), f.get("amount"), f.get("ftime")
            if status == "busy":
                status = "serial"
        elif sig in self.by_asof or signer in self.by_wallet:
            r = self.by_asof.get(sig) or self.by_wallet[signer]
            src = "window"
            n_prior = r.get("n_prior")
            status = r["status"]
            f = r.get("main") or r.get("first") or {}
            funder, amount, ftime = f.get("funder"), f.get("amount"), f.get("time")
        elif signer in self.db:
            row = self.db[signer]
            src = "db"
            status = row[1] or "unknown"
            funder, amount = row[2], (row[3] / 1e9 if row[3] else None)
            ftime, n_prior = row[4], row[5]
        if n_prior is not None and n_prior >= 1000:
            status = "serial"
        cat = _label_cat(funder)
        lag = (bt - ftime) if (ftime and bt) else None
        if status == "serial":
            cls = "serial"
        elif status not in ("ok",) or funder is None:
            cls = "unknown" if src is None else "nofund"
        elif cat == "cex":
            cls = "cex_fresh" if (n_prior is not None and n_prior <= 10) else "cex_old"
        elif cat in ("swap_service", "bridge"):
            cls = "swap"
        else:
            cls = "mother_fresh" if (n_prior is not None and n_prior <= 10) else "mother_old"
        return dict(fund_src=src or "none", fund_cls=cls, fund_n_prior=n_prior, fund_amount=amount,
                    fund_lag_h=(lag / 3600) if lag is not None else None,
                    fund_round=int(bool(amount and abs(amount * 10 - round(amount * 10)) < 1e-3)))


# ----------------------------------------------------------------------------- create rate
class CreateIndex:
    """Every pump create of the 16 days (gap-1 index of the mint authority's signatures)."""

    def __init__(self):
        d = json.load(open(REPO / "data/cache/gap1/index.json"))
        ts = sorted(int(s[2]) for s in d["sigs"] if s[2] and not s[3])
        self.ts = np.array(ts, dtype=np.int64)
        self.lo, self.hi = int(self.ts[0]), int(d["anchor"])

    def rate(self, bt: int, win: int = 60) -> float | None:
        if bt - win < self.lo or bt > self.hi:
            return None
        a = np.searchsorted(self.ts, bt - win, side="left")
        b = np.searchsorted(self.ts, bt, side="left")
        return float(b - a)


# ----------------------------------------------------------------------------- bundle features
def bundle_features(t: Tape) -> dict:
    s0 = np.nonzero(t.slot == t.S0)[0]
    bidx = [i for i in s0 if i <= t.pb and t.buy[i] and t.ins_b[i]]
    sizes = np.array([t.sol[i] for i in bidx]) if bidx else np.array([0.0])
    toks = np.array([t.tok[i] for i in bidx]) if bidx else np.array([0.0])
    per_w = collections.defaultdict(float)
    for i in bidx:
        per_w[int(t.uid[i])] += t.sol[i]
    wsz = np.array(list(per_w.values())) if per_w else np.array([0.0])

    def roundish(x):
        return abs(x * 10 - round(x * 10)) < 0.002 or abs(x * 100 - round(x * 100)) < 0.0005

    n_pre = int(np.sum(s0 < t.pb + 1))
    return dict(
        nB=t.nB, bsol=t.bsol, dev_sol=t.dev_sol, tot_sol=t.bsol + t.dev_sol, me=t.me,
        dev_share=t.dev_sol / (t.bsol + t.dev_sol) if (t.bsol + t.dev_sol) > 0 else 0.0,
        b_n_tx=len(bidx), b_max=float(wsz.max()), b_min=float(wsz.min()), b_mean=float(wsz.mean()),
        b_cv=float(wsz.std() / wsz.mean()) if wsz.mean() > 0 and len(wsz) > 1 else 0.0,
        b_round=float(np.mean([roundish(x) for x in sizes])) if bidx else 0.0,
        b_tok_eq=float(np.mean(np.abs(toks / toks.max() - 1) < 0.002)) if bidx and toks.max() > 0 else 0.0,
        b_desc=int(len(sizes) > 1 and bool(np.all(np.diff(sizes) <= 1e-9))),
        n_trades_to_pb=n_pre,
        b_sells_in_bundle=int(np.sum(~t.buy[:t.pb + 1])),
        dev_round=int(roundish(t.dev_sol)) if t.dev_sol > 0 else 0,
        tok_pct_insiders=float(np.sum(np.where(t.buy[:t.pb + 1], t.tok[:t.pb + 1], 0)) / 1e9),
    )




# ----------------------------------------------------------------------------- labels
def activity(t: Tape, trunc: bool = False) -> dict:
    after = t.ts[t.pb + 1:]
    n60 = int(np.sum(after <= t.bt + 60))
    n10 = int(np.sum(after <= t.bt + 10))
    n900 = int(np.sum(after <= t.bt + 900)) if t.hor >= 900 else None
    if t.hor >= 900:
        busy = int(len(t.ts[t.ts <= t.bt + 900]) > BUSY_N)
    elif trunc:
        busy = 1     # gap-3 tape truncated because the history ran past its page budget
    else:
        busy = None
    return dict(n10=n10, n60=n60, n900=n900, busy900=busy)


# ----------------------------------------------------------------------------- operator pools (angle B)
POOL_SRCS = ("exp", "opA", "opB", "ops")


def load_pool() -> list[tuple[Tape, str, str]]:
    """Operator-sampled multi-wallet tapes of strategy-operators.md (S3): gap-1 expanded wallets (`exp` +
    `opA`), new heavy bundlers (`opB`) and gap-3's operator pull (`ops`). Short tapes (2-25 s), chosen by
    operator, not at random: used only as training rows and as point-in-time operator history.
    Returns [(tape, create sig, signer)]."""
    from . import arming
    from ..pumpapi import normalize_trade
    L = arming.load_launches(verbose=False)
    sig = {}
    for line in open(REPO / "data/cache/strategy/op_tapes.jsonl"):
        r = json.loads(line)
        dev = [x for x in r["trades"] if x[4] == r["wallet"] and x[5]]
        if dev:
            sig[r["mint"]] = dev[0][9]
    for s in json.load(open(REPO / "data/cache/gap3/ops_sel_EhrQmJ.json")):
        sig.setdefault(s["mint"], s["sig"])
    api = _CacheOnlyApi().api
    out = []
    for x in L:
        if x.src not in POOL_SRCS or x.tape.kind != "multi":
            continue
        s = sig.get(x.mint)
        if s is None and x.src == "exp":
            try:
                raw, _ = api.trades_before(x.mint, (x.bt + 2) * 1000 + 999, 6)
            except KeyError:
                continue
            nt = sorted((normalize_trade(t) for t in raw), key=lambda t: (t["slot"], t["txi"], t["sub"]))
            dev = [t for t in nt if t["user"] == x.creator and t["buy"]]
            s = dev[0]["sig"] if dev else None
        if s is None:
            continue
        x.tape.src = "pool_" + x.src
        out.append((x.tape, s, x.creator))
    return out
