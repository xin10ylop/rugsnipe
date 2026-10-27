# Wallet 4 (H9kuqGKN) over 8 days: how it makes its money, and a copy rule (Oct 27)

Run against the session scratchpad (`SP/h9long/`), with its Triton client (`month/tri.py`) and the pump parsers
(`rivalpnl/`). Order:

- `sigs.py WALLET T0`: every signature since T0 (Oct 19 00:00 UTC: 17,278, of which 16,845 landed).
- `pull.py WALLET`: each landed transaction as a row: SOL change, token changes by mint (wSOL included), the pump
  curve trades (every user), the programs called. Any venue.
- `ledger.py WALLET`: its result in SOL over every venue and quote. A token-for-token swap (a coin sold for its quote
  token) is valued at the received token's price at the time (from its own swaps of that token against SOL or USD).
  Average cost per token; sold-before-the-window and unpriced legs kept apart. Accounting check against the cash.
- `groups.py WALLET`: the result by kind of token, entry stage, day and 6-hour block, without each period's best 2.
- `trips.py`, `behave.py`, `exitrule.py`: its round trips on the curve (one buy, one sell), the price multiple at its
  sell (constant product: price ~ 1/vt^2 whatever the quote) and its hold. Its exit rule falls out of the clusters.
- `byquote.py`, `tprate.py`: by quote token and by what predicts its take-profit hits.
- `entry_tapes.py WALLET N_OTHER N_SOL` (`-1 0` = every other-quote trip): the curve trades right after its buy (its
  slot, +1, +2) and, for the sample, right before its sell.
- `sim1.py` (no tapes, flat premium), `sim2.py` + `score.py` (the tapes' real premium at our landing): the copy.
  `filt.py`, `filt2.py`: filters picked on P1 (Oct 19-22), scored on P2 (Oct 23-27). `rule.py`: the rule by day.
- `full_paths.py WALLET MIN [CAP]`: the curve path from its buy to its sell (long paths sampled), and `exits.py`: our own
  exits on it.

Costs everywhere: its own median round-trip cost on the curve and routes (2.9% SOL-quoted coins, 5.1% other quotes),
0.0045 or 0.0095 SOL a transaction, and the model bias 0.0155 SOL a fill (`../../2026-10-26/h9/calib.py`).
