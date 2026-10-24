# 14. The tweet rule, re-measured on complete data: holds, stakes, costs and how he uses Twitter (2026-10-24 evening)

Scripts: `analysis/research/2026-10-24/corrected/` (`bt.py` is the engine, `bt_all.py` the tables below).

## 1. Two faults in the docs/13 data

- **No pump fee.** The trade files of most coins (`early/*.json`, all of Oct 23 and 314 of the 741 Oct 24 coins) stored
  no fee per trade, so a buy cost `sol` instead of `sol + 1.25%`. On the 427 coins that have both formats the value
  was overstated by **+1.6 points at hold 1 and +2.3 points at hold 10**. The fee is exactly 1.25% on every one of
  14,797 trades (0.95% protocol + 0.30% creator).
- **Cut and missing tapes.** Those files kept at most 150 transactions per coin, failed ones included. 1,113 of 2,440
  hit the cap, 620 of them before slot +10, and were then valued at the last price they had. 111 coins with more than
  40,000 transactions after the create were dropped entirely: the biggest winners.

`fulltape.py` re-pulled every rule-universe coin (tool host, template dev buy, a deployer he traded in the 7 days
before): every succeeded transaction from the create to slot +12, the fee and creator fee from each trade event, no
cap, paging back to the create however busy the coin. 2,260 coins, Oct 23 all day and Oct 24 00-12 UTC; 1,884 with a
first outside buy behind the bundle and post-bundle at most 40 SOL. Values are the real first outside buyer's
position (sales plus the rest marked at that slot's price, both fees counted), net of our measured round trip of
**1.2%** (6.2 mSOL on 0.5 SOL: buy tip 3M, sell 2M plus the 1M sender tip, base fees).

## 2. The rule, corrected

Sell at create + 10 as the bot does; "exit" sells a non-first coin at create + 3 (when the verdict arrives live).

| | coins / h | buy all, hold 10 | "exit" (live mode) | "on" (tweet decides, out of reach) |
|---|---|---|---|---|
| Oct 23 00-12 UTC | 41 | +4.07% [+2.39, +5.89] | +3.98% | +6.71% |
| **Oct 23 12-24 UTC** (never measured before) | 66 | **+3.88% [+2.70, +5.25]** | +2.58% | +4.67% |
| Oct 24 00-12 UTC | 50 | +3.10% [+1.03, +5.73] | +2.65% | +4.34% |
| **all, 1,881 coins** | | **+3.68% [+2.71, +4.69]** | +2.97% [+2.25, +3.65] | +5.12% |

- The rule holds in the afternoon and evening (12-24 UTC). The weakest 4 hours were 16-20 UTC (+0.9% in "exit").
- **The copy cut loses in every period.** A coin that is not first is worth −0.6% sold at create + 3 and +1.5% held
  to 10 (section 3). "exit" is 0.7 points a trade below buying everything and holding 10. Switch `tweet_gate` to
  `"log"`: the verdicts are still journaled, the trades are left alone.

## 3. Holds

**His rule.** Counted from the create, he sells at slot +6 or +10 and nothing else (146 of 154 trades with a full
tape; 2,930 of 3,407 since Oct 16). It is a setting per deployer: it repeats his previous one on that deployer 86% of
the time (83% after a +10, 12% after a +6) whether that trade won or lost. It is not the insiders' dump clock: on 112
of the 154 the creator set had not sold by slot +12.

| sell at create + H (all 1,881 coins, net 1.2%) | 2 | 4 | 6 | 8 | 9 | **10** | 12 |
|---|---|---|---|---|---|---|---|
| all | +0.2% | +1.7% | +2.5% | +3.4% | +3.7% | **+3.7%** | +3.6% |
| first coins on a fresh tweet | +1.0% | +3.0% | +4.0% | +4.8% | | +5.1% | +5.3% |
| the rest | −1.1% | −0.3% | +0.2% | +1.2% | | +1.5% | +1.1% |

On the slot +30 sample (every 6th coin, 300 with a value at every hold) the peak stays at 10-12 (+4.9%) and falls
after (+4.2% at 16, +3.6% at 30). First coins are flat from 12 to 18 (+6.9%), the rest peak at 10 (+2.6%) and lose
past 18.

**His per-deployer setting does not help us.** His last setting on the deployer (none: 10) makes +3.53% against
+3.68% for 10 for all; on his "6" deployers holding 10 is still better (+5.18% against +4.82% at 6), on his "10" ones
far better (+2.03% against −0.33%). Keep `operator_holds = false` and `hold_to_create_slots = 10`.

## 4. Stakes

His stake is 0.4 SOL x 2^tier (x1.5^a noise) by his confidence in the deployer. Tier from his median stake on the
deployer in the 7 days before the coin; value at hold 10 before our costs:

| tier | coins | value | win rate |
|---|---|---|---|
| 0 (and no history) | 1,620 | +4.44% | 53% |
| 1 | 170 | +5.91% | 62% |
| 2 | 91 | **+10.87%** | 52% |

Our cost per trade is fixed in lamports, so a smaller stake pays the same tips on less money:

| at a 50% land rate, about 627 trades a day | per trade | per day |
|---|---|---|
| **flat 0.5 SOL (now)** | **+18.2 mSOL** | **+11.4 SOL** |
| his ladder scaled to a 0.5 top (0.125 / 0.25 / 0.5) | +2.5 mSOL | +1.6 SOL |
| 0.25 / 0.375 / 0.5 | +8.0 mSOL | +5.0 SOL |
| tiers 1-2 only, at 0.5 | +32.0 mSOL | +2.8 SOL |

Every tier pays at the full 0.5, so the best scheme under a 0.5 ceiling is 0.5 everywhere. His ladder is copied by
sizing **up** on tiers 1-2 once the wallet carries it (1.0 / 2.0 SOL), not down on tier 0. The per-day column assumes
we win half of all rule coins at the backtest's value; live is below that so far, so read it as the ranking of the
schemes, not a forecast.

## 5. Costs

- **Sells: to his level.** He pays about 1.3M per sell and lands a slot after his usual one 22% of the time (we 7%);
  a slot late is worth +0.002 SOL on average (docs/08), and section 3 says a later sell loses nothing up to +12.
  `dump_tip_lamports` and `sell_tip_lamports` 2M -> 0.5M saves 1.5M per trade (0.3%).
- **Buys: measured, not copied blind.** In the 22 races we won on Oct 24 17:29-18:04, 95 buys failed right behind
  ours: 6 bid over 3M (we won on position), 34 bid 1.5-3M (in 6 of the 22 races), 55 bid 1.5M or less. The copied
  wallet was behind us on 10 of them: part of why we beat him is that we bid 3M where he bids 1.5M. At 1.5M up to a
  quarter of our wins were at risk, for 0.3% saved. `bid.ab_by_tier` (commit c03c4cc) puts the odd half of the mints
  on a second ladder; `rugsnipe-check` shows land rate and P&L per bid. Proposed second ladder: 1.5M on tier 0 (his
  bid), 6M and 12M on tiers 1 and 2 (his 6-24M, where the coins are worth +5.9% and +10.9%).

## 6. How he uses Twitter

He does not use the tweet's status to buy or to hold:
- on deployer-list tweet coins he attempts 55-56% of first coins and 40-43% of copies (Oct 23 and 24, 00-12 UTC);
- his create + H is the same for copies and first coins.

**His attempt rate rises only on first coins created 2 to 10 s after their tweet:**

| first coins, tweet to coin | coins | he attempted | value, hold 10, net 1.2% |
|---|---|---|---|
| under 2 s | 461 | 51% | +7.57% |
| **2-5 s** | 186 | **82%** | +6.10% |
| **5-10 s** | 165 | **67%** | +4.71% |
| 10-60 s | 255 | 49% | +1.67% |
| 1-60 min | 61 | 38% | −0.82% |

A metadata read would show him the under-2 s coins as well. A tweet feed with a lag of about 2 s explains it: a coin
whose name or ticker is in a tweet of the last few seconds is a first coin. `tweetfeed.py` rebuilds that rule on the
3,150 tweets behind the coins (fxtwitter; 498 deleted): with a 2 s lag and a 10 s window it calls 7 coins an hour,
95% of them truly first, and he attempted 75% of them against 49% of the rest. So that is how he picks. But it finds
only 21% of the first coins (31% with a 30 s window), and the coins it calls are worth +4.27% against +3.59% for
the rest of the list: **+0.7 points on 7 coins an hour, not worth a paid feed and a build yet.** The coins that are
first on their tweet are worth +5.1% against +1.5% (section 3), but that link is only in the metadata file, too late
for the buy (docs/13 section 4). The one source that could give it at create time is the launch tools' own feed of
the coins they deploy (uxento.io is itself a tweet-tracker site that deployers launch from).

**Tweet authors.** Ranked on Oct 23 00-12 and scored on the later coins: the authors he took 70% or more of made
**+6.44%** on their later coins (335; he took 79%), the ones he took 30% or less **+1.85%** (84). The frequent
authors look like the deployers' own accounts, so this overlaps his deployer list, and it is known only from the
metadata, after the buy, where holding to 10 still beats selling early.
