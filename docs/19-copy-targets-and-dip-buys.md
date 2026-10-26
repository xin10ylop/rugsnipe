# 19. Other wallets to copy, why the profitable ones cannot be copied, and the dump-and-bounce test (2026-10-26)

Scripts: `analysis/research/2026-10-26/wallets/`, `holders/`, `flush/`. Every result here is chain-true unless it says
otherwise: a wallet's SOL balance change on its own transactions (fees, tips and priority fees in). The tape-based "own"
figures used at first left out tips and priority fees and flattered every scalper.

## 1. The search

- **Universe.** Every early buyer of the 26,952 in-class launches of Oct 23 00:00 - Oct 25 18:00. 251 wallets made 30+
  launch buys on 9,141 coins whose first 13 slots were fetched complete (every landed trade). The 5-day tapes
  (`five/coin5b.py`) keep one curve state per slot after create + 1, and only up to his last sell. That is right for
  copying him, not for other wallets.
- **The copy, as for him.** Our buy lands at our measured distances behind theirs, bound +5%, 0.5 SOL, sold at their
  learned hold - 1 or a fixed slot.
- **The selection decays.** Picking on two days and scoring the third, the top 5 made +1.57 SOL, the top 10 +0.26 and
  the top 20 -0.68. Backwards the same numbers were -0.59, -0.03 and -2.70. Only wallets positive on *every* day kept
  their sign.
- **Out of sample** (Oct 25 18:00 - Oct 26 11:00, the 13 finalists' own transactions and their coins' tapes): the set,
  one copy per coin, made **-4.86 SOL** on 1,052 copies. JBokzLFR held up (+1.52 on 90 copies, positive in each 6-hour
  block). So did 3yrJE1A8 (+0.53).

## 2. Why the profitable wallets cannot be copied

A copy buys after their buy has pushed the price, and sells after their sell has pushed it. It keeps only the market's
move between their buy and their first sell, less about 5% (2.5% fees, 0.7% tips, ~2% for landing behind), and only if
it sells first.

| | the move between their buy and their first sell |
|---|---|
| him, key buys on hot operators | mean +13.0%, median +5.4% |
| him, his other launch buys | mean +5.0% (our cost: the copy loses there) |
| 3yrJE1A8 | mean +11.0%, median +7.0%; a copy lands at its price (median +0.2%); it sells 4-6 s after its buy |
| 9apNtyQc (+41.8 SOL itself) | mean +5.8%: a copy loses (-6.12 following its sells) |

- **Most of them lose.** The 88 launch holders with 15+ closed coins (Oct 24 - Oct 26) lost -757 SOL together. 23 were
  positive on every day.
- **The rival CWiNFA breaks even.** It sends one buy per coin with a 3M tip through one relay (`astra…` tip accounts)
  and lands 92% of them. It outbids him in 627 of 632 head-to-heads, and its coins go a median +12.4% by create + 10.
  After 2.5% fees, its tip and priority fees it made -0.47 SOL on 979 launch buys (Oct 24 - Oct 25 18:00), -7.79 since
  Oct 24. It picks j7tracker / uxento launches with 2-4 SOL dev buys.
- **The biggest winners are insiders.** The top every-day holders come in clusters:
  - B3zRbxte, 9rbokPEP and 4QYcEYZ1 share ~170 coins.
  - Dg3oFnB4, 13yfeA3m and GB7NgXoi share 96.
  - HAmnM7in and 6F1kzJ3U share 45.

  On every taped coin where two or more of the first cluster bought (60 of 60), its buys sit at position +1, right
  after the create, inside the dev's bundle. Nobody is ever ahead of them, and the price rises +27% across their buys.
  A copy lands 11-28% above their price. The move after their buy is their own; the buyers after them are who they
  sell to.

## 3. The dump-and-bounce idea (buy the flush, sell the bounce)

The bonding curve cannot fall below its launch price, so here a "limit order at the dump's bottom" means buying right
after a big sell, at some fraction of the high. Tested on the 3 minutes after each dump, sampled every 3 slots. Our
buy and sell act one sample after the signal; 0.5 SOL, fees, 1M + 0.5M tips.

