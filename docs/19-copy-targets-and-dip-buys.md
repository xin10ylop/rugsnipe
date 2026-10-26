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

## 7. The finalists out of sample to Oct 26 14:50, and JBokzLFR added

The 13 finalists' own transactions were pulled again from 10:30 to 14:50 and their coins' 13-slot tapes fetched (2,415
tapes). Filters and exits stay as fixed in sample (`specs.txt`); our buy lands at our measured distances behind theirs,
+5% bound, 0.5 SOL (`oos_screen.py`). Oct 25 18:00 to Oct 26 14:50, by 6-hour block:

| wallet | copies | total | 18-24h / 00-06h / 06-12h / 12-15h |
|---|---|---|---|
| JBokzLFR | 116 | +1.84 | +0.64 / +0.56 / +0.48 / +0.16 |
| 9su3UPQW (its repeat operators) | 377 | +1.75 | +0.93 / +1.02 / -1.94 / +1.74 |
| 3yrJE1A8 | 130 | +1.70 | +0.57 / -0.01 / +0.04 / +1.10 |
| 2c945Fd8 | 97 | +0.03 | |
| the other 9 | | -8.5 together | |
| the set, one copy per coin | 1,608 | -4.84 | |

The screen sells at the wallet's sell when that comes first. The bot sells a `copy_more` position at create + hold
whatever the wallet does (`oos_screen_nosell.py`):

| JBokzLFR, sold at | in sample (Oct 23 / 24 / 25) | out of sample (4 blocks) |
|---|---|---|
| create + 5 | +1.10 (+0.22 / +0.52 / +0.36) | **+1.93** (+0.73 / +0.44 / +0.53 / +0.23) |
| create + 9 | +0.97 (+0.10 / +0.38 / +0.50) | +1.40 (+0.54 / +0.51 / +0.40 / -0.05) |
| create + 12 | | +1.64 |

It buys 0.1 to 0.5 SOL (1 SOL since Oct 25: those 28 copies -0.24). 9su3UPQW needs its repeat-operator filter, which
`copy_more` does not have, and lost -1.94 in one block: left out. **JBokzLFR goes in at create + 5, every size.**

## 8. 3yrJE1A8's exit, decoded (Oct 24 - Oct 26 12:00, 304 coins)

Its gain is read from the curve at its buy + 5 / 9 / 14 / 17 / 19 / 24 / 29 (`w3/rule.py`, `rule2.py`, `timer.py`,
`timer2.py`, `tp.py`).

- **Two timers, chosen at random as far as the chain shows.** 130 coins are sold about 20 slots after its buy, 147 about
  30. The choice is not its size (2 SOL: 63 / 75; 0.5 SOL: 67 / 72), not create + 0 or + 1, and not the operator: 24 of
  the 25 operators with 3+ coins get both timers. The host doesn't decide it either. The price path before its +20 sell
  does not tell the two apart (AUC 0.48 to 0.59 for the gain at +9 / +19, momentum, drawdown, the peak).
- **An early take profit, loosely.** 22 coins are sold within 12 slots. 16 of those are sold when the position would
  fetch +35% or more over its cost. But 10 timer coins were worth that at +5 or +9 and were held, so it isn't a plain
  threshold on what this data shows.
- **For the copy.** Following its early sells loses (+7.07 against +8.25, `early.py`). A take profit of our own at the
  sampled slots (+30% over its price): +8.91 against +8.25, but Oct 24 is worse (+3.75 against +4.54). That's not
  consistent enough to build. With a random timer, holding to create + 29 stays the copy's exit. A take profit could
  only be settled on per-slot paths to + 29 (about 30,000 calls).

## 9. The failed copies, one by one, and every fix tested (Oct 26, 16:00)

Since the start, 12 of our 35 copies of him and 3yrJE1A8 landed and were refused by the 5% limit (pump error 6042).
Both wallets made money on most of those coins. Each one, from the chain (`fails/`):

