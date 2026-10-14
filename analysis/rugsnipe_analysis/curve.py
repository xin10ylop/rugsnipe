"""pump.fun bonding-curve math (constant product on virtual reserves).

Units: SOL reserves in lamports (1e9), token reserves in raw units (6 decimals).
Market cap convention (matches the API's `market_cap`, in SOL):
    mcap_sol = price_sol_per_token * 1e9 tokens = vsol/vtok * 1e6

Fees observed on 2026 TradeEvents: protocol 95 bps + creator 30 bps = 1.25 %
per side, charged on top of the SOL amount for buys and deducted for sells.

Mayhem-mode tokens (CreateEvent.is_mayhem_mode) start from the same reserves,
but an agent holds extra supply and trades it, so their price CAN go below the
initial-price "floor" -- the 'natural stop-loss' argument does not hold there.
"""
from __future__ import annotations

LAMPORTS = 1_000_000_000
TOKEN_UNIT = 1_000_000
INITIAL_VSOL = 30 * LAMPORTS
INITIAL_VTOK = 1_073_000_000 * TOKEN_UNIT
INITIAL_RTOK = 793_100_000 * TOKEN_UNIT
TOTAL_SUPPLY = 1_000_000_000 * TOKEN_UNIT
DEFAULT_FEE_BPS = 125  # 95 protocol + 30 creator


def mcap_sol(vsol: int | float, vtok: int | float) -> float:
    """Market cap in SOL from virtual reserves."""
    return vsol / vtok * 1e6


def price_sol(vsol: int | float, vtok: int | float) -> float:
    """Spot price in SOL per whole token."""
    return (vsol / LAMPORTS) / (vtok / TOKEN_UNIT)


INITIAL_MCAP_SOL = mcap_sol(INITIAL_VSOL, INITIAL_VTOK)  # ~27.96 SOL


def completion_mcap_sol(vsol0: int = INITIAL_VSOL, vtok0: int = INITIAL_VTOK,
                        rtok0: int = INITIAL_RTOK) -> float:
    """Market cap when all real tokens are sold (curve complete)."""
    k = vsol0 * vtok0
    vtok = vtok0 - rtok0
    return mcap_sol(k / vtok, vtok)


def buy_tokens_out(sol_in_lamports: int, vsol: int, vtok: int,
                   fee_bps: int = DEFAULT_FEE_BPS) -> tuple[int, int, int]:
    """Tokens received for spending `sol_in_lamports` (fees included in spend).

    Returns (tokens_out, new_vsol, new_vtok).
    """
    net = sol_in_lamports * 10_000 // (10_000 + fee_bps)
    k = vsol * vtok
    new_vsol = vsol + net
    new_vtok = -(-k // new_vsol)  # ceil
    return vtok - new_vtok, new_vsol, new_vtok


def sell_sol_out(tokens_in: int, vsol: int, vtok: int,
                 fee_bps: int = DEFAULT_FEE_BPS) -> tuple[int, int, int]:
    """SOL received (after fees) for selling `tokens_in`. Returns (sol, new_vsol, new_vtok)."""
    k = vsol * vtok
    new_vtok = vtok + tokens_in
    new_vsol = -(-k // new_vtok)
    gross = vsol - new_vsol
    return gross - gross * fee_bps // 10_000, new_vsol, new_vtok


def reserves_from_mcap(mcap: float, vsol0: int = INITIAL_VSOL,
                       vtok0: int = INITIAL_VTOK) -> tuple[float, float]:
    """Virtual reserves on the standard curve for a given market cap (SOL).

    k = vsol*vtok, mcap = vsol/vtok*1e6  ->  vsol = sqrt(k*mcap/1e6).
    """
    k = vsol0 * vtok0
    vsol = (k * mcap / 1e6) ** 0.5
    return vsol, k / vsol


def sol_to_reach_mcap(mcap: float) -> float:
    """SOL (excl. fees) that must be bought from launch to reach `mcap` on a standard curve."""
    vsol, _ = reserves_from_mcap(mcap)
    return (vsol - INITIAL_VSOL) / LAMPORTS


def roundtrip_multiple(entry_mcap: float, exit_mcap: float, size_sol: float,
                       fee_bps: int = DEFAULT_FEE_BPS) -> float:
    """Net SOL multiple for buying `size_sol` at entry_mcap and selling everything
    when the (pre-sell) curve is at exit_mcap, on a standard curve, incl. own impact."""
    vsol, vtok = reserves_from_mcap(entry_mcap)
    toks, _, _ = buy_tokens_out(int(size_sol * LAMPORTS), int(vsol), int(vtok), fee_bps)
    vsol2, vtok2 = reserves_from_mcap(exit_mcap)
    sol_out, _, _ = sell_sol_out(toks, int(vsol2), int(vtok2), fee_bps)
    return sol_out / (size_sol * LAMPORTS)