- **Early dumps** (a sell taking >= 25% off the high after a >= 30% rise, in the first 13 slots): 1,315 coins.
  - Sellers: the dev 563, the bundle 408, another early buyer 344.
  - Buying right after: **-26 to -66 SOL** on 1,305 trades, whatever the exit.
  - A 20% target with a 25% stop wins 29% of the time; it needs about 45%.
  - 1 of 912 entry / exit / seller variants is positive in total. None is positive on every day.
- **Later dumps** inside those paths (a run of +50% or +100% off the low, then a flush of 30-40%): 271 and 59 events.
  0 of 144 variants is positive.

After a dump these coins keep falling more often than they bounce. Not built.

## 4. What stands

- **Him**, on his key buys on hot operators (section 10 of docs/17), sold one slot before his hold.
- **3yrJE1A8**: a copy lands at its price, and it sells 4-6 s after buying. Sold just before it, the copy is worth
  +3.25 SOL over Oct 24-26, against -3.23 following its sell. On the tapes with our landing distances and its learned
  hold - 1: +2.75 in sample (positive every day), +0.53 out of sample.
- **JBokzLFR**, on its 1-2M tip buys: +1.52 out of sample. It needs more days.

## 5. Built: `entry.copy_more`, with 3yrJE1A8 at create + 29

- **Its exit.** 3yrJE1A8 buys at create + 0 (206 coins) or + 1 (88), and sells on a timer: 20 slots after its buy (116
  coins) or 30 (125). Its 13-slot tapes never reached that far, so its exit was measured on the curve itself
  (`analysis/research/2026-10-26/w3/`).

  | copy sold at its buy + | copies | Oct 24 / 25 / 26 (to 12:00) | total |
  |---|---|---|---|
  | 5 slots | 256 | +1.54 / +1.89 / -0.02 | +3.42 SOL |
  | 14 | 247 | +3.73 / +1.78 / -0.29 | +5.22 |
  | 19 | 241 | +4.69 / +0.22 / +0.37 | +5.28 |
  | 24 | 249 | +5.10 / +1.81 / +0.48 | +7.39 |
  | **29** | 252 | +4.54 / +2.99 / +0.75 | **+8.28** |
  | after its first sell, else 29 | 280 | +1.32 / +1.80 / +0.23 | +3.35 |

  On its 20-slot coins the copy loses at every exit (-1.7 to -2.5 SOL). Its 30-slot coins and the rest carry the
  result. The gain grows smoothly with the hold rather than at one exit.
- **Config.** `[entry] copy_more = [{ wallet = "...", hold_to_create_slots = 29 }]`, up to 8 wallets. Each one's launch
  buys (create + 0/1) of at least `min_sol` are bought behind it as his are: the same +5% bound, window and risk caps,
  but none of his filters. The copy is sold at create + `hold_to_create_slots`; its wallet is not the position's
  insider, so its own sell does not trigger ours.
- **Running it.** `mode` off / log / on, and `entry.copy` must be on or log (a buy needs both on). The live feed already
  passes every trade of a fresh launch to the hot loop, so the feed is unchanged. Journal lines
  `copyw <mint> by <wallet> <SOL> at create+<k> slot <s> idx <i> sell create+<h> -> buy|log|skip_small`. The status line
  adds `copyw wallets N seen small buy log passed`.

## 6. Live, 2026-10-26 13:00: copy only its 2 SOL buys (`min_sol = 1.5`)

3yrJE1A8 buys either 0.5 or 2 SOL. Its own trades from the start (Oct 25 22:08) to 13:22, read with
`bash ~/check.sh him 3yrJE1A8`:

| its buy | coins | wins | its mean | a 0.5 SOL copy at its prices |
|---|---|---|---|---|
| 2 SOL | 43 | 25 | +7.5% | +1.60 SOL |
| 0.5 SOL | 22 | 9 | -1.4% | -0.15 SOL |

The first live copy (3c3WaGz7, a 0.5 SOL buy) lost -44.5%, as the wallet itself did (-43%). The box runs
`copy_more = [{ wallet = "3yrJE1A8...", min_sol = 1.5, hold_to_create_slots = 29 }]` from 13:26. The split is one
15-hour sample, its own fills before ours; `check.sh him 3yrJE1A8` keeps measuring it.