| coin | copied | its buy | ours | price moved | buys between (SOL) | filled where we landed | right behind it |
|---|---|---|---|---|---|---|---|
| DAkZMwi3 | him | +0 #598 | +1 #22 | +32% | 4 (5.55) | +0.521 | +0.853 |
| 3cEYBUp4 | him | +0 #509 | +1 #161 | +12% | 6 (2.29) | +0.083 | +0.154 |
| 9387oQyF | him | +0 #690 | +3 #29 | +8% | 3 (2.38) | +0.066 | +0.112 |
| HZEe2Rvq | him | +0 #146 | +0 #205 | +8% | 3 (1.34) | +0.033 | +0.076 |
| GUjeXnaQ | him | +0 #328 | +1 #74 | +9% | 4 (1.52) | +0.013 | +0.058 |
| FNHRpj8P | him | +0 #232 | +0 #396 | +10% | 3 (1.84) | -0.016 | +0.033 |
| D7cp9jeq | him | +0 #196 | +1 #50 | +7% | 4 (1.19) | -0.016 | +0.019 |
| 97NhoAsk | him | +0 #275 | +1 #17 | +6% | 6 (1.08) | -0.031 | -0.003 |
| 3fe3ppjS | 3yrJE1A8 | +1 #28 | +1 #287 | +45% | 8 (8.50) | +0.475 | +0.922 |
| 2DkuBnM6 | 3yrJE1A8 | +1 #22 | +1 #515 | +10% | 4 (1.81) | +0.052 | +0.106 |
| pu5Q3GD2 | 3yrJE1A8 | +0 #343 | +1 #346 | +12% | 1 (2.47) | +0.051 | +0.118 |
| 3g1MdujY | 3yrJE1A8 | +0 #538 | +1 #260 | +13% | 3 (2.67) | -0.079 | -0.024 |

- **Who moved the price.** In his own slot it's the create snipers, whose transactions were sent before his buy was
  visible: CWiNFA (0.99 SOL, 3M tip) on 4 of them, G9sQPg9E (0.8-1 SOL, 5.5M priority + 2M tip) on 4. In the next slot,
  three times, a reactor with an 18-50M tip and a 2.5-4 SOL buy went first (DAkZMwi3, pu5Q3GD2, 3fe3ppjS).
- **The 12, filled, would have made +1.15 SOL**: +0.65 on him, +0.50 on 3yrJE1A8. Two coins carry it (DAkZMwi3 +0.52,
  3fe3ppjS +0.48). The other 10 come to +0.16.
- **Over 5 days of his copies (558, tip >= 2M), every fix loses:**

  | fix | adds |
  |---|---|
  | a wider limit (10% / 15% / none) | -1.05 / -1.79 / -2.07 SOL |
  | any group of the refused ones (his tip, his size, the premium paid) | none positive except his 8-12M tips (+0.14 on 15) |
  | a second buy 1 / 2 / 3 slots after the refusal | -0.50 to -3.57 |
  | filling them with a short hold (create + 2 .. + 9) | -0.09 (+5) to -2.14 |

- **For 3yrJE1A8 (3 days, 252 copies) they're about even:** no limit +8.01 against +8.25. A second try at +1 to +3
  slots adds -0.13 to +0.29, all of it Oct 24.
- **What would pay is landing closer behind him.** At 5%, the copy right behind him (D = 0) makes +11.97 SOL over the
  5 days, against +5.51 at our real distances (D = 10: +8.07). The positions between are the create snipers'
  transactions, already in flight, so a bid doesn't buy them (section 9 of docs/17). Only the next-slot race is decided
  by tips, and there the reactors ahead of us paid 18-50M on a 2.5-4 SOL buy. On our 0.5 SOL that's 4-10% a fill.

Nothing here is built: on the larger history the refused coins lose as a group. The ones that won today are mostly two
large coins.

## 10. A wider limit (7.5% / 10%) only when a condition holds (`fails/condlimit.py`, `w3/condlimit.py`)

The 5-day copy of him (558 copies, tip >= 2M). For each condition known before the send, what the wider limit adds
over 5%. The conditions: his tip, his size, create + 0 / + 1, his block index, the virtual SOL his buy left, the launch
site, listed or not, and pairs of these.

