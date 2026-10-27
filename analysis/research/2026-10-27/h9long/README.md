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

## Results (Oct 19 00:00 to Oct 27 00:40 UTC)

- **+355 SOL in 8 days** (ledger; its native SOL rose +412 with +11.9 deposited; 467.7 SOL now). Pump coins priced in
  another token (WBTC, PUMP, TTWO, CARDS, xStocks ...) +278 on 3,056 coins; SOL-priced pump coins +28.9 on 1,439;
  PumpSwap-only tokens +17.3 (CARDS +13.5); the quote tokens it holds between trades +28.6.
- **Its rule:** one buy (median 1.1 SOL), one sell of everything: take profit when the curve price reaches 2.5x the price
  its buy left, stop at 0.7x, otherwise sell after 30 minutes (77% of its trips). About 10% of trips hit the take
  profit and carry all of the profit. Bigger buys hit it more often (>= 1.5 SOL: 22.7% on Oct 19-22, 9.8% on Oct 23-27).
- **The copy that passes:** its buys of at least 1.5 SOL (SOL value), 1.0 SOL each, landing at the end of its slot or
  the next, sold a step ahead of its own exit (2.45x / 0.71x / 1790 s). Picked on P1, scored on P2, worst costs
  (0.0095 SOL a transaction + 0.0155 bias), full paths: P1 +43.2 (+39.3 without the 2 best), P2 +12.8 (+9.2).
  At 0.5 SOL the fixed costs eat it (P2 about 0 without the 2 best). Copying every buy, mirroring its sell, or
  scalping the copiers behind it lose.

## Second check of the exits (main thread, `exits_pess.py`)

`exits.py` fills a take profit on the trade that fired it and never puts the wallet's own sell ahead of ours. Both
flatter the copy: its sell is as fast as ours and fires on the same trades. `exits_pess.py HAIR TP SL [KIND] [-v]`:
a TP / SL fired within a slot of its own sell is filled on the state its sell leaves; otherwise on the next trade of
the path; a rule that never fired sells before its 30-minute sell (ours fires by the clock) or, when its TP / SL jumped
past ours in one trade, after it. HAIR takes a further share off every TP / SL fill (other copiers ahead).

- At 2.45x our TP lands after its sell on 83 of 126 TP trips: P2 +5.0 (+1.6 without the best 2), and negative with 5%.
- **A lower TP sells first**: at 2.0x only 8-10 land after it. TP 2.0 / SL 0.8 / 1790 s, its buys >= 1.5 SOL:

  | coins | P1 Oct 19-22 | P2 Oct 23-27 | P2, 5% haircut |
  |---|---|---|---|
  | SOL-priced | -11.12 (73) | -2.34 (31) | |
  | **other quote tokens** | **+39.53 (+35.92), 352** | **+15.27 (+12.22), 246** | +8.53 (+5.74) |

  By day: +11.7 +10.0 +0.1 +17.7 / +2.1 +5.8 +6.6 +1.3. P2 rests on WBTC-priced coins (+10.9 on 83).
- **Capital** (in buy order, a copy skipped while the cap is full), P2: 1 open +4.69 (+2.03), 2 open +7.50 (+4.84),
  3 open +10.48 (+7.43), 5 open +15.26 (+12.21).
