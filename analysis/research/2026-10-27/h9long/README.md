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

## What `rugsnipe-long` can route (main thread)

The bot routes through pump's own programs only (`multi_hop_swap` from wrapped SOL or a held float, docs/20). Its
598 qualifying buys by route (`routable.json`; a direct curve buy is routable when its quote is a float or has a
canonical PumpSwap pool against wSOL or WBTC):

| buys | count | P1 Oct 19-22 | P2 Oct 23-27 | P2, at most 3 open |
|---|---|---|---|---|
| **routable** | 293 | +10.97 (+8.51) | **+12.21 (+9.34)** | **+11.03 (+8.16)** |
| of which: multihop from wSOL | 179 | +5.32 | +1.08 | |
| of which: multihop from WBTC (a float) | 91 | +2.35 | +10.02 (+7.64) | +8.89 |
| of which: curve quoted in WBTC | 20 | +1.71 | +1.12 | |
| not routable without Jupiter (direct buys of other quotes, PUMP floats, 3+ hops) | 305 | +28.57 | +3.06 (+0.41) | +1.44 |

The routable part holds in both periods; the rest made its money in P1 and is about zero in P2.

## Does it scale? (`scale.py`, main thread)

The routable buys, TP 2.0 / SL 0.8 / 1790 s, its sell ahead of ours within a slot, at most 3 open, at a bigger stake.
`exits_pess.py`'s price model (the recorded path shifted by our tokens: later buyers trade fixed token amounts) makes a
bigger stake pay more than its share: later buyers pay more for the same tokens and it flows to us (5 SOL: +96 on
Oct 23-27). The cautious model shifts the path by our quote instead (copy bots spend fixed SOL), and lets the wallet's
own exits fire on the price with our tokens in (its 2.5x comes earlier by our lift and lands ahead of our 2.0x):

| stake | Oct 19-22 | Oct 23-27 | Oct 23-27, 5% haircut |
|---|---|---|---|
| 1 SOL | +6.74 (+4.77) | +8.84 (+6.19) | +5.90 (+3.48) |
| 1.5 SOL | +10.92 (+8.04) | +14.42 (+10.51) | |
| 2 SOL | +14.30 (+10.56) | +18.98 (+13.87) | +13.25 (+8.59) |
| 3 SOL | +18.66 (+13.32) | +26.05 (+18.70) | +17.67 (+11.00) |
| 5 SOL | +3.86 (-3.33) | +25.58 (+15.62) | |

Its TP fires earlier because of our lift on 39 trips at 1 SOL, 55 at 3, 65 at 5. Up to 2-3 SOL the result grows about
with the stake (the fixed costs thin out, our own impact eats part); at 5 SOL Oct 19-22 is negative without its best
2. More open positions add little past 5 (at 1 SOL: 3 open +11.03, 5 open +12.46, no cap +12.21, optimistic model).

## Its smaller buys, and the rest of the settings (main thread, `band.py`, `grid.py`, `route_cls.py`)

Price paths fetched for its 2,011 buys of 0.8-1.5 SOL of coins priced in another token (`full_paths_band.py`);
routability rebuilt from the transaction rows (`route_cls.py`: 593 of 598 agree with `routable.json`). The long copy's
rule on each band, routable only, no cap, P1 Oct 19-22 / P2 Oct 23-27 (cautious model q + lift; path model v):

| its buy | copies | P1 (q / v) | P2 (q / v) |
|---|---|---|---|
| 0.8-1.0 SOL | 199 | -1.40 / -0.49 | -2.39 / -1.62 |
| 1.0-1.25 SOL | 162 | +5.62 / +6.97 | -6.46 / -6.32 |
| 1.25-1.45 SOL | 56 | -1.84 / -1.82 | -0.92 / -0.66 |
| 1.45-1.5 SOL | 28 | -1.54 / -1.58 | +0.54 / +0.78 |
| **1.5+ SOL** | 295 | **+6.69 / +9.11** | **+10.43 / +12.91** |

A stake of two thirds of its buy instead of 1 SOL changes none of the signs. The floor stays at 1.45 (the 1.45-1.5 slice:
about -0.03 SOL a copy on 28, too few to move it). On the 1.5+ buys (`grid.py`, at most 3 open): the built exits rank 5 of
129 on P1 under q (trailing stops never in the top 15; a 0.85 stop helps the SOL routes only); a tighter entry premium
does nothing on P1; each slot of landing delay costs about 3-5% (10 slots: P1 negative without its best 2). By route,
WBTC-routed copies make about +0.12 SOL a copy in both periods (28 and 76 copies), SOL-routed about +0.02 (110, 63);
the WBTC profit comes on 3 of the 8 days. Our copy as built by day (q): +3.1 +1.4 +1.0 -0.5 / -1.0 +5.8 +3.5 +1.4; the
wallet's own result on the same coins was positive every day (+82 against our +17 over the 8 days).

Take profit from 1.1x (`grid.py tp`, stop 0.8 and 0.9, 1790 s): 2.0 is the best on both periods (P1 +5.03, P2 +9.63).
Lower takes lose on P1 (1.1: -7.76 / -4.66; 1.2: -6.17 / -0.25; 1.4: -2.90 / +5.85; 1.5: +0.57 / +5.61; 1.6:
+0.48 / +7.18; 1.75: +3.72 / +8.59) and win barely more often (1.1: 45 of 144 on P1 against 37 of 135 at 2.0): a small
take is mostly eaten by the round trip (the curve fees and about 0.0345 SOL fixed a trade) and cuts off the runs that pay
for the losers. 2.5 and above equal no take profit at all: the wallet's own 2.5x sale lands first and we sell after it.

Positions open at once (`grid.py cap`): the wallet holds a median 4 of these coins at once (90%: 7, max 17; every one 30
minutes), all its coins a median 89. Our routable copies overlap less: at most 1 open P1 +2.59 / P2 +3.51, 2: +6.50 /
+8.11, 3 (built): +5.03 / +9.63, 4: +6.73 / +11.13, 5: +6.69 / +10.86, no cap: +6.69 / +10.43 (146 and 149 copies against
135 and 136 at 3). Its costs on these buys: a median 0.0071 SOL fee (base + priority; 10%: 0.0035, 90%: 0.0134) and a
0.00107 SOL tip on 40-55% of them; on its sells a median 0.00001 SOL and no tip.