- **The best ones add +0.2 to +0.47 SOL over 5 days** (on +5.51), positive on 3 of 5 days at most. At 7.5%: other hosts
  +0.47, tip >= 8M +0.26, vSOL >= 45 +0.26. At 10%: size >= 1.1 SOL +0.20, his index >= 400 +0.20. Most conditions lose,
  and 10% for every copy loses on every day (-1.05).
- **Picked on Oct 21-23 and scored on Oct 24-25, they don't hold:** the 7.5% pick (vSOL >= 45) adds -0.009, the 10%
  pick (size >= 1.1) -0.027.
- **3yrJE1A8's 2 SOL buys:** 7.5% adds +0.28, all of it Oct 24 (+0.48; then -0.03, -0.17). 10% adds -0.17.
- **Live, it wouldn't have caught today's winners either.** The refused coins that made money had moved +12% to +45%
  (DAkZMwi3 +32%, 3fe3ppjS +45%, 3cEYBUp4 and pu5Q3GD2 +12%). At 10%, the ones that would have filled come to +0.10
  SOL together (HZEe2Rvq, 9387oQyF, GUjeXnaQ, FNHRpj8P, 2DkuBnM6, 97NhoAsk, D7cp9jeq).

The limit stays at 5%.

## 11. The second search (Oct 26 evening, `wallets2/`)

**A data correction first.** 1,101 of the 4,808 tapes are coins priced in another token. Their trade events carry
0 SOL reserves, and the wallets buy them through a swap. They are outside the bot's class (SOL quote), and the screens
had valued them on a reserve rebuilt as if in SOL. Without them:

- **JBokzLFR**: none of its coins are affected. Its result stands (+2.05 out of sample, sold at create + 5).
- **3yrJE1A8**: 55 of its 131 taped coins were another quote. On its SOL coins (166 copies, Oct 24-26, sold at
  create + 29) the copy makes +6.32 SOL, against +8.25 with them: its 0.5 SOL buys +3.33 (+2.30 / +1.61 / -0.58), its
  2 SOL buys +2.99 (+2.59 / +0.20 / +0.20). `min_sol = 1.5` stays (`w3/solonly.py`).

**Eight new candidates** (positive in 3-4 of 4 blocks on the first screen) through the process, on SOL coins, their
own trades pulled (exit picked on Oct 25 18:00 - Oct 26 06:00, scored on 06:00 - 17:15):

| wallet | launch buys | exit | 1st half | 2nd half | verdict |
|---|---|---|---|---|---|
| H9kuqGKN | 79 | create + 9 | +1.89 | +0.48 (+0.017 a copy) | **passes**: positive at every hold (+3 .. +12) in both halves; our sale comes first on 76 of 79 |
| 6anjqMeW | 191 | create + 5 | +0.77 | +0.30 (+0.004) | too thin |
| dKMjeGJ4 | 233 | create + 5 | +0.18 | +0.14 (+0.002) | too thin |
| 8ksFdx6i | 152 | create + 12 | +0.87 | -0.27 | fails |
| CWUqhP2g | 243 | create + 9 | +1.33 | -0.62 | fails |
| 9qz7cWBq | 410 | create + 5 | -0.39 | -1.66 | fails |
| 9pdYXJGy | 208 | create + 7 | +0.35 | -0.71 | fails |
| AEgTuy8L | 178 | create + 5 | -1.60 | +0.26 | fails |

6anjqMeW's and dKMjeGJ4's first-screen results came from coins priced in another token. None of the eight funds the
creators of the coins it buys.

**The widened search** (`discover3.py`, 3,621 SOL tapes, picked on the first half, scored on the second): of 43 wallets
positive on the first half, the four we copy and H9kuqGKN held. Him +1.07, JBokzLFR +0.67, 3yrJE1A8 +0.51 and
H9kuqGKN +0.48 on the second half. Most of the rest decayed, as the first search found. The next five that held, on
partial tapes (D5BDDkBt +0.89 on 14, CxyKYmQo +0.78 on 24, AMDEmVoc +0.40 on 67, JBbbosQC +0.39 on 35, M8CcrseQ +0.21
on 24), are in stage 2.
